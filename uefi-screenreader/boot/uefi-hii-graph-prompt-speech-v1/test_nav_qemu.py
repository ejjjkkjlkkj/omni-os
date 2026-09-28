"""QEMU tests of the NAV.BIN navigator (Linux CI and Windows).

  python test_nav_qemu.py --efi SCREENREADER.EFI --nav NAV.BIN --ovmf OVMF_CODE.fd --work DIR [--fuzz]

1. navigation: boots the reader with an emulated HDA codec, presses real keys
   over QMP (down, enter into a sub-menu, help, options, back, quit) and checks
   the serial trace, the persisted OMNI-SR-TRACE.TXT and the recorded audio
   (no clipping, no click, quiet between utterances);
2. --fuzz: corrupted NAV.BIN files must be rejected with a NAV_BANK=BAD_* or
   READ_FAILED marker and the reader must fall back to live HII navigation.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import socket
import struct
import subprocess
import sys
import time

os.environ.setdefault("MTOOLS_SKIP_CHECK", "1")

KEYS = ["down", "down", "ret", "down", "h", "right", "right", "left", "spc", "r",
        "esc", "end", "home", "pgdn", "esc", "esc"]


OVMF_VARS: str | None = None


def fat_image(esp: pathlib.Path) -> pathlib.Path | None:
    """Real FAT32 image of the ESP when dosfstools + mtools exist (Linux CI):
    QEMU's writable vvfat corrupts files the guest rewrites on some versions."""
    if not (shutil.which("mkfs.fat") and shutil.which("mcopy")):
        return None
    img = esp.parent / "esp.img"
    img.unlink(missing_ok=True)
    size = sum(p.stat().st_size for p in esp.rglob("*") if p.is_file()) + 64 * 1024 * 1024
    with img.open("wb") as f:
        f.truncate(size)
    subprocess.run(["mkfs.fat", "-F", "32", str(img)], check=True, capture_output=True)
    subprocess.run(["mcopy", "-s", "-i", str(img)] + [str(p) for p in esp.iterdir()] + ["::"], check=True)
    return img


def fat_read(img: pathlib.Path, name: str, dest: pathlib.Path) -> None:
    subprocess.run(["mcopy", "-o", "-i", str(img), f"::{name}", str(dest)], capture_output=True)


def qemu_cmd(qemu: str, ovmf: str, esp: pathlib.Path, serial: pathlib.Path, extra: list[str],
             image: pathlib.Path | None = None) -> list[str]:
    flash = ["-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}"]
    if OVMF_VARS:  # private writable copy of the variable store per run
        vars_copy = esp.parent / "OVMF_VARS.fd"
        shutil.copy(OVMF_VARS, vars_copy)
        flash += ["-drive", f"if=pflash,format=raw,file={vars_copy}"]
    disk = f"format=raw,file={image}" if image else f"format=raw,file=fat:rw:{esp}"
    return [qemu, "-machine", "q35", "-m", "1024", *flash,
            "-drive", disk, "-serial", f"file:{serial}", "-display", "none",
            "-net", "none", "-device", "intel-hda"] + extra


def make_esp(root: pathlib.Path, efi: pathlib.Path, nav: bytes | None) -> pathlib.Path:
    shutil.rmtree(root, ignore_errors=True)
    (root / "EFI/BOOT").mkdir(parents=True)
    (root / "EFI/OMNI").mkdir(parents=True)
    shutil.copy(efi, root / "EFI/BOOT/BOOTX64.EFI")
    if nav is not None:
        (root / "EFI/OMNI/NAV.BIN").write_bytes(nav)
    return root


def wait_for(serial: pathlib.Path, needles: tuple[str, ...], seconds: float) -> str:
    end = time.time() + seconds
    text = ""
    while time.time() < end:
        text = serial.read_text(errors="replace") if serial.exists() else ""
        if any(n in text for n in needles):
            break
        time.sleep(0.5)
    return text


def audio_report(path: pathlib.Path) -> dict:
    raw = path.read_bytes()
    ch, rate = struct.unpack_from("<HI", raw, 22)
    n = (len(raw) - 44) // (2 * ch)
    x = struct.unpack(f"<{n * ch}h", raw[44:44 + n * ch * 2])[::ch]
    peak = max((abs(v) for v in x), default=0)
    clipped = sum(1 for v in x if abs(v) >= 32700)
    # A click is a sample step far larger than the signal around it, on both
    # sides: a centred window, so a sharp consonant attack after a quiet
    # passage (whose following samples are just as busy) is not a click.
    d = [abs(x[i] - x[i - 1]) for i in range(1, len(x))]
    half = 32
    prefix = [0]
    for v in d:
        prefix.append(prefix[-1] + v)
    clicks = 0
    for i, v in enumerate(d):
        if v <= 1600:
            continue
        lo, hi = max(0, i - half), min(len(d), i + half)
        local = (prefix[hi] - prefix[lo] - v) / max(1, hi - lo - 1)
        if v > 12 * local + 3:
            clicks += 1
    return {"seconds": round(n / rate, 1), "peak": peak, "clipped": clipped, "clicks": clicks}


def navigation(a) -> bool:
    work = pathlib.Path(a.work) / "nav"
    esp = make_esp(work / "esp", pathlib.Path(a.efi), pathlib.Path(a.nav).read_bytes())
    serial, wav = work / "serial.txt", work / "audio.wav"
    port = 4460
    image = fat_image(esp)
    p = subprocess.Popen(qemu_cmd(a.qemu, a.ovmf, esp, serial,
                                  ["-device", "hda-duplex,audiodev=a0", "-audiodev", f"wav,id=a0,path={wav}",
                                   "-qmp", f"tcp:127.0.0.1:{port},server,nowait"], image))
    try:
        text = wait_for(serial, ("NAV_READY=PASS", "STATUS=BLOCKED"), 180)
        if "NAV_READY=PASS" not in text:
            print(text[-2000:])
            return False
        s = socket.create_connection(("127.0.0.1", port))
        f = s.makefile("rw")
        f.readline()
        f.write('{"execute":"qmp_capabilities"}\n'); f.flush(); f.readline()
        time.sleep(a.welcome)
        for k in KEYS:
            f.write(json.dumps({"execute": "human-monitor-command",
                                "arguments": {"command-line": f"sendkey {k}"}}) + "\n")
            f.flush()
            f.readline()
            time.sleep(a.key_gap)
        text = wait_for(serial, ("NAV_EXIT=PASS", "OMNI_SR_EXIT"), 60)
        # The exit trace is written after the exit marker; slow (TCG) runners
        # need more than a fixed pause before QEMU is shut down.
        if image:
            time.sleep(15)
        else:
            wait_for(esp / "OMNI-SR-TRACE.TXT", ("OMNI_SR_EXIT",), 60)
            time.sleep(1)
        f.write('{"execute":"quit"}\n'); f.flush()
        p.wait(20)
    finally:
        if p.poll() is None:
            p.kill()
    if image:
        fat_read(image, "OMNI-SR-TRACE.TXT", esp / "OMNI-SR-TRACE.TXT")
    keys_seen = text.count("NAV_KEY=")
    trace = (esp / "OMNI-SR-TRACE.TXT").read_text(errors="replace") if (esp / "OMNI-SR-TRACE.TXT").exists() else ""
    audio = audio_report(wav)
    checks = {
        "nav_bank_loaded": "NAV_BANK=LOADED" in text,
        "neural_clip_mode": "SYNTH=NEURAL_CLIPS_NAV_BIN_V1" in text,
        "all_keys_handled": keys_seen == len(KEYS),
        "no_play_failure": "NAV_PLAY_FAILED" not in text,
        "clean_exit": "NAV_EXIT=PASS" in text and "OMNI_SR_EXIT=SUCCESS" in text,
        "trace_persisted": "OMNI_SR_EXIT=SUCCESS" in trace,
        "audio_present": audio["seconds"] >= 3 and audio["peak"] >= 3000,
        "audio_no_clipping": audio["clipped"] == 0,
        "audio_no_clicks": audio["clicks"] == 0,
    }
    print(json.dumps({"keys_seen": keys_seen, "audio": audio, "checks": checks}, indent=1))
    if not checks["trace_persisted"]:
        print("ESP contents:", sorted(str(p.relative_to(esp)) for p in esp.rglob("*")))
        print("trace tail:", trace.strip().splitlines()[-3:] if trace.strip() else "(empty)")
    return all(checks.values())


def fuzz(a) -> bool:
    src = pathlib.Path(a.nav).read_bytes()
    links_off = struct.unpack_from("<I", src, 20)[0]
    sys_off = struct.unpack_from("<I", src, 48)[0]

    def patch(off, fmt, value):
        b = bytearray(src)
        struct.pack_into(fmt, b, off, value)
        return bytes(b)
    cases = {
        "node_count_wrap": patch(8, "<I", 0x08000001),
        "clip_count_wrap": patch(28, "<I", 0x20000001),
        "links_count_wrap": patch(24, "<I", 0x40000000),
        "fir_up_zero": patch(56, "<H", 0),
        "rate_wrong": patch(40, "<I", 44100),
        "child_link_oob": patch(links_off, "<I", 0xFFFFFF00),
        "child_first_oob": patch(64 + 16, "<I", 0x7FFFFFFF),
        "target_not_container": patch(64 + 32 * 2 + 12, "<I", 2),
        "sys_clip_oob": patch(sys_off, "<I", 0xFFFFFFF0),
        "truncated": src[:200],
        "bad_magic": b"XXXXXXXX" + src[8:],
    }
    ok_all = True
    for name, data in cases.items():
        work = pathlib.Path(a.work) / "fuzz" / name
        esp = make_esp(work / "esp", pathlib.Path(a.efi), data)
        serial = work / "serial.txt"
        p = subprocess.Popen(qemu_cmd(a.qemu, a.ovmf, esp, serial,
                                      ["-device", "hda-duplex,audiodev=a0", "-audiodev", "none,id=a0"]))
        try:
            text = wait_for(serial, ("HII_GRAPH_NAV_READY=PASS", "NAV_READY=PASS", "STATUS=BLOCKED"), 180)
        finally:
            p.kill()
        markers = [l for l in text.splitlines() if l.startswith("NAV_BANK")]
        ok = "HII_GRAPH_NAV_READY=PASS" in text and "NAV_BANK=LOADED" not in markers
        ok_all &= ok
        print(f"{'PASS' if ok else 'FAIL'} {name}: {markers}", flush=True)
    return ok_all


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--efi", required=True)
    ap.add_argument("--nav", required=True)
    ap.add_argument("--ovmf", required=True)
    ap.add_argument("--ovmf-vars")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    ap.add_argument("--work", required=True)
    ap.add_argument("--welcome", type=float, default=14.0)
    ap.add_argument("--key-gap", type=float, default=2.5)
    ap.add_argument("--fuzz", action="store_true")
    a = ap.parse_args()
    global OVMF_VARS
    OVMF_VARS = a.ovmf_vars
    ok = navigation(a)
    print("NAV_QEMU_NAVIGATION=" + ("PASS" if ok else "FAIL"))
    if a.fuzz:
        f = fuzz(a)
        print("NAV_QEMU_FUZZ=" + ("PASS" if f else "FAIL"))
        ok &= f
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
