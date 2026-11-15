#!/bin/sh
# Fail when guest source names a 64/128-bit integer or a float type: the SDK's `psoxide-dev no64-scan`
# over the guest crate and the shared crates it links. The one exempt file is the reference arithmetic
# of a test-only module (combat.rs includes it under cfg(test)), which is wide on purpose. Wide
# arithmetic in guest code is psx_math::Wide, enforced in the SDK. Run from anywhere. The link-map
# symbol gate (the SDK's tools/guest_symbol_gate.sh, run by the build) is the other half: it proves
# no 64-bit divide or soft-float routine is linked.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
cargo run -q --release --manifest-path .psoxide/tools/psoxide-dev/Cargo.toml \
    --target-dir "${PSOXIDE_DEV_TARGET:-$root/build/psoxide-dev-target}" -- \
    no64-scan --exempt shared/hk-sim/src/combat_intersection_tests.rs \
    game/src shared/hk-sim/src shared/hk-cache/src shared/hk-format/src
