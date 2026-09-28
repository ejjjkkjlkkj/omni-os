"""Build NAV.BIN: the complete, navigable BIOS menu tree with natural speech.

The UEFI screen reader only sees the HII forms a firmware publishes to boot
applications (5 of 11 form sets on the ASUS M1603QA; the main Setup is absent).
This tool reads every form set straight from the firmware image, rebuilds the
menu tree (form sets -> pages -> questions, sub-menus, choice options, help)
and pre-renders every utterance with ST's neural voices (Kokoro-82M, 24 kHz):
English BIOS labels with an English voice, French screen-reader words with a
French voice. The reader then plays whole clips: no unit concatenation at run
time, hence no robotic voice. Read-only: nothing here can change a setting.

NAV.BIN (little endian)
  header 64 bytes:
    0 "QEVNAV01"  8 u32 node_count  12 u32 root  16 u32 nodes_off
    20 u32 links_off  24 u32 links_count  28 u32 clip_count  32 u32 clip_index_off
    36 u32 clip_data_off  40 u32 sample_rate  44 u32 sys_count  48 u32 sys_off
  node (32 bytes): u32 speak, u32 help, u32 enter, u32 target, u32 child_first,
    u16 child_count, u8 role, u8 flags, u32 option_first, u16 option_count,
    u16 option_default            (clip ids / node ids, NONE = 0xFFFFFFFF)
  links: u32 array (children = node ids, options = clip ids)
  sys: u32 clip ids of the fixed messages, in SYS order
  clip index: clip_count x {u32 offset from clip_data_off, u32 bytes}
  clip data: signed 16-bit mono PCM, 24 kHz

  python build_nav_bank.py M1603QAAS.308 NAV.BIN --st-neural C:\\st\\neural [--dry-run]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import pathlib
import struct
import sys
import uuid
from dataclasses import dataclass, field

LZMA_GUID = uuid.UUID("EE4E5898-3914-4259-9D6E-DC7BD79403CF").bytes_le
MAIN_SETUP = "7b59104a-c00d-4158-87ff-f04d6396a915"
KOKORO_RATE = 24000
RATE = 16000          # stored rate: ~175 MB for the whole BIOS instead of ~260 MB
UP = 48000 // RATE    # the HDA stream runs at 48 kHz
TAPS = 16             # FIR taps per polyphase branch for the 16 -> 48 kHz interpolation
# ONNX Runtime CPU output is bit-identical run to run for a fixed thread count
# (1 and 2 agree; 8 differs in the 3rd decimal). Fixed so NAV.BIN is reproducible.
RENDER_THREADS = 2
NONE = 0xFFFFFFFF

ROLE_CONTAINER, ROLE_SUBTITLE, ROLE_TEXT, ROLE_CHOICE, ROLE_CHECKBOX, ROLE_NUMBER, ROLE_PASSWORD, \
    ROLE_ACTION, ROLE_RESET, ROLE_MENU, ROLE_DATE, ROLE_TIME, ROLE_STRING, ROLE_ORDERED, ROLE_LAUNCH = range(15)
OP_ROLE = {0x02: ROLE_SUBTITLE, 0x03: ROLE_TEXT, 0x05: ROLE_CHOICE, 0x06: ROLE_CHECKBOX, 0x07: ROLE_NUMBER,
           0x08: ROLE_PASSWORD, 0x0C: ROLE_ACTION, 0x0D: ROLE_RESET, 0x0F: ROLE_MENU, 0x1A: ROLE_DATE,
           0x1B: ROLE_TIME, 0x1C: ROLE_STRING, 0x23: ROLE_ORDERED}
ROLE_FR = {ROLE_SUBTITLE: "section", ROLE_TEXT: "", ROLE_CHOICE: "liste de choix", ROLE_CHECKBOX: "case à cocher",
           ROLE_NUMBER: "valeur numérique", ROLE_PASSWORD: "mot de passe", ROLE_ACTION: "bouton",
           ROLE_RESET: "bouton de réinitialisation", ROLE_MENU: "sous-menu", ROLE_DATE: "date",
           ROLE_TIME: "heure", ROLE_STRING: "champ de texte", ROLE_ORDERED: "liste ordonnée"}
FLAG_READ_ONLY, FLAG_CONDITIONAL = 0x01, 0x02
PLACEHOLDERS = {"empty", "n/a", "na", "unknown", "not present", "not installed", "0", "0x0", "-"}
QUESTION_OPS = {0x05, 0x06, 0x07, 0x08, 0x0C, 0x0F, 0x1A, 0x1B, 0x1C, 0x23}
# Expression opcodes that may follow SUPPRESS_IF / DISABLE_IF / GRAY_OUT_IF.
EXPRESSION_OPS = set(range(0x12, 0x19)) | set(range(0x2B, 0x2F)) | set(range(0x33, 0x3A)) | \
    set(range(0x40, 0x5A)) | {0x5C, 0x5E, 0x5F, 0x60, 0x61, 0x62}

# Fixed messages, same order as the C enum NAV_SYS_*.
SYS = [
    ("welcome", "Lecteur d'écran du BIOS. Flèches haut et bas pour parcourir, Entrée pour ouvrir un sous-menu, "
                "Échap pour revenir, H pour l'aide, gauche et droite pour écouter les options, R pour répéter, "
                "Espace pour savoir où vous êtes, Page haut et Page bas pour changer de section. "
                "Lecture seule : aucun réglage n'est modifié."),
    ("top", "Début de la liste."),
    ("bottom", "Fin de la liste."),
    ("no_help", "Pas d'aide pour cet élément."),
    ("not_menu", "Ce n'est pas un sous-menu. Lecture seule : ce réglage ne peut pas être modifié ici."),
    ("no_options", "Pas d'options pour cet élément."),
    ("quit_confirm", "Appuyez encore sur Échap pour quitter le lecteur d'écran."),
    ("goodbye", "Fermeture du lecteur d'écran."),
    ("empty", "Menu vide."),
    ("launching", "Démarrage de l'environnement de récupération accessible. Le Narrateur parlera dans environ une minute."),
    ("launch_missing", "Environnement de récupération accessible introuvable sur cette clé."),
]


# ---------------------------------------------------------------- extraction
def sections(image: bytes) -> list[bytes]:
    out = [image]

    def walk(buf: bytes, depth: int) -> None:
        pos = 0
        while (i := buf.find(LZMA_GUID, pos)) >= 0:
            pos = i + 16
            if i < 4 or buf[i - 1] != 0x02:  # EFI_SECTION_GUID_DEFINED
                continue
            size = int.from_bytes(buf[i - 4:i - 1], "little")
            offset = int.from_bytes(buf[i + 16:i + 18], "little")
            try:
                data = lzma.LZMADecompressor(format=lzma.FORMAT_ALONE).decompress(buf[i - 4 + offset:i - 4 + size])
            except lzma.LZMAError:
                continue
            out.append(data)
            if depth < 3:
                walk(data, depth + 1)

    walk(image, 0)
    return out


def parse_strings(pkg: bytes) -> dict[int, str]:
    """HII string package (SIBT blocks) -> {string id: text}."""
    p = struct.unpack_from("<I", pkg, 8)[0]
    sid, strings = 1, {}

    def ucs2(at: int) -> tuple[str, int]:
        end = at
        while pkg[end:end + 2] != b"\0\0":
            end += 2
        return pkg[at:end].decode("utf-16-le", "replace"), end + 2

    while p < len(pkg):
        t = pkg[p]
        if t == 0x14:
            strings[sid], p = ucs2(p + 1); sid += 1
        elif t == 0x15:
            n = struct.unpack_from("<H", pkg, p + 1)[0]; p += 3
            for _ in range(n):
                strings[sid], p = ucs2(p); sid += 1
        elif t == 0x16:
            strings[sid], p = ucs2(p + 2); sid += 1
        elif t == 0x17:
            n = struct.unpack_from("<H", pkg, p + 2)[0]; p += 4
            for _ in range(n):
                strings[sid], p = ucs2(p); sid += 1
        elif t == 0x20:
            strings[sid] = strings.get(struct.unpack_from("<H", pkg, p + 1)[0], ""); sid += 1; p += 3
        elif t == 0x21:
            sid += pkg[p + 1]; p += 2
        elif t == 0x22:
            sid += struct.unpack_from("<H", pkg, p + 1)[0]; p += 3
        elif t == 0x30:
            p += pkg[p + 2]
        elif t == 0x31:
            p += struct.unpack_from("<H", pkg, p + 2)[0]
        elif t == 0x32:
            p += struct.unpack_from("<I", pkg, p + 2)[0]
        else:  # SIBT_END, SCSU or unknown block
            break
    return strings


def ifr_ops(body: bytes):
    """Yield (offset, opcode, length, scope) over a forms package body (after the 4-byte header)."""
    q = 0
    while q + 2 <= len(body):
        op, ln = body[q], body[q + 1] & 0x7F
        if ln < 2 or q + ln > len(body):
            raise ValueError(f"bad IFR opcode length at {q}")
        yield q, op, ln, bool(body[q + 1] & 0x80)
        q += ln


def string_tokens(body: bytes) -> set[int]:
    toks = set()
    for q, op, ln, _ in ifr_ops(body):
        if op in OP_ROLE and ln >= 6:
            toks.update(struct.unpack_from("<HH", body, q + 2))
        elif op in (0x01, 0x09) and ln >= 6:
            toks.add(struct.unpack_from("<H", body, q + (4 if op == 0x01 else 2))[0])
        elif op == 0x0E and ln >= 22:
            toks.add(struct.unpack_from("<H", body, q + 18)[0])
    toks.discard(0)
    return toks


def extract(image: bytes) -> list[dict]:
    """Every form set in the image with the en-US string table it uses."""
    formsets, strings_by_blob = {}, []
    for bi, blob in enumerate(sections(image)):
        tables = []
        for i in range(len(blob) - 52):
            t = blob[i + 3]
            if t not in (0x02, 0x04):
                continue
            length = int.from_bytes(blob[i:i + 3], "little")
            if not 16 < length < 0x400000 or i + length > len(blob):
                continue
            pkg = blob[i:i + length]
            if t == 0x02 and pkg[4] == 0x0E and pkg[5] & 0x80:
                body = pkg[4:]
                try:
                    list(ifr_ops(body))
                except ValueError:
                    continue
                guid = str(uuid.UUID(bytes_le=body[2:18]))
                formsets.setdefault(guid, {"guid": guid, "body": body, "blob": bi, "offset": i})
            elif t == 0x04:
                hs = struct.unpack_from("<I", pkg, 4)[0]
                if 46 <= hs < 200 and pkg[46:51].lower() == b"en-us":
                    try:
                        tables.append((i, parse_strings(pkg)))
                    except (struct.error, IndexError):
                        continue
        strings_by_blob.append(tables)
    # Form sets without their own package list (AMD CBS/PBS/Overclocking) share
    # a section with several string tables. Token counts alone pick the wrong
    # one (any big table resolves most ids); the right table defines exactly as
    # many strings as the highest id the form set uses.
    all_tables = [t for ts in strings_by_blob for _, t in ts]
    out = []
    for fs in formsets.values():
        toks = string_tokens(fs["body"])
        top = max(toks, default=0)
        best, best_key, best_hit = {}, None, 0
        for table in all_tables:
            hit = sum(1 for t in toks if table.get(t, "").strip())
            key = (hit >= 0.95 * len(toks), -abs(max(table, default=0) - top), hit)
            if best_key is None or key > best_key:
                best, best_key, best_hit = table, key, hit
        fs["strings"] = best
        fs["coverage"] = (best_hit, len(toks))
        out.append(fs)
    return out


# ---------------------------------------------------------------- tree
@dataclass
class Node:
    role: int
    label: str = ""
    value: str = ""        # TEXT's second string, e.g. the BIOS version
    help: str = ""
    flags: int = 0
    options: list[str] = field(default_factory=list)
    default: int = -1
    target: object = None  # ("form", formset guid, form id) or ("formset", guid)
    children: list = field(default_factory=list)
    title: str = ""        # containers: announced when entered
    lang: str = "en"       # language of label/title (BIOS text is English)
    help_lang: str = "en"


def clean(text: str) -> str:
    return " ".join(text.replace("\u00a0", " ").split())


def build_formset(fs: dict) -> tuple[str, dict[int, Node], list[int], set[int]]:
    body, S = fs["body"], fs["strings"]
    s = lambda tok: clean(S.get(tok, "")) if tok else ""
    forms: dict[int, Node] = {}
    order: list[int] = []
    referenced: set[int] = set()
    title = s(struct.unpack_from("<H", body, 18)[0])
    ops = list(ifr_ops(body))
    stack: list[tuple[int, object]] = []   # (opcode, payload) per open scope
    form: Node | None = None
    question: Node | None = None
    for idx, (q, op, ln, scope) in enumerate(ops):
        hidden = any(k in (0x0A, 0x1E) and v == "always" for k, v in stack)
        conditional = any(k in (0x0A, 0x19, 0x1E) and v != "always" for k, v in stack)
        payload = None
        if op == 0x29:  # END
            if stack:
                k, v = stack.pop()
                if k in QUESTION_OPS and question is v:
                    question = None
                if k in (0x01, 0x5D):
                    form = None
            continue
        if op in (0x01, 0x5D) and ln >= 6:  # FORM / FORM_MAP
            fid = struct.unpack_from("<H", body, q + 2)[0]
            ftitle = s(struct.unpack_from("<H", body, q + 4)[0]) if op == 0x01 else \
                (s(struct.unpack_from("<H", body, q + 6)[0]) if ln >= 8 else "")
            form = forms.setdefault(fid, Node(ROLE_CONTAINER, title=ftitle or title))
            if fid not in order:
                order.append(fid)
        elif op in (0x0A, 0x19, 0x1E):  # SUPPRESS_IF / GRAY_OUT_IF / DISABLE_IF
            nxt = ops[idx + 1][1] if idx + 1 < len(ops) else None
            after = ops[idx + 2][1] if idx + 2 < len(ops) else None
            payload = "always" if nxt == 0x46 and after not in EXPRESSION_OPS else "expr"
        elif op in OP_ROLE and form is not None and ln >= 6:
            prompt, helptok = struct.unpack_from("<HH", body, q + 2)
            node = Node(OP_ROLE[op], label=s(prompt), help=s(helptok))
            if conditional:
                node.flags |= FLAG_CONDITIONAL
            if op in QUESTION_OPS and ln >= 13 and body[q + 12] & 0x01:
                node.flags |= FLAG_READ_ONLY
            # Grey-out conditions read runtime variables; only a constant TRUE
            # is known to hold offline.
            if any(k == 0x19 and v == "always" for k, v in stack):
                node.flags |= FLAG_READ_ONLY
            if op == 0x03 and ln >= 8:
                node.value = s(struct.unpack_from("<H", body, q + 6)[0])
            # Values the firmware fills in at run time are placeholders in the image.
            if node.value.lower() in PLACEHOLDERS or "%" in node.value:
                node.value = ""
            if node.label.lower() in PLACEHOLDERS or "%" in node.label:
                node.label = ""
            if op == 0x06 and ln >= 14:
                node.options = ["Disabled", "Enabled"]
                node.default = 1 if body[q + 13] & 0x01 else 0
            if op == 0x0F:
                if ln >= 15:
                    fid = struct.unpack_from("<H", body, q + 13)[0]
                    if ln >= 33:
                        g = str(uuid.UUID(bytes_le=body[q + 17:q + 33]))
                        node.target = ("formset", g) if fid == 0 else ("form", g, fid)
                    elif fid:
                        node.target = ("form", fs["guid"], fid)
                        referenced.add(fid)
            # Runtime-conditional variants of one text ("... Status: INSTALLED" /
            # "NOT INSTALLED"): offline we cannot tell which holds, keep the label.
            twin = next((c for c in form.children if c.role == ROLE_TEXT == node.role and node.label
                         and c.label == node.label), None)
            if twin is not None:
                twin.value = "" if twin.value != node.value else twin.value
            elif not hidden and (node.label or node.value):
                form.children.append(node)
            payload = node
            if op in QUESTION_OPS:
                question = node
        elif op == 0x09 and question is not None and ln >= 6:  # ONE_OF_OPTION
            text = s(struct.unpack_from("<H", body, q + 2)[0])
            if text:
                if body[q + 4] & 0x10 and question.default < 0:
                    question.default = len(question.options)
                question.options.append(text)
        if scope:
            stack.append((op, payload))
    return title, forms, order, referenced


def launch_node() -> Node:
    """First item of the main menu: hand over to the accessible Windows RE on the key."""
    return Node(ROLE_LAUNCH, label="Démarrer l'environnement de récupération Windows accessible", lang="fr",
                help="Démarre l'environnement de récupération de Windows avec le Narrateur et la voix ST. "
                     "Pour démarrer Windows normalement, appuyez deux fois sur Échap au menu principal.",
                help_lang="fr")


def build_tree(formsets: list[dict]) -> Node:
    root = Node(ROLE_CONTAINER, title="Menus du BIOS")
    tops, containers = [], {}
    for fs in formsets:
        title, forms, order, referenced = build_formset(fs)
        if not order:
            continue
        for fid, f in forms.items():
            containers[("form", fs["guid"], fid)] = f
        # The first form is the entry page. Forms no reference leads to are
        # normally hidden (AMD debug pages...): keep them reachable, but apart.
        fs_node = forms[order[0]]
        fs_node.title = title or fs_node.title
        orphans = [fid for fid in order[1:] if fid not in referenced]
        if orphans:
            hidden = Node(ROLE_CONTAINER, title="Pages cachées", lang="fr")
            for fid in orphans:
                f = forms[fid]
                hidden.children.append(Node(ROLE_MENU, label=f.title or title, target=("form", fs["guid"], fid)))
            fs_node.children.append(Node(ROLE_MENU, label="Pages cachées", lang="fr", target=hidden,
                                         help="Pages que ce BIOS n'affiche normalement pas.", help_lang="fr"))
        containers[("formset", fs["guid"])] = fs_node
        questions = sum(1 for f in forms.values() for c in f.children if c.role not in (ROLE_SUBTITLE, ROLE_TEXT))
        tops.append((fs["guid"] != MAIN_SETUP, -questions, Node(
            ROLE_MENU, label=title or "Formulaire sans titre", lang="en" if title else "fr",
            target=("formset", fs["guid"]), help=f"{plural(len(forms), 'page')}, {plural(questions, 'réglage')}.",
            help_lang="fr")))
    root.children = [launch_node()] + [n for *_, n in sorted(tops, key=lambda t: t[:2])]
    # Resolve targets to container objects; drop dangling references.
    seen = set()

    def resolve(n: Node) -> None:
        if id(n) in seen:
            return
        seen.add(id(n))
        for c in n.children:
            if c.role == ROLE_MENU and isinstance(c.target, tuple):
                c.target = containers.get(c.target)
            if c.role == ROLE_MENU and not isinstance(c.target, Node):
                c.role = ROLE_ACTION  # a reference with no page (Save & Exit...) acts as a button
            if isinstance(c.target, Node):
                resolve(c.target)
    resolve(root)
    return root


# ---------------------------------------------------------------- utterances
def segments_for(n: Node) -> list[tuple[str, str]]:
    """(lang, text) segments spoken when the item gets focus."""
    segs = []
    label = n.label or n.value
    if n.role == ROLE_TEXT:
        segs.append((n.lang, label))
        if n.value and n.label:
            segs.append(("en", n.value))
    else:
        segs.append((n.lang, label))
        role = ROLE_FR.get(n.role, "")
        if role:
            segs.append(("fr", role))
    if n.options and 0 <= n.default < len(n.options):
        segs += [("fr", "par défaut"), ("en", n.options[n.default])]
    if n.flags & FLAG_READ_ONLY and n.role not in (ROLE_TEXT, ROLE_SUBTITLE):
        segs.append(("fr", "grisé"))
    return segs


def plural(n: int, word: str) -> str:
    if n == 0:
        return f"aucun {word}" if word != "page" else "aucune page"
    if n == 1:
        return f"une {word}" if word == "page" else f"un {word}"
    return f"{n} {word}s"


def count_fr(n: int) -> str:
    return plural(n, "élément")


def walk(root: Node):
    """Every node reachable from root, breadth first, each once."""
    order, seen, queue = [], set(), [root]
    while queue:
        n = queue.pop(0)
        if id(n) in seen:
            continue
        seen.add(id(n))
        order.append(n)
        for c in n.children:
            queue.append(c)
            if isinstance(c.target, Node):
                queue.append(c.target)
    return order


class Clips:
    def __init__(self):
        self.utterances: list[tuple[tuple[str, str], ...]] = []
        self.ids: dict[tuple, int] = {}

    def add(self, segs) -> int:
        key = tuple((l, t) for l, t in segs if t)
        if not key:
            return NONE
        if key not in self.ids:
            self.ids[key] = len(self.utterances)
            self.utterances.append(key)
        return self.ids[key]


def plan(root: Node):
    clips = Clips()
    nodes = walk(root)
    index = {id(n): i for i, n in enumerate(nodes)}
    sys_ids = [clips.add([("fr", text)]) for _, text in SYS]
    records, links = [], []
    for n in nodes:
        if n.role == ROLE_CONTAINER:
            speak = clips.add([(n.lang, n.title)]) if n.title else NONE
            enter = clips.add(([(n.lang, n.title)] if n.title else []) + [("fr", count_fr(len(n.children)))])
            help_id = NONE
        else:
            speak = clips.add(segments_for(n))
            enter = NONE
            help_id = clips.add([(n.help_lang, n.help)]) if n.help and n.help != n.label else NONE
        child_first = len(links)
        links += [index[id(c)] for c in n.children]
        option_first = len(links)
        links += [clips.add([("en", o)] + ([("fr", "par défaut")] if i == n.default else []))
                  for i, o in enumerate(n.options)]
        target = index[id(n.target)] if isinstance(n.target, Node) else NONE
        records.append((speak, help_id, enter, target, child_first, len(n.children), n.role, n.flags,
                        option_first, len(n.options), n.default if n.default >= 0 else 0xFFFF))
    return records, links, sys_ids, clips


# ---------------------------------------------------------------- rendering
class Renderer:
    VOICES = {"en": ("af_heart", "en-us", 1.1), "fr": ("ff_siwis", "fr-fr", 1.1)}

    def __init__(self, neural: pathlib.Path, cache: pathlib.Path, threads: int = RENDER_THREADS):
        import numpy as np
        self.np = np
        self.neural = neural
        self.threads = threads
        self.cache = cache
        cache.mkdir(parents=True, exist_ok=True)

    def _create(self, text: str, voice: str, code: str, speed: float):
        """Kokoro's vocoder has 11 unseeded RandomUniform/NormalLike ops whose
        generator is created with the session and then advances on every run.
        A fresh session per segment, seeded from the segment itself, makes each
        clip independent of process, order and machine."""
        import onnxruntime as rt
        from kokoro_onnx import Kokoro
        seed = int.from_bytes(hashlib.sha256(f"{voice}|{code}|{speed}|{text}".encode()).digest()[:4], "little")
        rt.set_seed(seed)
        o = rt.SessionOptions()
        o.intra_op_num_threads = self.threads
        k = Kokoro.from_session(rt.InferenceSession(str(self.neural / "models/kokoro-v1.0.onnx"), sess_options=o,
                                                    providers=["CPUExecutionProvider"]),
                                str(self.neural / "models/voices-v1.0.bin"))
        return k.create(text, voice=voice, speed=speed, lang=code)

    def segment(self, lang: str, text: str):
        np = self.np
        voice, code, speed = self.VOICES[lang]
        key = hashlib.sha256(f"seeded-v2|{voice}|{code}|{speed}|{RATE}|{text}".encode()).hexdigest()[:24]
        path = self.cache / f"{key}.npy"
        if path.exists():
            return np.load(path)
        from scipy.signal import resample_poly
        x, rate = self._create(text, voice, code, speed)
        assert rate == KOKORO_RATE
        x = resample_poly(np.asarray(x, dtype=np.float64), RATE, KOKORO_RATE)
        x -= x.mean()
        env = np.abs(x) > 0.01
        if env.any():  # trim silences, keep 15 ms margins
            m = RATE * 15 // 1000
            a, b = int(np.argmax(env)), len(x) - int(np.argmax(env[::-1]))
            x = x[max(0, a - m):min(len(x), b + m)]
        # Loudness: same speech RMS for both voices, peaks kept well below full scale.
        voiced = x[np.abs(x) > 0.02]
        rms = float(np.sqrt(np.mean(voiced ** 2))) if voiced.size else 1.0
        x *= 0.12 / max(rms, 1e-6)
        peak = float(np.abs(x).max()) if x.size else 0.0
        if peak > 0.8:
            x *= 0.8 / peak
        np.save(path, x)
        return x

    def utterance(self, segs) -> bytes:
        np = self.np
        parts = []
        gap = np.zeros(int(0.14 * RATE))
        for i, (lang, text) in enumerate(segs):
            x = self.segment(lang, text).copy()
            fade = min(RATE // 250, len(x) // 2)  # 4 ms fades: no click at segment joins
            if fade:
                x[:fade] *= np.linspace(0, 1, fade)
                x[-fade:] *= np.linspace(1, 0, fade)
            if i:
                parts.append(gap)
            parts.append(x)
        y = np.concatenate(parts) if parts else np.zeros(1)
        return np.clip(np.round(y * 32767), -32767, 32767).astype("<i2").tobytes()


def _prerender_worker(job) -> int:
    neural, cache, threads, segs = job
    r = Renderer(pathlib.Path(neural), pathlib.Path(cache), threads)
    for lang, text in segs:
        r.segment(lang, text)
    return len(segs)


def prerender(segments: list[tuple[str, str]], neural: pathlib.Path, cache: pathlib.Path, workers: int) -> None:
    """Fill the segment cache with several Kokoro processes (one session is ~single core bound)."""
    import multiprocessing
    if workers <= 1:
        return
    # Longest texts first, dealt round-robin so every worker gets a similar load.
    todo = sorted(set(segments), key=lambda s: -len(s[1]))
    chunks = [todo[i::workers * 4] for i in range(workers * 4)]
    threads = RENDER_THREADS
    done = 0
    with multiprocessing.Pool(workers) as pool:
        for n in pool.imap_unordered(_prerender_worker, [(str(neural), str(cache), threads, c) for c in chunks if c]):
            done += n
            print(f"prerender {done}/{len(todo)}", flush=True)


def interpolation_filter() -> list[int]:
    """Q15 polyphase FIR for RATE -> 48 kHz: windowed sinc cut at 0.475*RATE.

    Coefficient p + UP*k belongs to branch p (output sample UP*i + p) and
    multiplies input sample i - k. Each branch is normalised to unity DC gain,
    so the branches cannot beat into a tone at RATE."""
    import math
    n = UP * TAPS
    fc = 0.475 * RATE / 48000.0
    h = []
    for i in range(n):
        t = i - (n - 1) / 2
        sinc = 1.0 if t == 0 else math.sin(2 * math.pi * fc * t) / (2 * math.pi * fc * t)
        w = 0.42 - 0.5 * math.cos(2 * math.pi * i / (n - 1)) + 0.08 * math.cos(4 * math.pi * i / (n - 1))
        h.append(2 * fc * sinc * w)
    q = []
    for i in range(n):
        branch = sum(h[p] for p in range(i % UP, n, UP))
        q.append(h[i] / branch)
    out = [int(round(v * 32767)) for v in q]
    for p in range(UP):  # exact unity per branch after rounding
        idx = max(range(p, n, UP), key=lambda j: abs(out[j]))
        out[idx] += 32767 - sum(out[j] for j in range(p, n, UP))
    assert all(-32768 <= v <= 32767 for v in out)
    return out


def write_nav(out: pathlib.Path, records, links, sys_ids, pcm: list[bytes]) -> dict:
    header_size = 64
    fir = interpolation_filter()
    nodes_off = header_size
    links_off = nodes_off + 32 * len(records)
    sys_off = links_off + 4 * len(links)
    filter_off = sys_off + 4 * len(sys_ids)
    clip_index_off = filter_off + 2 * len(fir)
    clip_data_off = (clip_index_off + 8 * len(pcm) + 4095) & ~4095
    blob = bytearray(b"QEVNAV01")
    blob += struct.pack("<IIIIIIIIIIIIHH", len(records), 0, nodes_off, links_off, len(links), len(pcm),
                        clip_index_off, clip_data_off, RATE, len(sys_ids), sys_off, filter_off, UP, TAPS)
    blob += bytes(header_size - len(blob))
    for r in records:
        blob += struct.pack("<IIIIIHBBIHH", *r)
    blob += struct.pack(f"<{len(links)}I", *links)
    blob += struct.pack(f"<{len(sys_ids)}I", *sys_ids)
    blob += struct.pack(f"<{len(fir)}h", *fir)
    offset = 0
    for clip in pcm:
        blob += struct.pack("<II", offset, len(clip))
        offset += len(clip)
    blob += bytes(clip_data_off - len(blob))
    for clip in pcm:
        blob += clip
    out.write_bytes(blob)
    return {"bytes": len(blob), "sha256": hashlib.sha256(blob).hexdigest(),
            "seconds": round(offset / 2 / RATE, 1)}


def dump(root: Node, path: pathlib.Path) -> None:
    """Human-readable tree (each container expanded once)."""
    lines, seen = [], set()

    def rec(n: Node, depth: int) -> None:
        for c in n.children:
            segs = " | ".join(t for _, t in segments_for(c))
            lines.append("  " * depth + segs + (f"  [options: {', '.join(c.options)}]" if c.options else ""))
            if isinstance(c.target, Node) and id(c.target) not in seen and depth < 12:
                seen.add(id(c.target))
                rec(c.target, depth + 1)
    rec(root, 0)
    path.write_text("\n".join(lines), encoding="utf-8")


def environment(neural: pathlib.Path) -> dict:
    """Everything that decides the bytes of NAV.BIN, for reproduction."""
    from importlib import metadata
    def sha(p: pathlib.Path) -> str:
        h = hashlib.sha256()
        with p.open("rb") as f:
            for block in iter(lambda: f.read(1 << 20), b""):
                h.update(block)
        return h.hexdigest()
    pkgs = {}
    for name in ("kokoro-onnx", "onnxruntime", "numpy", "scipy", "espeakng-loader", "phonemizer-fork", "misaki"):
        try:
            pkgs[name] = metadata.version(name)
        except metadata.PackageNotFoundError:
            pass
    return {"python": sys.version.split()[0], "packages": pkgs, "render_threads": RENDER_THREADS,
            "models": {p.name: sha(p) for p in (neural / "models/kokoro-v1.0.onnx", neural / "models/voices-v1.0.bin")},
            "builder_sha256": sha(pathlib.Path(__file__))}


def synthetic_tree() -> Node:
    """Small fixed menu tree for CI and QEMU tests (no firmware image needed)."""
    sub = Node(ROLE_CONTAINER, title="Advanced")
    sub.children = [Node(ROLE_SUBTITLE, label="Devices"),
                    Node(ROLE_CHOICE, label="SVM Mode", help="Virtualization.", options=["Disabled", "Enabled"], default=1),
                    Node(ROLE_CHECKBOX, label="Fast Boot", options=["Disabled", "Enabled"], default=0),
                    Node(ROLE_SUBTITLE, label="Storage"),
                    Node(ROLE_NUMBER, label="Timeout")]
    root = Node(ROLE_CONTAINER, title="Setup")
    root.children = [launch_node(),
                     Node(ROLE_TEXT, label="BIOS Version", value="308"),
                     Node(ROLE_MENU, label="Advanced", target=sub),
                     Node(ROLE_ACTION, label="Save Changes")]
    return root


def synthetic_pcm(key) -> bytes:
    """Deterministic tone per utterance: 50 ms fades, frequency from the text hash."""
    import math
    digest = hashlib.sha256(repr(key).encode()).digest()
    freq = 300 + digest[0] * 4
    n = RATE * (300 + digest[1]) // 1000
    fade = RATE // 20
    out = []
    for i in range(n):
        g = min(1.0, i / fade, (n - 1 - i) / fade)
        out.append(int(round(9000 * g * math.sin(2 * math.pi * freq * i / RATE))))
    return struct.pack(f"<{n}h", *out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("image", help="firmware image, or 'synthetic' for the CI test tree")
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--st-neural", type=pathlib.Path, default=pathlib.Path(r"C:\st\neural"))
    ap.add_argument("--cache", type=pathlib.Path)
    ap.add_argument("--manifest", type=pathlib.Path)
    ap.add_argument("--dump", type=pathlib.Path)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--workers", type=int, default=6)
    a = ap.parse_args()
    if a.image == "synthetic":
        records, links, sys_ids, clips = plan(synthetic_tree())
        info = write_nav(a.out, records, links, sys_ids, [synthetic_pcm(u) for u in clips.utterances])
        info.update(nodes=len(records), clips=len(clips.utterances))
        print(json.dumps(info, indent=1))
        return 0
    a.image = pathlib.Path(a.image)
    image = a.image.read_bytes()
    formsets = extract(image)
    root = build_tree(formsets)
    records, links, sys_ids, clips = plan(root)
    info = {"image": a.image.name, "image_sha256": hashlib.sha256(image).hexdigest(),
            "formsets": [{"guid": f["guid"], "strings_resolved": f"{f['coverage'][0]}/{f['coverage'][1]}"}
                         for f in formsets],
            "nodes": len(records), "clips": len(clips.utterances),
            "segments": len({s for u in clips.utterances for s in u}),
            "characters": sum(len(t) for u in clips.utterances for _, t in u)}
    if a.dump:
        dump(root, a.dump)
    print(json.dumps(info, indent=1, ensure_ascii=False), flush=True)
    if a.dry_run:
        return 0
    cache = a.cache or a.out.with_suffix(".cache")
    prerender([s for u in clips.utterances for s in u], a.st_neural, cache, a.workers)
    r = Renderer(a.st_neural, cache)
    pcm = []
    for i, u in enumerate(clips.utterances):
        pcm.append(r.utterance(u))
        if i % 50 == 0:
            print(f"{i}/{len(clips.utterances)} {' | '.join(t for _, t in u)[:70]!r}", flush=True)
    info.update(write_nav(a.out, records, links, sys_ids, pcm))
    info["voices"] = {k: v[0] for k, v in Renderer.VOICES.items()}
    info["engine"] = "ST neural (Kokoro-82M v1.0)"
    info["environment"] = environment(a.st_neural)
    if a.manifest:
        a.manifest.write_text(json.dumps(info, indent=1, ensure_ascii=False), encoding="utf-8")
    print(json.dumps(info, indent=1, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
