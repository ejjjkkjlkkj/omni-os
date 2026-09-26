"""Build a blind listening test: same sentences, equal loudness, shuffled labels.

python scripts/perceptual_pack.py [--out release/perceptual-validation]
Needs: built st.exe, ST_PYTHON/ST_NEURAL_HOME (or a release layout) and SAPI voices
Hortense (FR) / Zira (EN) for the system reference. Output:
  samples/<id>_<A|B|C>.wav   48 kHz 16-bit mono, RMS-matched to -24 dBFS
  mapping.json               label -> system (keep away from listeners)
  scores.csv                 one row per sample for the listener to fill
  README.md                  instructions and questionnaire
"""
import argparse, csv, json, random, subprocess, sys
from pathlib import Path
import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

ROOT = Path(__file__).resolve().parents[1]
SENTENCES = [
    ('fr01', 'fr', 'Bonjour, bienvenue dans ST.'),
    ('fr02', 'fr', 'Enregistrer, bouton, indisponible.'),
    ('fr03', 'fr', 'Démarrage sécurisé, case à cocher, cochée.'),
    ('fr04', 'fr', 'Le fichier pèse 12,5 mégaoctets et contient 342 éléments.'),
    ('fr05', 'fr', 'Voulez-vous enregistrer les modifications apportées au document ?'),
    ('fr06', 'fr', r'Ouvrir C:\Users\adm\Documents\rapport.docx.'),
    ('en01', 'en', 'Hello, welcome to ST.'),
    ('en02', 'en', 'Save, button, unavailable.'),
    ('en03', 'en', 'Do you want to save the changes you made to this document?'),
    ('en04', 'en', 'The update was installed on 2026-09-27 at 14:05.'),
]
SYSTEMS = {
    'st-neural': lambda lang: ['--voice', 'ff_siwis' if lang == 'fr' else 'af_heart'],
    'st-compact': lambda lang: ['--voice', 'female'],
    'sapi': None,
}
TARGET_DBFS = -24.0


def render(system, lang, text, out, st):
    if system == 'sapi':
        voice = 'Hortense' if lang == 'fr' else 'Zira'
        subprocess.run(['powershell.exe', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', str(ROOT / 'benchmark' / 'sapi_gen.ps1'),
                        '-VoiceName', voice, '-Text', text, '-OutFile', str(out), '-SampleRate', '24000'], check=True, capture_output=True)
    else:
        subprocess.run([str(st), '--lang', lang, *SYSTEMS[system](lang), '--text', text, '--out', str(out)], check=True, capture_output=True)


def level(path):
    x, sr = sf.read(path, always_2d=True)
    x = x.mean(axis=1)
    if sr != 48000:
        from math import gcd
        g = gcd(sr, 48000)
        x = resample_poly(x, 48000 // g, sr // g)
    frames = np.lib.stride_tricks.sliding_window_view(x, 960)[::480]
    rms = np.sqrt(np.mean(frames ** 2, axis=1))
    active = rms > rms.max() * 10 ** (-40 / 20)  # speech frames only
    gain = 10 ** (TARGET_DBFS / 20) / np.sqrt(np.mean(rms[active] ** 2))
    y = x * gain
    peak = np.max(np.abs(y))
    if peak > 0.98:
        y *= 0.98 / peak
    sf.write(path, y, 48000, subtype='PCM_16')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', type=Path, default=ROOT / 'release' / 'perceptual-validation')
    ap.add_argument('--st', type=Path, default=ROOT / 'target' / 'release' / 'st.exe')
    ap.add_argument('--seed', type=int, default=20260927)
    a = ap.parse_args()
    samples = a.out / 'samples'
    samples.mkdir(parents=True, exist_ok=True)
    rng = random.Random(a.seed)
    mapping, rows = {}, []
    for sid, lang, text in SENTENCES:
        systems = list(SYSTEMS)
        rng.shuffle(systems)
        for label, system in zip('ABC', systems):
            out = samples / f'{sid}_{label}.wav'
            render(system, lang, text, out, a.st)
            level(out)
            mapping[out.name] = system
            rows.append(dict(sample=out.name, sentence=sid, lang=lang, text=text, intelligibility_1_5='',
                             naturalness_1_5='', listening_comfort_1_5='', errors_heard='', comment=''))
        print(sid, 'done', flush=True)
    (a.out / 'mapping.json').write_text(json.dumps(dict(seed=a.seed, target_dbfs=TARGET_DBFS, mapping=mapping), indent=1), encoding='utf-8')
    with (a.out / 'scores.csv').open('w', newline='', encoding='utf-8-sig') as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]), delimiter=';')
        w.writeheader()
        w.writerows(rows)
    (a.out / 'README.md').write_text(README, encoding='utf-8')
    print(len(rows), 'samples ->', a.out)


README = """# Test d'écoute ST — à l'aveugle

Chaque phrase est rendue par trois systèmes, étiquetés A, B et C dans un ordre
tiré au hasard **différent pour chaque phrase**. Le volume est égalisé (-24 dBFS
RMS sur la parole). `mapping.json` révèle les systèmes : ne pas l'ouvrir avant
d'avoir fini, et ne pas le donner aux auditeurs.

## Consignes

1. Casque ou haut-parleurs habituels, volume confortable, fixé pour toute la séance.
2. Pour chaque phrase, écouter A, B et C (plusieurs fois si besoin), puis remplir
   `scores.csv` (séparateur `;`) :
   - **intelligibility_1_5** : 1 = incompréhensible, 5 = tout est compris du premier coup ;
   - **naturalness_1_5** : 1 = très robotique, 5 = comme une personne ;
   - **listening_comfort_1_5** : pourriez-vous l'écouter des heures au quotidien ? 1 = non, 5 = oui ;
   - **errors_heard** : mots mal prononcés, coupures, bruits, mauvaise intonation ;
   - **comment** : libre.
3. Questions finales (à ajouter en fin de fichier ou par écrit) :
   - Quel système utiliseriez-vous comme voix de lecteur d'écran, et pourquoi ?
   - À vitesse rapide, lequel resterait compréhensible le plus longtemps ?
   - Un défaut vous empêcherait-il d'utiliser l'un d'eux ?

Les systèmes comparés sont listés dans `mapping.json`. La référence « système »
est une voix SAPI Windows installée localement ; aucun enregistrement JAWS/Vocalizer
n'est inclus.
"""

if __name__ == '__main__':
    main()
