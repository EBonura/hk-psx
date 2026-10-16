#!/bin/sh
# Fail when guest source names a 64/128-bit integer or a float type (the rules and the allow marker
# are in tools/no64_guard.rs). There is no exempt file: wide arithmetic is psx_math::Wide, enforced in
# the SDK. Scans the guest crate and the shared crates it links. Run from anywhere. The link-map
# symbol gate (the SDK's tools/guest_symbol_gate.sh, run by the build) is the other half: it proves
# no 64-bit divide or soft-float routine is linked.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
out="${TMPDIR:-/tmp}/no64_guard.$$"
trap 'rm -f "$out"' EXIT
rustc -O --edition 2021 "$root/tools/no64_guard.rs" -o "$out"
"$out" "$root/game/src" "$root/shared/hk-sim/src" "$root/shared/hk-cache/src" "$root/shared/hk-format/src"
