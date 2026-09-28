"""Affiche un resume synthetique des metriques pour la conclusion."""
import json, statistics
from pathlib import Path

m = json.loads(Path(r"C:\st\benchmark\metrics.json").read_text(encoding="utf-8"))
by_eng = {"ST v3": [], "SAPI 5": []}
for x in m:
    if x.get("error"):
        continue
    by_eng[x["engine"]].append(x)

def stats(items, key):
    vals = [x[key] for x in items if x.get(key) is not None and x[key] != 0]
    if not vals:
        return (0, 0, 0)
    return (min(vals), statistics.mean(vals), max(vals))

for engine, items in by_eng.items():
    print(f"\n=== {engine} (n={len(items)}) ===")
    for k in ["duration_s", "rms_dbfs", "peak_dbfs", "f0_mean_hz", "f0_std_hz",
              "hnr_db", "spec_centroid_hz", "spec_rolloff85_hz",
              "spec_bandwidth_hz", "voiced_ratio", "file_kb"]:
        lo, mean, hi = stats(items, k)
        print(f"  {k:24s}: min={lo:7.2f}  mean={mean:7.2f}  max={hi:7.2f}")
    if engine == "ST v3":
        lo, mean, hi = stats(items, "synth_ms")
        print(f"  {'synth_ms':24s}: min={lo:7.2f}  mean={mean:7.2f}  max={hi:7.2f}")

st, sapi = by_eng["ST v3"], by_eng["SAPI 5"]
print("\n=== VERDICT (moyennes) ===")
for key, label, lower_better in [
    ("synth_ms", "Temps de synthese (ms)", True),
    ("file_kb", "Taille WAV (ko)", True),
    ("spec_centroid_hz", "Brillance (Hz)", None),
    ("hnr_db", "Harmonicite HNR (dB)", False),
    ("f0_std_hz", "Jitter F0 (Hz)", None),
    ("voiced_ratio", "Ratio voix activee", False),
    ("dyn_range_db", "Dynamique (dB)", False),
    ("rms_dbfs", "Energie RMS (dBFS)", False),
]:
    s_vals = [x[key] for x in st if x.get(key) not in (None, 0)]
    sa_vals = [x[key] for x in sapi if x.get(key) not in (None, 0)]
    if not s_vals or not sa_vals:
        print(f"  {label:30s}: skip (data missing)")
        continue
    sm = statistics.mean(s_vals)
    sam = statistics.mean(sa_vals)
    if lower_better is True:
        winner = "ST" if sm < sam else "SAPI"
        diff = (sam - sm) / max(abs(sam), 1e-9) * 100
    elif lower_better is False:
        winner = "ST" if sm > sam else "SAPI"
        diff = (sm - sam) / max(abs(sam), 1e-9) * 100
    else:
        diff = abs(sm - sam) / max(abs((sm + sam) / 2), 1e-9) * 100
        winner = "egal"
    print(f"  {label:30s}: ST={sm:8.3f} | SAPI={sam:8.3f}  delta={diff:+5.1f}%  -> {winner}")