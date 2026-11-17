#!/bin/sh
# Collect game/hk.layout, the I-cache layout profile of the hot text (docs/HOT-TEXT.md).
#
#   tools/hot-layout.sh DISC_DIR [OUT]
#
# DISC_DIR holds a route-input test disc of this checkout and its build outputs, as
# `HK_GUEST_FEATURES=route-input cargo hk-build build --no-validate --library DISC_DIR` leaves them:
# `Hollow Knight (hk-psx).cue`, and this checkout's dist/hk-psx.exe and build/hk-psx-normal.map are that
# disc's image and link map. HK_PSX_FRONTEND is the PSoXide frontend (as for every route replay). Each of
# the frame-table routes is replayed twice from its tape and card: once for its route log, which says where
# the gameplay starts (the first flip after the save's load, a gap of more than 30 ticks between flips),
# then with a count for every instruction word from there to the tape's end. `psoxide-pgo layout` folds the
# counts into the layout profile (OUT, default game/hk.layout); it refuses one in which a function that ran
# 0.5% or more of the instructions lies outside the text the layout places, and names it.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
disc=$(cd "$1" && pwd)
out=${2:-$root/game/hk.layout}
frontend=${HK_PSX_FRONTEND:?set HK_PSX_FRONTEND to the PSoXide frontend}
work=$(mktemp -d "${TMPDIR:-/tmp}/hk-layout.XXXXXX"); mkdir "$work/words"
trap 'rm -rf "$work"' EXIT
routes="c37-stand kings-death kings-return baldur-spell cornifer-map greenpath-walk secret-c10 secret-c37 cocoon-open secret-f08a mawlek-fight gruz-fight"
for route in $routes; do
  tape=$root/tools/tapes/$route.pxtape
  card=$(sed -n 's/^card *= *//p' "$root/tools/tapes/$route.pins")
  # The tape's header: polls (count) and the first poll it drives (start), little-endian words at 8 and 12.
  set -- $(od -An -tu4 -j8 -N8 "$tape")
  stop=$(($1 + $2))
  replay() {
    mkdir -p "$work/emu"
    cp "$root/tools/cards/$card" "$work/card.mcd"
    "$frontend" launch --path "$disc/Hollow Knight (hk-psx).cue" --embedded-playtest --config-dir "$work/emu" \
      --steps 400000000000 --input-tape "$tape" --stop-at-poll "$stop" --memcard "$work/card.mcd" "$@" \
      > "$work/replay.log" 2>&1
  }
  replay --route-log "$work/route.csv"
  start=$(awk -F, 'NR>1 && $11=="1" { if (p != "" && $1 - p > 30) { print $1; exit } p = $1 }' "$work/route.csv")
  [ -n "$start" ] || { echo "$route: no load gap in the route log" >&2; exit 1; }
  replay --route-log "$work/route.csv" --pc-log-words --pc-line-log "$work/words/$route.csv" --pc-line-start-route-tick "$start"
  echo "$route: gameplay from route tick $start, $(wc -l < "$work/words/$route.csv") words"
done
cargo run -q --release --locked --manifest-path "$root/.psoxide/Cargo.toml" -p psoxide-pgo -- \
  layout "$root/build/hk-psx-normal.map" "$root/dist/hk-psx.exe" "$work"/words/*.csv "$out"
