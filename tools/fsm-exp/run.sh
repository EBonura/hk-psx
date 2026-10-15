#!/bin/sh
# Build fsm-exp like an SDK example, harden it, size it, run it headless.
#   V3=/fsm3-sizes/output W=/scratch/dir HAZARD=/path/to/hazard-bin-dir FRONTEND=/path/to/frontend sh run.sh
# Needs the hydrated .psoxide of the checkout. Nothing is written outside $W.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
: "${W:?set W to a scratch directory}"
: "${HAZARD:?set HAZARD to the directory holding hazard-patch and hazard-scan}"
: "${V3:?set V3 to the fsm3-sizes output directory (hornet1.hkf3, arity.rs)}"
export FSM_EXP_BANK="$V3/hornet1.hkf3" FSM_EXP_ARITY="$V3/arity.rs"
SDK="$ROOT/.psoxide/sdk"
TARGET=mipsel-sony-psx
FLAGS='"-Cllvm-args=-disable-mips-df-succbb-search=false","-Cllvm-args=-disable-mips-df-forward-search=false"'
mkdir -p "$W"
cd "$HERE"
CARGO_TARGET_DIR="$W/target" cargo build --release --target $TARGET -Zbuild-std=core -Zbuild-std-features=compiler-builtins-mem \
  --config "target.$TARGET.rustflags=[$FLAGS,\"-Clink-arg=-T$SDK/psoxide.ld\",\"-Clink-arg=--oformat=binary\",\"-Clink-arg=-Map=$W/fsm-exp.map\"]"
EXE="$W/target/$TARGET/release/fsm-exp.exe"
cp "$EXE" "$W/fsm-exp.unpatched.exe"
echo "== hazard-scan before the patch"
"$HAZARD/hazard-scan" "$EXE" --map "$W/fsm-exp.map" || true
echo "== hazard-patch"
"$HAZARD/hazard-patch" "$EXE" --map "$W/fsm-exp.map"
echo "== hazard-scan after the patch"
"$HAZARD/hazard-scan" "$EXE" --map "$W/fsm-exp.map"
cp "$EXE" "$W/fsm-exp.exe"
if [ -n "$FRONTEND" ]; then
  echo "== run"
  "$FRONTEND" launch --path "$W/fsm-exp.exe" --steps 400000000 2>&1 | grep -E "FSMEXP|^E[12]|^   "
fi
