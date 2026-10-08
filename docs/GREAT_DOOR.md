# Tutorial Great Door

The final King's Pass door uses a PlayMaker `Great Door` controller rather
than the ordinary `Breakable` component. Its collider was previously cooked
as permanent terrain and its tk2d MeshRenderer was omitted. The Town gate
behind it could consequently not be reached through normal play.

The Windows source controller accepts nail damage, increments its hit counter
once per accepted hit and uses these stages:

| Hits | Source behavior |
| --- | --- |
| 0 | `door_v01` intact sprite |
| 4 | `door_v02` damaged sprite |
| 8 | `door_v03` damaged sprite |
| 13 | Mark activated and begin transition to Town, entry `left1` |

Ordinary and intermediate-stage hits have a 0.15 second wait (nine 60 Hz ticks).
The wait exceeds the nail's active window, preventing multiple counts from one
swing. An activated door disables its collider and renderer on source reentry.
The implementation keeps the activated state for this session, including scene
return; Select resets it. Memory-card persistence remains unsupported.

`host/great_door.py` recognizes and validates this specific source contract,
then appends all three original stage sprites to the existing scene animation
bank. Each frame retains its own original bounds and pivot; the source poses
have different extents. Textures are capped at 128 texels on the long axis
(`DOOR_MAX_AXIS`; the poses project to about 117x216 and 142x209 screen
pixels) with 4bpp conversion, and each pose is tiled over a 2x2 rectangle of
64x64 animation slots the way NPC art is. Four of the frame's animation keys
are used while the door is visible, with no additional VRAM rectangle or
per-stage CD read.
Per-region collider indices are derived from the original collider source ID.

The runtime state occupies at most eight bytes. Collider removal shares the
existing bounded scripted-exclusion scratch with Lifeblood. The normal source
gate supplies the Town spawn and the existing ambience mixer handles scene
changes.

The source sequences two NextFrame actions, immediate fade/audio events and
a scene-transition request with a 2.5 second entry delay. This implementation
uses a 152-tick input lock before the existing scene transition. Hiding the
door/removing its collider immediately and moving the delay before the warp
are explicit choreography approximations. Camera shake, original Great Door
hit/break audio, stage debris and the original fade sequence are not yet
implemented. This does not claim full source effects or transition parity.

The standalone native contract test checks all thirteen hits, all three
stages, cooldown rejection, one-shot transition delivery, scene filtering,
visibility and session reset. The final September 14 actual-CUE replay also passes:
`.hkpsx/progression-scalar-town/` contains all 10,271 controller samples, all
thirteen hit events, the two stage changes and a transition 152 ticks after
opening. It ends in Town region 93 at `(2.5,44.390625)` with zero runtime/CD/
input/geometry faults and unchanged input hashes. Software stage/entry captures
and the final hardware-renderer capture were inspected. Surface ambience settles
to mask 54, with the shared loops retained. This does not validate all of Town.

Reproduce using a fresh ignored output directory:

```sh
python3 tools/replay_cue.py --tape .hkpsx/town-validation-search/attempt3.pxtape --output .hkpsx/town-recheck --screenshot-interval 30
```

Inspect `HK_GREAT_DOOR_*` and `HK_AMBIENCE_*` watches in `command.json` and
`route.csv`; `great-door-coverage.json` in the validated run records thresholds,
delay, entry, ambience state and hashes. The route's 23 particle rejections
come from its overlapping ordinary breakable bursts; destruction effects remain
capacity-limited.
