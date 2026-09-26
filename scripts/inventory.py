"""Read-only inventory of ST sources, experiments and audio; hashes every reference."""
import hashlib, json, subprocess, sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
out = root / 'work/voice-quality-benchmark'
out.mkdir(parents=True, exist_ok=True)
rows = []
for p in sorted(root.rglob('*')):
    if not p.is_file() or any(x in p.parts for x in ('.git', 'target', 'nextgen-venv')):
        continue
    if p.suffix.lower() not in ('.wav', '.rs', '.toml', '.ps1', '.py', '.json', '.md', '.txt'):
        continue
    if out in p.parents:
        continue
    row = dict(path=str(p.relative_to(root)), bytes=p.stat().st_size,
               sha256=hashlib.file_digest(p.open('rb'), 'sha256').hexdigest())
    if p.suffix.lower() == '.wav':
        import soundfile as sf
        try:
            info = sf.info(p)
            row.update(rate=info.samplerate, channels=info.channels,
                       subtype=info.subtype, frames=info.frames, seconds=info.duration)
        except Exception as e:
            row['error'] = str(e)
    rows.append(row)
(out / 'inventory.json').write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding='utf-8')
print(f'{len(rows)} files, {sum(r["path"].lower().endswith(".wav") for r in rows)} WAVs inventoried')
