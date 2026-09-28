from __future__ import annotations
import pathlib, re, sys
if len(sys.argv) < 2:
    raise SystemExit("usage: coverage_gate.py <llvm-cov-report>")
report = pathlib.Path(sys.argv[1])
if not report.is_file() or report.stat().st_size == 0:
    raise SystemExit(f"MISSING: coverage report absent or empty: {report}")
text = report.read_text(encoding="utf-8")
line = next((x for x in text.splitlines() if x.strip().startswith("TOTAL")), "")
numbers = re.findall(r"(\d+(?:\.\d+)?)%", line)
if len(numbers) < 2:
    raise SystemExit(f"cannot parse llvm-cov TOTAL line: {line!r}")
rates = [float(x) for x in numbers]
print(f"coverage percentages={rates}; minimum={min(rates)}")
if min(rates) < 100.0:
    raise SystemExit("critical native coverage is below 100%")
