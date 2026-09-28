#!/usr/bin/env bash
set -euo pipefail
rm -f default.profraw coverage.profdata protocol_cov
clang -std=c11 -Wall -Wextra -Wpedantic -Werror   -fprofile-instr-generate -fcoverage-mapping   -Iinclude c/omni_protocol.c tests/c/test_protocol.c -o protocol_cov
LLVM_PROFILE_FILE=default.profraw ./protocol_cov
llvm-profdata merge -sparse default.profraw -o coverage.profdata
llvm-cov report ./protocol_cov -instr-profile=coverage.profdata   -ignore-filename-regex='tests/' > coverage.txt
cat coverage.txt
python3 scripts/coverage_gate.py coverage.txt
