from __future__ import annotations
import hashlib, os, pathlib, subprocess, tempfile
ROOT = pathlib.Path(__file__).resolve().parents[1]
ENV = os.environ.copy()
ENV["SOURCE_DATE_EPOCH"] = ENV.get("SOURCE_DATE_EPOCH", "1790164716")
ENV["PYTHONHASHSEED"] = "0"
def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest().upper()
def build_once(dest: pathlib.Path) -> pathlib.Path:
    subprocess.run([os.sys.executable, "-m", "build", "--no-isolation", "--wheel", "--outdir", str(dest)], cwd=ROOT, env=ENV, check=True)
    wheels = list(dest.glob("*.whl"))
    if len(wheels) != 1:
        raise RuntimeError(f"expected one wheel, got {wheels}")
    return wheels[0]
with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
    wa = build_once(pathlib.Path(a)); wb = build_once(pathlib.Path(b))
    ha, hb = digest(wa), digest(wb)
    print("build A:", ha); print("build B:", hb)
    if ha != hb:
        raise SystemExit("reproducibility gate FAIL: wheel hashes differ")
    print("reproducibility gate PASS")
