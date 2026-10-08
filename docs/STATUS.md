## Shared-runtime harmonisation, 2026-09-21

The runtime uses SDK integer roots, aligned texture uploads and SPU IRQ control while
preserving the source simulation, room residency and input-observation policies.
Existing build 107 assets and the last library disc remain the controls. Host and
headless route results are recorded in the harmonisation audit; no new hardware
validation or gameplay completion is implied by this maintenance change.

## Build 107: the whole Forgotten Crossroads on the actual disc

The sole disc pair is build 107 (45 scenes, 693 views; death costs the Geo a
Hollow Shade then carries, and caps SOUL until it is killed; a bench rest saves
to the port-1 memory card and the title offers Continue; five benches rest, heal and set
the respawn marker; build 104 made every gate enter through the source
fade/lead/forced-walk or hidden-drop sequence; build 103 applied the source gate
facing and delayed top-collider rules; build 102 added source camera locks and a damped camera; build 101 folded the actor runtimes into one enum;
build 100 admitted the Aspid Hunters with
the first projectile pool (docs/ASPID.md); build 99 admitted the Leapers through a Leap
attack on the Runner walker; build 98 admitted Husk Bullies, Hornheads and
the other-scene Runners through the parameterized Runner; build 97 admitted
the Gruzzers (19, docs/GRUZZER.md) and Baldurs (11, docs/BALDUR.md); build 95 admitted the Vengeflies (18 of 20
Buzzers, docs/VENGEFLY.md) with actor simulation bounded to one view around the
active view; build 94 admitted the Crossroads Climbers;
earlier build 89 admitted the Crossroads Zombie Runners with their
audio bank, break-effect art streamed per scene from its own disc chunk, 210
emitters admitted across 18 scenes; earlier builds (the static region table is gone: every
region's metadata is read from the loadable bank through a runtime `Region`
value, guest state tables sized from the
catalog, a seventh route through Crossroads_02 and back, 271,340 bytes of
linked headroom). Build 76 was: King's Pass, Dirtmouth and seventeen Forgotten
Crossroads scenes (329 views, 200 disc chunks) reachable through the authored
gates, six route replays passing with 219,292 bytes of linked headroom. Growing
the catalog now reuses cooked regions and derives the scene arena from the
pack; break-effect art past scene 4 and oversized Geo art are explicit
omissions until effect art streams per scene. Build 74 and it is validated by the root Cargo driver:
`cargo hk-build build` cooks with caches, builds the guest, replaces
`~/Downloads/ps1 games/hk-psx.bin/.cue`, writes the build report and replays
the route tapes in `tools/tapes`, failing on any fault. Forgotten
Crossroads (Crossroads_01) is scene 2: the authored Dirtmouth well gate drops
the Knight into it and the `town-well` route ends inside Crossroads region 110
after three scene loads. Build 72 was the first disc run of the metadata
relocation and exposed three guest faults (neighbour id base, arena-tail bank
overwritten by later in-place decodes, per-tick re-parsing overflowing the
input queue); all are fixed and the host gate-load proof now stages the bank
exactly as the guest does. Baked particle tracks were removed in favour of the
proven scalar path, freeing 449 KB: the link now has 401,256 bytes before the
stack floor with three scenes. Every cooker derives its scene list from
`host/quality.py SCENE_TABLE`. See `docs/plans/COMPLETE_GAME_PROGRESS.md`.

## Complete-game implementation resumed

P00-P04 are complete; P05-P08 and the first P06/P13 slices are in progress. The current isolated-worker
import verified all 501 BuildSettings scenes with zero failures: 463 have
complete source geometry and 38 retain only explicit runtime text-glyph work. It
preserves all 5,274 CircleCollider2D boundaries, 28,814 particle systems and
renderers, 831 trails, 12,745 canonical tk2d generated-mesh aliases and all 577
shipped Unity Plane/Quad instances. The remaining 761 cases are 757 TextMeshPro
and four legacy TextMesh generators, with zero extraction errors.

The final run used 504 isolated attempts. First attempts for `Fungus2_14`,
`Fungus2_15` and `Fungus2_34` ended on signal 11; fresh-process retries succeeded
and the final source/code verification passed. The 106-region Tutorial/Town
comparison now verifies pack provenance and exact scenery/tilemap geometry plus
terrain. It includes all 20 Town rows that lack `base_path`, resolving them by
chunk identity through `.hkpsx/regions-provenance.json`.

The P02 inventory adds 318 serialized PlayMaker scene-load actions to the 1,119
TransitionPoint records: 19 literal actions map to exact gates, 11 LoadLevel
actions map to scenes, and all 286 variable targets now carry serialized dataflow.
The split is 130 storage/constant reads, 89 defaults, three exact same-object
branch overrides and 64 runtime-injected targets assigned to P09/P22. All 501
scenes have an evidence-backed primary role and remain in scope; all
TransitionPoints have resolved trigger geometry and full initial flags.

The source catalog also inventories 17,419 PlayMaker PlayerData actions and 815
resolved keys. Installed completion CIL is hash-bound and decoded into 44
boolean/group rules, six integer rules, four nested Godhome rules, 40 charm
entries and the 33/66/99 SOUL-vessel cases. Installed initialization/override
CIL proves the item caps and exact maximum of 112. All 95 contributing fields
are joined to 33 direct managed user methods and their serialized PlayMaker
counts.

Progression evidence covers every load route. Of 318 scripted loads, 18 have a
same-state PlayerData condition and 176 have broader FSM-reachable candidates;
the other 124 are explicitly owned by the transition/progression runtime. The
catalog also records 204 TransitionPoints with same-object PlayerData evidence,
25 one-way serialized candidates and 15 reference cases. P02 is validated. No
FSM execution, guest admission or traversal parity is claimed. See
`docs/plans/COMPLETE_GAME_PROGRESS.md`.

The playable disc remains build 71 and has not been rebuilt by this host work.

P04 is also validated. The ignored stable-ID registry collision-checks all
1,997,203 components plus every scene, gate, PlayerData key and referenced asset;
no asset remains unresolved. IDs use source scene paths/PathIDs instead of build
order. Compatibility aliases preserve the current 37 Breakable and 39 grass
sparse IDs. The new no-std central state API separates global, mode and active
scene owners, makes duplicate rewards idempotent and passes all reset/reload and
exact-size tests. Its conservative current-source capacities total 93,912 bytes,
without multiplying active state by 501 scenes. See `docs/STATE_IDENTITY.md`.

The first P05 metadata slice is implemented as `HKWMTA01`. The host cooker emits
one checked bank per currently cooked source scene from `data/regions.json`:
Tutorial is 32,072 bytes for 86 regions and Town is 2,844 bytes for 20 regions.
The banks contain global region IDs, Q16 bounds/camera data, global neighbour
IDs, breakable/grass/hazard/checkpoint/actor records and polygon point pools.
`hk-format::WorldMeta` validates section strides, alignment, padding, spans and
geometry before exposing allocation-free borrowed views. The scene pack emits
its compressed admission copies under `.hkpsx/world-metadata-packed/` and
records metadata checksums, source fingerprints and post-coverage chunk IDs.
The guest admission transaction stages each bank in the unused aligned tail of
the admitted scene arena, then cross-checks it against scene geometry before
publishing a scene; this avoids a second static RAM arena. A pinned release
guest build now links with zero final hazards and 23,464 bytes before the stack
floor. The arena is now sized to the measured 354,032-byte scene-plus-metadata
peak, leaving 23,464 bytes before the stack floor in the current no-disc link.
Spatial actor and checkpoint lookup now resolves through the admitted bank while
retaining the existing static gameplay records. Build 71 and the sole Downloads
disc are unchanged while this new chunk layout is validated.

The guest edge cache now keys entries by world generation, generated global
region ID and the borrowed room view. Scene admission, retry, death and reset
paths revoke the key before arena reuse; the view key prevents equal-sized room
payloads from reusing one another's filtered edges, while the generation key
protects the reusable arena from stale addresses.

The admitted metadata path now differentially checks every Tutorial/Town bank
against the legacy region table before publication: bounds, collision bounds,
camera limits and ordered global neighbour IDs must match. The current pinned
no-disc release link remains hazard-free (`401,936` bytes of code and `23,464`
bytes before the stack floor). The loader's pack index and header are sized
from the generated manifests rather than an arbitrary 256-chunk ceiling; for
the current 38-chunk pack this removes 6 KiB of unused static header storage.
Targeted host tests and the complete
`shared/hk-format` suite pass; the build-71 EXE/BIN/CUE remains untouched.

The first P06 source-matrix slice is recorded in
`.hkpsx/world-import/pack-matrix.{json,md}`. It covers all 501 verified scenes,
joins each scene's geometry/component hashes with its 2,615-texture/21,741-
fragment dependency inventory, and computes current-plus-authored-neighbour
windows. Its measured largest window is `Hive_03_c` plus four neighbours at
52,062,693 extracted geometry bytes and 246 source texture fragments. These
are source evidence only: PS1 RAM/VRAM fields remain unset until real crop,
palette, compression and upload cooking proves them.

`tools/cook_scene_pack.py` now performs that cooking per scene in isolation. It
drives the canonical cooker, similarity pass and scene-bank packer for any
catalog scene under a write guard, derives the activation envelope from the
source tilemap, gate triggers and spawn points, and writes a measured summary
under the ignored `.hkpsx/scene-packs/`. Crossroads_01 cooks into 20 views,
103,960 resident bytes, 7 pages and 501 palettes with a passing native
scene-gate decoder and one explicit Secret Mask exception. The matrix merges
these summaries into per-scene PS1 costs and admission labels without claiming
playability.

The whole-catalog batch is complete: 457 of 501 scenes cook with a passing
native scene-gate decoder, 41 have no source envelope and three exceed the
128-view HKSCNE room limit (Abyss_21, White_Palace_13, White_Palace_20).
Eleven shared abort causes found by the batch are now explicit recorded
exceptions or lossless transformations, including exact tiling of hazard
polygons above 16 vertices and packet-bound guards on the similarity pass.
Isolated resident costs peak at 316,892 bytes for Tutorial_01 with 18 pages;
no scene alone exceeds the current scene arena, but two need 16 or more pages
and Cliffs_01 needs 1,084 palettes before actor banks or neighbour windows.
The regenerated `.hkpsx/world-import/pack-matrix.{json,md}` records per-scene
costs and failure causes. Build 71 remains the sole delivered disc.

Persistent Geo state no longer reserves a scene-multiplied `[scene][32]` table.
The guest now uses a flat 256-entry source-state pool keyed by scene and authored
rock slot. The verified source catalog contains 207 GeoRock instances, so the pool
has measured headroom while preserving depletion across scene revisits; overflow
remains an explicit deferred event. Native Geo runtime tests pass and the no-disc
link remains hazard-free. This removes another two-scene ceiling without changing
the build-71 disc.

The residency planner now emits version-2 directed gate records for all authored
edges in the 501-scene graph. Each record preserves gate/entry identity and
serialized conditions, orders current/target/target-neighbour fetch tiers, and
only publishes an eviction set after the target window is admitted. The refreshed
ignored plan contains all 501 windows and 850 directed transitions; it remains a
host scheduling input until guest background reads are bound.

The bounded room-residency primitive now has an `evict_unwanted` operation for
applying those keep-sets. It releases only completed/parked slots outside the
set; current, protected GPU uploads and in-flight CD leases remain pinned. The
room-residency and stored-payload suites pass with this guard.

The shared persistent state layer now has a versioned `HKSV` snapshot codec:
sorted stable IDs, signed values, exact length, and FNV checksum are validated
before a store is published. Truncation, version, corruption and capacity cases
are covered by native tests. This is the host/simulation foundation for P13
memory-card I/O; no card driver or playable-disc replacement is claimed yet.

The first P08 rendering-correctness slice is now implemented. Certified tile
ownership events are partitioned into independent back and front chains, and
the back pass retires only back ownership; front masks therefore remain valid
through the whole back-layer traversal and are retired in their own pass. This
fixes a source-index-dependent occlusion failure without disabling the
precomputed proof. A regression covers mixed same-index back/front ownership,
and the hk-format coverage suite exercises prefix resume, invisible draws and
both layer passes.

Current validation after these changes: 401 Python tests, 47 world-runtime
tests, the complete `shared/hk-format` suite (including 15 tile-coverage tests),
and the complete `hk-sim` suite pass. The no-disc guest rebuild links with zero
final hazards; code/data/BSS are 402,860/715,344/847,768 bytes and the
pre-stack margin is 16,232 bytes. These numbers describe the source build only;
build 71 remains the sole delivered disc.

## Completion plan and execution history

The comprehensive source-grounded plan is [COMPLETE_GAME_SOL.md](plans/COMPLETE_GAME_SOL.md),
with a short [execution entry point](plans/SOL_START_HERE.md). It includes 32 work
packages,20 first slices, full scope, explicit acceptance criteria, current
architecture blockers, commands and a copy/paste handoff prompt. This was a
planning-only task: no import was resumed, no guest code was changed, and the
playable disc remains71. The interrupted import status below still applies.

## Whole-world development pass — stopped at user request

Source changes are saved; the sole playable disc remains71 with identical EXE,
BIN and CUE hashes.344 Python tests passed. No guest build was performed.

New `host/world_import.py` and `host/world_geometry.py` separate all-scene source
extraction from gameplay admission. They preserve sprite/tk2d geometry, collider
paths, mesh data, cameras, gates and serialized components, with resumable
hash-bound checkpoints and explicit gaps. A searchable local report is generated
by `tools/world_import_report.py`. See [WORLD_IMPORT.md](WORLD_IMPORT.md).

The501-scene run is incomplete. At stop,249 scenes had exported records
(2 without reported geometry gaps,247 partial),196 further jobs were recorded
failed after a worker process terminated abruptly, and56 had no recorded result.
The cause of that worker termination has not been diagnosed; do not interpret
those196 pool failures as individual corrupt scenes. Final source/code verification
was not completed. The earlier dependency inventory's1,409 source hashes match.
Evidence: `.hkpsx/world-import/report.json`, `stopped.json`, `import.log`, and
`python-tests-final.log`. Completed artifacts are retained, not PS1-ready rooms.

On resumption, diagnose the worker failure and use `--jobs 1 --resume`; intact
matching checkpoints can be reused and failed jobs are retried. Complete final
verification before claiming a whole-world import. The attempted comparison
against all canonical region base files stopped on a missing `base_path` and
provides no equivalence proof. No additional playable areas were added.

# Status, 2026-09-15

## Current playtest71: actor integration and requested refresh

The user's latest request replaces the sole Downloads/ps1 games/hk-psx BIN/CUE
pair again. Build and final hazards pass (zero); the850-poll actual-CUE replay
completes without reported faults, and its inspected320x240 gameplay image is
byte-identical to70. Evidence: .hkpsx/playtest71/{build.log,focus/command.json}.
BIN SHA256:4fa805dfa4aeec272d743b8d2ce9721ded164971f43d5ff02ba99e7b02b52d1a.
Playable area coverage remains Tutorial and Town. Keep this disc available
for the user's playtest until they request another replacement.

Runner now has an explicit guest ActorController branch with shared cooked clips,
live terrain sensing, source-ordered damage/recoil, corpse and once-only Geo
ownership. Native tests check ordered source audio/dust events. Production
rejects unbound Runner presentation; canonical Runner admission remains closed
until sound/dust and Crossroads scene bindings are complete. Measured source
ground clearance fixes a false floor-as-wall query. Runner corpse bounce/offset/
timing are distinct from the unchanged Crawler path. Actual source art/remap
proofs pass. Prepared Runner audio uses20,464B at8kHz movement/11.025kHz calls,
leaving8,032B SPU space, but is not installed in the guest. See RUNNER.md.

All326 Python and222 hk-sim tests pass, including18 actual enemy-runtime tests.
Logs: .hkpsx/runner71/{python-final,native-final}.log. Current link code391,628B,
data720,432B, BSS862,100B;8,044B remain before the49,152B reserved stack. Main
frame33,728B leaves15,424B for nested calls; high-water is not established.
Additional controller code makes metadata/code capacity work necessary before
adding more linked scene tables. No new FPS or physical-hardware claim;
long gate/return routes were not rerun for this refresh.

## Previous playtest70: user-requested provisional rebuild

Rebuilt the current working source and replaced the sole Downloads/ps1 games
hk-psx.bin/.cue pair. Full asset recook and final guest build pass, with zero
R3000 hazards and34,668B before the reserved stack. The850-poll actual-CUE
smoke replay completes with no reported faults; its inspected320x240 gameplay
image is byte-identical to provisional69. Evidence: .hkpsx/playtest70/build.log
and focus/command.json. BIN SHA256:
1016708eb5482a17eeaf2d2ffc7efbe0b4fa19b55e3b84c681fa4614fa276b1c.
Exactly one Hollow Knight BIN/CUE pair remains in the library. Crossroads enemy
preparation is still source-only; this rebuild adds no new playable area.
Long traversal routes were not rerun for this rebuild. Leave this disc available
for the user's playtest until they request another replacement.

## In progress69: scene-owned visibility certificates

Moving unchanged tile/group visibility proofs from aggregate linked tables into
one84KiB scene-owned bank. Loading stages them through the empty geometry arena
before atlas/scene admission; checked source identity and bounded copy precede
readiness. Renderer views borrow that owner and retain only copied IDs/geometry.
The user-requested provisional69 now replaces the sole Downloads/ps1 games/hk-psx
BIN/CUE pair. Build succeeded with zero final hazards; 301 Python tests and
412 reported native tests passed. Linked free space before the reserved stack is
34,668B, a14,320B net recovery over68. The850-poll actual-CUE Focus smoke replay
completed without reported faults, loaded83,692B of Tutorial coverage, and its
inspected final320x240 image is byte-identical to68. Evidence: .hkpsx/game69/
(build-provisional.log, focus-provisional/); full tests are in
.hkpsx/scene-certificates69/tests-provisional.log. The12,112-poll Tutorial/Town/return replay also passes: three matching proof
and geometry admissions, exact321,956B geometry and83,692B proof after return,
569 loading ticks/samples versus508 in68, queue peak4 and no reported faults.
Its inspected final image is byte-identical to68. No static transfers occur
during11,382 checked same-scene gameplay intervals. The12,562-poll Town/reset route also passes, reachingx177.274 and returning
to the opening with an image byte-identical to68;11,832 stable intervals pass. A separate Town-ending replay verifies88,200B geometry and27,192B proof
against the exact cooked files in RAM, with a visible inspected Knight atx174.646.
Consolidated evidence: .hkpsx/game69/validation.json. Crossroads gameplay remains unimplemented.

## Source preparation70: Crossroads enemies

An isolated source-derived Zombie Runner controller is implemented in hk-sim.
See RUNNER.md for callback ordering, completion-token and integration contracts.
It is not connected to guest actors or included in the provisional disc.
The host now recognizes both source instances strictly and records their shared
34-sprite library, transformed sensing bounds and original audio references.
Rotation lock, terrain filters, gravity and scaled animation clock are checked;
unrecognized variants remain disabled. Pure bounded Q16 terrain queries implement
the three-ray Sweep, mirrored body bounds, AlertRange and terrain LOS, with
10 native tests. Source physics contact tolerances and guest bindings remain.

The isolated original run `runner-crossroads70` now passes revalidation with623
gameplay frames. Both Runners and both Climbers have578 complete samples; two
attack cycles expose synchronous restart/Ready behavior and player-hit slowdown
in the lunge clock. See tools/hkref/README.md for evidence and limitations.
The original audio validation failure was a frame/fixed-clock context switch;
raw logs and the failed report are retained alongside hash-bound revalidation.

A full isolated12-view Crossroads scenery/collision pack occupies98,808B RAM,
7 static pages and501 palettes and passes native incremental decoding. Pending
Runner/Climber/Mender animation libraries cost30,496B standalone plus2,272B CLUT;
this is not an integrated scene delta and excludes corpse, VFX and audio costs.
Evidence: .hkpsx/crossroads70/RESULT.md. Secret Mask scripting, well/camera flow,
guest actor/art/audio/corpse/Geo bindings and progression remain unfinished.

Runner now preserves synchronous StartMoving/Walker.Update, cooldown clearing
and immediate Ready sensing, with15 controller tests. Its native state is20B,
transient ordered actions260B. A separate Climber controller models source
attachment, ground-before-wall corner sensing, unsnapped inside-turn endpoints,
scaled-time freeze/ramp and timed stun/end-of-frame resumption. State56B,
transient actions64B;11 targeted tests pass. Original traces corroborate both
attachments, speed and10 outside turns, including one slowed by hit-stop.
Inside turns, stun and death still need live reference comparison. See CLIMBER.md
and .hkpsx/climber70/controller-trace-proof.json.

Validation:309 Python tests pass. The full hk-sim suite passed213 tests before
the final scaled-time Climber refinement; all11 Climber tests pass afterward.
Logs: .hkpsx/runner70/{python-tests-final,hk-sim-final}.log and
.hkpsx/climber70/controller-tests-final.log. No guest build or disc replacement
was performed for these source-only controller changes.

## Current playable build68: Town expansion and scene replacement

The previous final68 build, now superseded by provisional69, included
the return-entry correction after the earlier requested provisional.
106 spatial views cover Tutorial and Town0..270/-5..76 (86+20). Scene/atlas
ownership is exclusive: an authored gate retires GPU work, blacks the display
at VBlank, revokes old readiness and resets renderer/animation caches before
replacing the scene. All internal spatial views stay resident. Actual controller
observations and RAM-to-SPU audio continue during reads/decode/uploads; loading
does not become queued gameplay catch-up.

The source Great Door opening retains its13 hits and initial Town entry delay.
Normal travel in both directions now uses source horizontal entrance placement,
ground ray, facing, fade and28-tick walk without repeating that special delay.
An inspected provisional return capture exposed the old development spawn behind
the foreground. Tutorial/right1 now starts at(193.5,63.400625), walks left and
returns control visibly near(189.627,63.391). No arbitrary relocation was used.

All75 supported destruction emitters cook, including Town's four grave/crystal
emitters. Native original probes establish guarded planar/uniform-XY Hierarchy
scaling. Town emitters use the existing scalar path; Tutorial tracks stay399,432B.
An unused Geo strip was reassigned: Geo5,154/6,336B; break art6,330/7,680B.
Full occlusion certificate coverage grows to101,886/106,496B. Existing320x240
framing,48px scenery cap and approved95% sharing remain unchanged.

Three final-CUE routes pass: startup/Focus850 polls, gate return12,112, and Town
traversal/reset12,562. The last reachesx177.274 before resetting. Both long routes
load Tutorial->Town->Tutorial; final321,956B scene bytes match the cooked bank.
Gate waits total508 observed ticks and508 real pad samples; queue peak4/16.
No reported input, CD, guest, music or ambience underrun faults. Same-scene
walking adds no scene reads/static uploads. Captures of Town, returned Knight
and reset were inspected.23 particle-pool drops remain; gate blackouts are still
loading pauses. These are emulator results, not physical-console timing evidence.

Full suite:286 Python/406 reported native tests. Exact guest-decoder packing
passes six repeated scene replacements at budgets1,31,1024,2048,8192; final
MIPS hazards0. Main RAM: code355,652/data830,128/BSS776,068/alignment12, static
end0x801eef84.20,348B remain before the protected49,152B stack; main frame32,200B,
nested reserve16,952B. Arena369,180B; Tutorial321,956B/18pages/1,068palettes,
Town88,200B/6pages/390palettes. No new FPS or stack high-water claim.

EXE SHA256:9af52a2220bb26c011fc36c3c592f95e282e010f7fc41161aca36ba4962d410f.
BIN SHA256:40bd382393b59d394bc5f57bbad4ba568f44352916de9bdf4f3412feb97feee9.
Evidence: `.hkpsx/game68/{build-return.log,tests-return.log,validation.json}`,
`{focus-final,return-final,town-reset-final}/command.json` and scene validation
reports. `return-provisional` ended mid-load; `return-complete` exposed the old
spawn. Earlier build-final.log stopped before disc replacement on the old table
cap. These trials are not final-build validation. Native particle evidence:
`.hkpsx/town68/particle-validation.json`; source return and enemy audits:
`.hkpsx/crossroads68/`.

Next: move per-scene visibility certificates from linked tables into a reusable
CD-loaded bank before adding Crossroads. Exact sizing/API/validation proposal is
`.hkpsx/scene-certificates69/PLAN.md`; measured84KiB arena replaces110,066B of
linked tables, reclaiming roughly24KiB before loader cost without reducing art.
Then implement the well connection and source-checked Zombie Runner contract.
Town shops/NPCs/bench, well/Mines transitions and gameplay music are unfinished.
The full-game objective remains active and far from complete.

## Preparation67: full Town coverage and scene replacement

Fresh isolated Windows-source cooks cover Town0..270/-5..76, including central
street, well and eastern exit. The supported bank is88,980 resident bytes,
6 pages/408 palettes/426 textures, with684 pooled draws and221 edges. This is
41,764 more resident bytes than the entrance-only bank. Source records/texels
are verified during packing; source NPC/FSM/shop/bench behavior remains missing.
Evidence: `.hkpsx/town67/full-{bank.hk,bank.json,regions.json}`. These are host
assets only; the playable disc remains66 and still ends at Town x48.

A host-only scene-gate residency mode now permits overlapping scene allocations,
with explicit native repeated replacement proofs through the guest decoder.
The actual Tutorial/fullTown package passes six replacements at each of five
decoder budgets (1,31,1024,2048,8192). It fits the existing369,180-byte arena,
with321,956 peak resident bytes and18 pages/1,068 palettes at a time. Missing
atlases, misplaced uploads and undersized arenas are rejected. Reports/commands
are `.hkpsx/scene-gate67/{packed.json,packed.decoder.log,rejection.json}`.
`make test` passes275 Python/402 reported native tests. The production mode
remains joint residency; this is host evidence, not live CD/GPU/transition proof.

This prepares expansion beyond globally resident VRAM. Guest admission/blackout,
input/audio service during loading and cache invalidation must be integrated and
replayed before shipping it. No scene-gate package can compile against the current
joint-residency guest by accident. No new FPS or live-game coverage claim.

## Previous playable build:66 Focus audio and gameplay corrections

Build66 was the previous validated milestone, superseded by provisional68.
Focus now plays the complete source charging loop and healing sound, including
repeated heals and tails after charging stops. Sustained charging uses8,000Hz
mono; the short heal uses22,050Hz mono, matching the existing quality categories.
The108,208-byte bank loads once through startup scene scratch into SPU; Focus
adds no playback-time CD reads or persistent main-RAM sample bank. Two alternating
heal voices preserve overlapping tails. SFX volume applies to active voices.
Cancellation/damage stop charging; the existing Focus lifecycle controls fades.
Source FSMs, clip IDs and assembly hashes are checked by `host/focus_audio.py`.
Original mixer DSP and full pause-audio parity remain unverified.

The Great Door retains its13 authored hits. The instant blackout and two-frame
departure now precede the scene switch; the source2.5-second wait belongs to Town
entry. The destination uses the source gate-ground position, fade, footstep lead
and scripted28-tick walk with gravity disabled, avoiding the former arrival fall.
The PS1 subtractive fade approximates the original screen effect. Destruction
sound/alternate door-hit variants and complete original effects remain missing.

Crawlers and corpses now use collision data for their own resident region instead
of freezing at the Knight's collision-view boundary. A bounded128-edge scratch
cache avoids repeating edge remapping during every solver query; larger views
retain a direct fallback. Native tests compare all three crawler routes against
the full-scene edge union for1,200 ticks. Original recordings support continued
offscreen motion, but this is not exact Box2D, timing or RNG parity. Distant
script-controlled exclusions and flying enemies still need further work.

The render loop now consumes input for observed VBlanks before submitting another
expensive frame. An initial66 replay exposed queue overflow and was rejected;
the corrected final10762-poll traversal completes with queue peak4/16, no missed
polls, boundary waits, audio underruns or reported guest faults. It strikes the
door13 times, transitions once, finishes Town entry and ends grounded at
(4.373,44.391). It still records23 particle-pool drops; full effects parity is
not established. Town coverage still ends at x48: no additional maps were added.

Four final-CUE routes pass: Focus850 polls, isolated Focus audio900, stationary
crawler patrol1500, and Town traversal10762. Resident Focus bytes match the
cooked bank exactly. Captured charging phrases correlate above0.9997 with the
independently decoded source, and both heal tails above0.997 after alignment;
these are emulator capture checks, not original-mixer or hardware proof.
Town fade/entry and gameplay captures were inspected. `make test` passes272
Python tests and402 directly reported native tests; final MIPS hazards:0.

Final memory: code351,452B/data801,568B/BSS776,052B/alignment4B, static span
1,929,076B ending0x801e6f74. There are53,132B before the protected49,152-byte
stack. Main frame32,192B leaves16,960B for nested frames; the enemy helper's
final binary frame is2,336B. These are allocations, not measured stack high-water.
SPU bank0x5EA00..0x790B0 leaves28,496B contiguous free. Scene arena369,180B,
20 static pages/1,209 palettes,320x240 framing and existing textures are unchanged.
This milestone makes no new FPS claim. Build65's CD-DA title remains integrated.

EXE SHA-256: `a2da154b4e273a1578476584cc5636653ec85e8537f3faf38c91dff9a339a2ef`.
BIN SHA-256: `e70b76959353eb5153443a5de77dc93b1ad06cd5c2c035ad24238f26dbaa2aa2`.
CUE SHA-256: `7e0d1f91a3b1a1ae9db24e4650d45644db7087da2e3338cacbb2557390a78585`.
Evidence: `.hkpsx/game66/{build-fixed.log,fixed-tests.log,validation.json,
focus-wave-check.json,heal-wave-check.json}`, final replays in
`{focus-final,focus-isolated-final,patrol-final,town-final}/command.json`, source
provenance `.hkpsx/{focus-audio,great-door}.json`, and enemy audit
`.hkpsx/enemy-behavior-audit/`. The earlier `game66/town/` is a failed trial.
Replay commands/tape hashes are retained in each command.json; use
`python3 tools/replay_cue.py --tape TAPE --output OUTPUT` after completing the
canonical disc build. Next expand Town coverage and source-traced enemy/break/
death/UI cues, then unsupported enemies and progression systems.

## Previous build:65 CD-streamed title music

Build65 introduced the CD-streamed title. Title
plays its complete81.8-second source track directly from CD using the pinned
SDK's CD-DA starter/end detector. It is44,100Hz stereo PCM16, resampled from the
48kHz source, with no truncation or padding. Track02 shares the same BIN as data
track01 and has the SDK's150-sector pregap. No music sample bank occupies main
RAM, SPU RAM or a voice. There is an independent Music0..10 setting in title and
pause menus, including mute, and a fade before starting gameplay.

Music stops through completed SDK Pause before the first room/data command,
retaining drive spin. Source title delay is1s before the command handshake;
physical seek adds latency. End detection arms only after observed playback.
The final CUE completed10500 neutral input polls, two track repeats, and continued
playing with no reported faults. Captured stereo phrases match the cooked PCM
on all three plays (correlation above0.9999989 after gain/alignment). Measured
repeat gaps are0.500s and0.473s in PSoXide. This is not gapless playback or a
physical-console timing claim; original mixer DSP is also not reproduced.

Separate actual-CUE routes verify Music mute/restore, gameplay after active
music, and early Start before any Play command. Both gameplay captures were
inspected. No guest/audio/room faults or missed polls were reported; cave
ambience starts with the expected four stems and continues refilling its ring.
All266 Python and392 reported native tests pass; final MIPS hazards:0.
Main RAM has61,356B before the protected48KiB stack; main frame32,136B leaves
17,016B reserved for nested frames. SPU still has136,704B contiguous free.
EXE SHA-256: `11ecce96bc6d6dc7427a2968d8394926cab5ac2737918bd5e5c5986698420cda`.
BIN SHA-256: `ed5307f55dc07becfe6abb8adf9cc2715d582b2b8a0067135c2ccff20bf43f90`.
Evidence: `.hkpsx/music65/cdda-{title-loop,handoff2,early}/command.json`,
`cdda-pcm-check.json`, `cdda-build.log`, and `cdda-tests.log`.

Boss tracks are candidates for this transport once their complete arenas are
resident: CD-DA and room reads cannot run together. Boss gameplay/music is not
implemented. Tutorial remains source-silent for music, and Dirtmouth's trigger
is beyond current cooked coverage. Next: source-traced Focus/break/death/enemy/UI
sounds and the known Great Door/Crawler/Town gaps. Preserve320x240 and current
textures; this milestone makes no new FPS claim.

## Previous build:64 audio restoration

Build64 preserved
320x240 rendering and the63 cheat menus. Source running footsteps, hard-landing
sound selection and the Great Door hit callback are integrated. The door uses
its first authored hit variant and source pitch range; alternate hits and the
separate destruction sound remain missing. Hard landing selects audio only,
not the still-missing original recovery animation. No NoHardLanding components
exist in the two currently cooked scenes.

The complete199,872-byte cave-noises loop is now cached in main RAM and fed to a
16KiB SPU ring through8KiB scratch. All existing ambience sample data/rates stay
unchanged. No playback-time CD reads are added. The continuous ring preserves
predictor history and uses polled SPU IRQ boundaries, not writable ENDX flags.
A missed deadline stops the affected voice and records an underrun. All other
ambient stems remain fully resident, retaining existing fades/shared rain phase.

Final memory:63,376B player SFX,13,904B Geo,289,280B ambience SPU allocation;
136,704B contiguous SPU space remains for future music streaming. Main RAM has
63,448B unallocated before the protected48KiB stack; main frame32,144B leaves
17,008B reserved for nested frames. This is measured linker allocation, not a
physical-console stack high-water result. Player one-shots are22,050Hz, longer
running sequence11,025Hz, Geo11,025Hz, ambience8,000Hz. The user permits further
long-sample reductions if needed, applied consistently within categories.

Actual final-CUE checks: stationary5700-poll and recorded traversal routes
complete without guest faults, missed polls or audio underruns. They respectively
perform53/54 stream refills and two complete source wraps; traversal starts the
running sequence70 times. A separate100-second ambience capture verifies every
resident SPU payload, both ring halves against their expected source positions,
and the complete RAM source. A90.7666-second stereo section matches build63's
captured ambience sample-for-sample after930-sample startup alignment. Final
stationary/traversal captures were inspected. These are emulator results;
physical-console SPU IRQ/DMA timing remains untested. The traversal does not
reach a Great Door strike or qualifying hard landing; native runtime tests
cover those new dispatch paths, not an observed end-to-end door encounter.

Full make test suite passes after updating old layout/input-wrapper fixtures.
Final EXE hazard scan:0. Evidence: `.hkpsx/audio64/{build.log,tests.log,stream,
traversal,ambience,wave-comparison.json}`. Source bank quality/provenance remains
ignored. Music playback, Focus, numerous enemy/break/death/UI sounds, the door
transition delay, Crawler apron issue and Town expansion are still pending.

Original runner now intercepts217 managed audio/mixer call sites and records
actual requests in audio-calls.csv, alongside once-ready hero audio bindings.
Two300-frame traced runs pass; the first matches pre-instrumentation motion
exactly, the repeat differs only by at most0.00001 position units. Original
inputs/saves remain unchanged.14 validation tests pass; polling/native-engine
calls outside the instrumented sites and final mixed output remain outside
coverage. Evidence: `.hkpsx/og-reference/runs/audio-movement-{1,2}/`.

## Controllable headless original reference

The user's next feedback highlights the large audio gap heard in the original
reference runs. Audio restoration is now an active follow-up priority; see the
current section in MUSIC.md. The audit confirms no guest music transport and
only1,520 bytes free above current SPU banks. This update changes no playable
build; actual playback/mixer event tracing and long-clip streaming are next.

The user requested an original-game runner controlled through scripts so future
fixes can be compared against repeatable original behaviour. The Windows
Unity 6000.0.61f1 original now runs under CrossOver with a NullGfxDevice, isolated
from the Steam bottle/install/saves. See [tools/hkref/README.md](../tools/hkref/README.md).

Two final 300-input-frame movement runs pass: every consumed mask matches the
tape. Despite different startup frames, velocities, health, control/input states
and relative physics-step counts match exactly; positions differ by at most
0.00001 world units. Original walk speed is 8.3 units/s; the test walks, jumps,
lands, reverses and attacks. Both runs have zero gameplay exceptions. One run
reports a CameraController.ReleaseLock exception during shutdown after the
explicit completion marker; this is retained as teardown evidence, not hidden.
Nine host validation tests pass. Evidence is ignored under
`.hkpsx/og-reference/runs/movement-phase-1/`, `movement-phase-2/` and
`.hkpsx/og-reference/comparisons/movement-phase-1--movement-phase-2.json`.
All 1,793 original game/save inputs match before/after fingerprints.
No PS1 disc was rebuilt; 63 remains the playable pair.

Live mailbox control also passes: movement/release, direct Town loading,
teleport to (120, 15), and quit. It records 128 gameplay samples and 130 native input
updates; the extra polls occur during original scene loading and are now logged
explicitly in input-events.csv. Evidence: `.hkpsx/og-reference/runs/live-v2/` and
`.hkpsx/og-reference/live-check.json`. Direct loads/teleports are setup fixtures,
not measurements of normal traversal timing.

The driver retains native input edges, hero physics, enemies and FSMs. The
movie is bypassed as a test fixture, but the original Knight_Pickup prerequisite
and first-landing control lock are retained. Startup language confirmation was
the real first-launch blocker; the provisional splash-animation skip was
removed after the unmodified animation path passed. Input tapes wait for
original acceptingInput and grounded state. The test update clock is 60Hz;
original physics remains approximately 50Hz and both clocks are logged.
Readiness aligns their 100ms common phase so asynchronous startup does not shift
input edges by a physics step. No original physics/hero state is reset.

FSM/actor/audio-configuration sampling provides state evidence, not rendered
image or audible-output parity. Polling cannot prove every one-shot SFX call.
Next compare the reported Great Door, Crawlers and Town boundary using targeted
original/PS1 scenarios and add audio-call traces for missing SFX.

Source audits already identified concrete port issues, still unbuilt/unfixed:

- Great Door's original 2.5s delay applies to destination hero entry, while the
  port currently waits before departure.64 adds the hit sound hook only.
- Crawlers 12546/12548 freeze at the narrow collision-data apron while still
  visible; three original Buzzers are unsupported.
- Port Town coverage ends at x48; walking beyond it takes hazard recovery back
  to the gate. Original Town continues across the main street. The next content
  expansion requires an atlas-residency decision, not just wider bounds.

Ignored source evidence: `.hkpsx/great-door-audio-followup/`,
`.hkpsx/enemy-behavior-audit/`, `.hkpsx/town-expansion62-audit/RESULT.md`.
The previously pending Great Door hit-audio patch is integrated in64; its
transition timing and other sound variants remain pending.

## Playable build with cheats

The user paused FPS optimization to playtest the current port. The sole disc
now contains63: the retained54 renderer plus title/pause Cheats menus. Do not
resume generic FPS experiments during this reference-runner work.
The long-term start-to-finish objective remains unfinished; see ROADMAP.md.

Cheats default off. Title and pause both expose invincibility, Pure Nail
(21 damage), infinite SOUL and nine normal masks. Pause additionally restores
health/SOUL, adds five Lifeblood masks (up to20 from cheats), and turns all
toggles off. Toggles survive death and Select session reset, but not relaunch.
Invincibility prevents damage while preserving safe hazard/fall relocation.
Focus retains ordinary timings. Charms are not implemented, and both pages say
so rather than offering a nonfunctional unlock. The HUD displays nine normal
and up to22 blue masks over bounded rows without new textures.

63 validation passed:247 Python and379 directly reported native tests, final
binary hazards0. Three actual-CUE routes verify title/pause toggles,9 health,
99 SOUL,20 blue masks, persistence through Select, and turning all toggles off.
The normal Lifeblood route passes all four earned-health checks with no faults,
missed input polls or boundary waits; its final image is identical to54.
Pre-existing14 particle drops remain on that route. Screenshots were inspected.
No new63 FPS measurement is claimed. The user chose to test the current feel.

The final EXE is1,114,112 bytes, SHA-256
`10f85ace6f2f1381caa09ef4cc76932b96d1303ecf93350ddcfbd26d9b40f3dc`.
The canonical BIN is3,605,616 bytes, SHA-256
`ff85bd0ebf8619c4cbe8d96ef0225f6eb00fd9487cf3392cc66f3a9e54e46e00`.
Static span1,679,876;302,332 bytes remain before the protected48KiB stack.
Main frame32,120; nested reserve17,032. Scratch and resident scene/VRAM budgets
are unchanged. Evidence: `.hkpsx/cheats63-validation/validation.json`,
`.hkpsx/cheats63-lifeblood/`, `.hkpsx/cheats63-tests-passed.log` and build report.
Use `tools/validate_cheats.py --prepare PATH`, replay each generated tape with
`tools/replay_cue.py`, then `tools/validate_cheats.py --replay PATH`.

## Retained performance reference (before cheats)

**Pass54 is the retained renderer;57 was its byte-identical restoration.**
It adds exact host-cooked destruction-particle tracks:1,513 ordinals,33 phases,
two words per phase for size/radius/base spin/opacity. Source RNG and live
motion, forces, damping, contacts and speed-dependent spin remain unchanged.
Generic or unpackable emitters use scalar evaluation. Pool IDs are one-based,
with zero as the empty/scalar sentinel. Compact scene residency, packed texture
facts and row-retirement events from51 remain unchanged.

The retained route averages1.957504 VBlanks:285 /2,527 /147 /6 intervals at one /
two /three /four ticks.153/2,965 exceed two ticks, down from179/2,936 in51;
four-tick stalls fall9 ->6. The strict profiler fails only the maximum frame
interval. Average nominal30fps does not pass the maximum-two-tick gate.
Existing320×240 framing/layers/effects and48px scenery cap/approved95% static
deduplication remain unchanged. These are emulator results, not silicon proof.

Tests247 Python/367 directly reported native tests pass. A portable source-data
proof adds199,980 exact phase comparisons and334,107 cached draw/shape checks
through source-seeded trajectories, contacts and pool reuse. Run it explicitly
with `.venv/bin/python tools/test_particle_tracks.py` after a local asset cook
(historical: the harness was retired with the baked tracks when effect art
moved to per-scene disc chunks).
Final binary hazards0. Linked static span1,675,768 bytes;306,440 remain before
the protected48KiB stack. Main frame32,104; nested reserve17,048, without a
measured high-water claim. Scene arena369,180; scratch948/1,024. See BUDGET.md.

Rejected follow-ups:52 square-corner reuse regressed late intervals179 ->193.
53 tracks with a0xFFFF live sentinel gave184 late/6 four-tick;54's zero sentinel
improved the complete route. Exact I-cache tracing then identified persistent
scenery self-evictions, but55's admitted-scan extraction gave175 late/3 four-tick
and was reverted.56 moved constructor temporaries out of main, reducing its
frame to18,168 bytes, but gave158 late/7 four-tick and was also reverted.
Native correctness and lower isolated cost do not establish faster gameplay.
See PERFORMANCE.md for evidence and the separate RAM/stack tradeoffs.

Actual-CUE validation54: long endpoint region8, x163.153656/y6.390625, health4.
No input faults, missed polls, traversal CD reads, static VRAM uploads or long
boundary waits. Final89 changed pixels versus51 are within the Knight sprite;
scenery/HUD are exact. Lifeblood passes all four health checks. Great Door opens
after13hits and entersTown region93; Lifeblood/Town final images are exact51.
Lifeblood boundary waits0; Town1 frame-boundary wait tick without CD/static
uploads. Pre-existing particle drops14/23 remain. Input hashes stable; captures
inspected. Evidence: `.hkpsx/30fps-pass54-{profile,lifeblood,town}/`,54 validation
image,55/56 experiment logs, and portable particle proof.57 rebuild identity is
bound by `.hkpsx/30fps-pass57-identity.json`; no duplicate disc is retained.

Optimization is paused.58/59 force caches,60 small cuts,61 fused admission,
and62 packet-cost admission were reverted after cadence regressions.62 gave
184/2,945 late intervals and5 four-tick stalls. Evidence and unimplemented
precompute proposals are recorded in PERFORMANCE.md and ignored audits.
Do not confuse these historical trials with the playable63 cheat build.

Claude's clean worktree was removed after reviewing all nine changed files;
its committed history remains at `8db50a5` on the preserved Claude branch and
remote codex/bootstrap. Root owns builds again. See
[the salvage review](CLAUDE_OPTIMIZATION_REVIEW.md). The final source is on
`main`; assets, captures, converted tables and discs remain ignored.

The two-VBlank gate must remain explicit in future measurements:
`--max-frame-route-ticks 2 --pc-window-ticks 1 --require-seamless`.

In-progress measurements (normal EXE, same 5,871-sample controller tape):

| Build/report prefix | Mean VBlanks | Maximum | Frames over two |
| --- | ---: | ---: | ---: |
| `progression-scalar` baseline | 2.5523 | 5 | 1,222 / 2,274 |
| `30fps-pass2` collision/debris work | 2.5463 | 5 | 1,225 / 2,279 |
| `30fps-pass3` first opaque-core integration | 2.6370 | 6 | 1,346 / 2,201 |
| `30fps-pass4` in-place rectangle subtraction | 2.5716 | 5 | 1,250 / 2,257 |
| `30fps-pass5` reusable scratch / larger cut threshold | 2.5602 | 5 | 1,233 / 2,267 |
| `30fps-pass6` candidate admission / particle arithmetic | 2.4787 | 5 | 1,107 / 2,342 |
| `30fps-pass7` background cores / active fade timers | 2.4968 | 4 | 1,135 / 2,325 |
| `30fps-pass8` scoped edge readers / rotated prototype | 2.4761 | 4 | 1,103 / 2,344 |
| `30fps-pass9` partial-occlusion bbox admission | 2.4719 | 4 | 1,094 / 2,348 |
| `30fps-pass10` live solid-quad occlusion | 2.3850 | 4 | 924 / 2,434 |
| `30fps-pass11` 312-byte scratchpad workspace | 2.3742 | 4 | 910 / 2,445 |
| `30fps-pass12` partial occlusion of repaired children | 2.2814 | 4 | 712 / 2,544 |
| `30fps-pass13` shared bounds / inner-vertex solid rows | 2.2743 | 4 | 697 / 2,552 |
| `30fps-pass14` earlier first kick / i32 bound reuse | 2.2614 | 4 | 668 / 2,567 |
| `30fps-pass15` secondary solid bands / dynamic culling | 2.2954 | 4 | 736 / 2,529 |
| `30fps-pass16` cooperative presentation / smaller arena | 2.2439 | 4 | 625 / 2,587 |
| `30fps-pass17` checked legacy particle velocity | 2.2392 | 4 | 615 / 2,592 |
| `30fps-pass18` shared exact particle square root | 2.2439 | 4 | 626 / 2,587 |
| `30fps-pass19` first host-certified tile integration | 2.4618 | 5 | 1,008 / 2,358 |
| `30fps-pass20` event-driven row masks | 2.3521 | 5 | 801 / 2,468 |
| `30fps-pass21` prepare coverage during GPU wait | 2.2475 | 5 | 567 / 2,582 |
| `30fps-pass22` idle-only tile admission / polygon bounds | 2.2094 | 4 | 533 / 2,627 |
| `30fps-pass23` persistent prepared coverage | 2.2022 | 4 | 515 / 2,636 |
| `30fps-pass24` coarse retry / initial perspective helper | 2.2097 | 4 | 532 / 2,627 |
| `30fps-pass25` exact dividers / secondary scissor cache | 2.1897 | 4 | 496 / 2,651 |
| `30fps-pass26` aligned entries / core-map cache | 2.1861 | 4 | 487 / 2,655 |
| `30fps-pass27` incremental particle diagnostics | 2.1799 | 4 | 474 / 2,662 |
| `30fps-pass28` greedy + partial-tail fallback (rejected) | 2.2465 | 4 | 627 / 2,584 |
| `30fps-pass29` partial-tail fallback only (rejected) | 2.1836 | 4 | 481 / 2,658 |
| `30fps-pass30` initialized-prefix alpha output | 2.1807 | 4 | 476 / 2,662 |
| `30fps-pass31` cached draw admission / sparse reset | 2.1697 | 4 | 448 / 2,675 |
| `30fps-pass32` cached scenery RGB | 2.1661 | 4 | 442 / 2,679 |
| `30fps-pass33` release opt-level2 (rejected) | 2.2005 | 4 | 517 / 2,638 |
| `30fps-pass34` release opt-level s (rejected) | 2.3995 | 7 | 831 / 2,418 |
| `30fps-pass35` opt-level2 / forced packet inline (rejected) | 2.1885 | 4 | 493 / 2,652 |
| `30fps-pass36` original compiler / borrowed debris Spec | 2.1592 | 4 | 427 / 2,688 |
| `30fps-pass37` idle background packet preparation | 2.0494 | 4 | 235 / 2,832 |
| `30fps-pass38` full scenery bounds table | 2.0469 | 4 | 237 / 2,836 |
| `30fps-pass39` reuse prepared rectangle occluders | 2.0248 | 4 | 260 / 2,867 |
| `30fps-pass40` packed bounds indices / complete alias guard | 2.0188 | 4 | 235 / 2,875 |
| `30fps-pass41` scratch overlay / 16-piece partitions | 2.0059 | 4 | 215 / 2,893 |
| `30fps-pass42` 24 tile pieces / eight subtract pieces / radix16 root | 1.9983 | 4 | 200 / 2,904 |
| `30fps-pass43` cached sweeps / exact axis probes | 2.0031 | 4 | 214 / 2,897 |
| `30fps-pass44` allow initial idle prefix | 1.9907 | 4 | 209 / 2,915 |
| `30fps-pass45` grouped seam proofs / slow linear binding (rejected) | 2.0038 | 5 | 205 / 2,896 |
| `30fps-pass46` inverse binding / empty-mask preflight / nibble runs | 1.9850 | 4 | 195 / 2,924 |
| `30fps-pass47` textured-source outlining (rejected) | 1.9877 | 4 | 206 / 2,920 |
| `30fps-pass48` 32 terminal tile pieces | 1.9833 | 4 | 189 / 2,926 |
| `30fps-pass49` compact residency / row retirements | 1.9785 | 4 | 189 / 2,933 |
| `30fps-pass50` scratch run map (rejected) | 1.9789 | 4 | 195 / 2,933 |
| `30fps-pass51` compact residency / row events, run map reverted | 1.9768 | 4 | 179 / 2,936 |
| `30fps-pass52` square-corner reuse (rejected) | 1.9802 | 4 | 193 / 2,931 |
| `30fps-pass53` source-seeded particle tracks | 1.9715 | 4 | 184 / 2,944 |
| `30fps-pass54` zero-sentinel particle tracks | 1.9575 | 4 | 153 / 2,965 |
| `30fps-pass55` admitted occluder scan outlining (rejected) | 1.9628 | 4 | 175 / 2,957 |
| `30fps-pass56` outlined world construction (rejected) | 1.9645 | 4 | 158 / 2,954 |

### Earlier optimization measurements

Pass37 improved the mean to 2.049435 VBlanks (about 29.3 fps), with
98 / 2,499 / 232 / 3 intervals at one / two / three / four ticks. The strict
two-VBlank gate still fails. It reuses 2,231 background prefixes (178,159
packets), with 603 misses. Endpoint, error counters and the final image match
the baseline; traversal CD reads and static VRAM uploads remain zero. The full
recook/build passed with zero final-binary hazards after build ownership was
resolved. Pass38 is measuring the salvaged admission-time bounds optimization.

Pass27 was the preceding best result: 2.17994 VBlanks, approximately
27.5 fps, with 2,188 / 469 / 5 intervals at two / three / four VBlanks.
The strict 30 fps gate still fails. Its final route image matches the baseline,
and endpoint, error, CD and static VRAM counters are unchanged. All 233 Python
and 342 native/Rust tests passed, with zero final-binary hazards.

Pass28's exact rectangle merging regressed the route. Greedy append alone cost
13,885 PC samples and 61.85 million RAM stall cycles. Pass29 removed merging
and retained only a cheap partial-tail fallback; that also failed to improve
the full route (2.18360 VBlanks, seven four-tick intervals). Its final image
matches the baseline, with no observed guest/CD errors. Both fallback variants
are now removed from production and retained only as ignored experiment
artifacts. Native raster savings did not translate into overall guest gains.

Pass32 was the preceding best measured result: 2.16611
VBlanks (about27.7fps), with2,237 /439 /3 intervals at two /three /four ticks.
The strict gate still fails. Final route pixels, endpoint and error counters
match baseline; gameplay CD/static VRAM activity remains zero. All235 Python
and346 native/Rust tests pass, with zero final hazards. Cached RGB adds1,920B
RAM and preserves live GPU command bytes, UVs, palettes and texture pages.

Pass31's enabled flag/sparse reset removes about0.93ms/frame of direct
renderer/collector instruction and RAM costs. Pass30's initialized-prefix
alpha output removes another approximately0.38ms/frame of useful CPU work.
These cost estimates are not end-to-end speedup claims. The compiler-only
pass33 opt-level2 trial regressed to2.20053, with12 four-tick frames. Code
shrunk40,280 bytes, but stack RAM stalls rose while instruction-cache stalls
fell. Final route pixels and errors match baseline; arithmetic native-divide
paths remain correct. Reject this setting for the30fps target. The size-focused34 trial regressed further to2.39950, including a seven-tick
frame. Forcing the hot packet writer inline recovered part of O2's loss in35
(2.18854), but still failed to beat32. All compiler variants are rejected;
implicit opt-level3 is restored in source. The experimental disc was35 before36
restored the original settings plus a proved borrowed-debris-Spec change. Final
34/35 image differences are confined to the Knight animation box; endpoint
and error counters match baseline. Neither is accepted for performance.

Pass36 was the preceding validated best mean:2.15923 (about27.8fps),
2,261 /426 /1 intervals at two /three /four ticks. The strict gate still fails.
Only poll390 inregion1 takes four ticks. The final image differs only in the
Knight animation box; level pixels, endpoint and errors match baseline.
There are no gameplay CD reads, static VRAM uploads or boundary waits. Tests
remain235 Python/346 native/Rust, final hazards zero. Actual-CUE Lifeblood and
Town replays completed: allfourbluehealth checks pass, 13hitsopenDoorintoTown,
finalcapturesmatch25, anderrorsarezero. Town36 recordszero boundary-wait ticks;
pre-existingLife/Town particle drops remain14/23. No active CUE readers.

Pass37 implements CPU-only next-BACK-prefix preparation after final DMA has
drained, during the prior frame's GPU/VBlank wait. It must stop on complete
source boundaries at the flip, save partial tile rows and packet budget, and
forbid uploads or GPU commands before the normal post-flip path. Exact camera/
final-state mismatch discards it. Repeated simulation ticks may replace a
prefix only after invalidating it before rebuilding Events. Existing matching
full prefixes retain their Events unchanged. Main visual applies were reviewed
as idempotent; native renderer proofs and full replay remain required.

An exact unchanged-camera BACK packet cache has native proofs but is not in
production. Its sparse256B key can represent64 touched draws; overflow skips
caching. Only59/474 earlier slow frames repeated camera, limiting its scope.
Exact duplicate fog geometry and partial dynamic occlusion had no useful
candidates in heavy snapshots. A particle spatial index was rejected after
measuring only about0.043ms/frame of remaining collision work. Root owns builds.

Retained recent optimizations include exact particle dividers, a two-way alpha
mapping cache, aligned cache entries, a complete-key core-map cache and an
incremental particle occupancy count. Pass27 static span is 1,954,828 bytes,
with 27,380 bytes before the reserved 48 KiB stack. Its main frame is 25,520
bytes; 23,632 reserved bytes remain for nested calls. Scratchpad use is 972
bytes. These are linked budgets, not observed stack high-water measurements.

Pass25 is the preceding validated gameplay build:2.18974 VBlanks
(about27.4fps), with2,155 two-tick /489 three-tick /7 four-tick intervals. It
still fails30fps. All233 Python/329 direct native tests pass; final hazardszero.
The complete route reaches the same endpoint with no guest errors, traversal
CD reads or static VRAM uploads. Final route/Lifeblood/Town images all match
baseline pixels. Lifeblood's four earned-blue-health checks pass; Great Door
opens after13hits intoTown. Town retains one pending transition wait tick;
Life/Town particle drops14/23 remain the pre-existing bounded-pool limitation.

Pass24's coarse retry was removed after a full-route regression. Its perspective
correction loop was unexpectedly converted by LLVM to software wide division;
pass25 uses explicit bounded corrections. Final MIPS confirms one native divide
for perspective and two for exact64/32 velocity scaling, with wide fallback
outside proven domains. Arithmetic samples fall27% versus24. Multi-million
arithmetic comparisons and full trajectory/vertex tests retain exact values.
The128-entry2-way secondary alpha cache uses5,184B plus8B telemetry and reuses
25,717 of41,539 primary misses (61.9%). Selected cache helper samples fall
about2,500. Final MIPS revealed unaligned Entry copies; align4 without growing
40B entries is the next correction, alongside a small pure core-map cache.
Current static span1,953,540 leaves28,668 bytes before48KiB stack; main25,512,
scratch972. No hardware timing or stable30fps claim.

Pass22 is the preceding measured build and the best mean so far (about27.2fps),
but still fails the two-VBlank gate:2,094 two-tick /516 three-tick /17 four-tick
intervals. Endpoint/error counters match baseline and the final display is
pixel-identical. Pass21's64 four/five-tick frames all missed prepared coverage;
synchronous preparation and per-draw tile work cost more CPU during particle
bursts than they saved in GPU time. Pass22 uses tiles only when preparation
already succeeded during the prior GPU wait. Misses retain ordinary rendering.
Native GPU hit/miss proofs cover48 frames /3,686,400 identical16bit pixels.
The collision helper now rejects distant polygon bounds before edge tests;
159,049 differential cases plus boundary/extreme checks match the old algorithm.
Combined collision-helper samples fall41%, RAM-load stalls47%. Main shrinks
10.6KiB because two formerly inlined polygon instances now share the helper.
All232 Python/309 direct native tests pass, and final hazards arezero.
Static span1,944,252 leaves37,956 bytes before the48KiB stack reserve; main
frame25,512 bytes. Scratchpad972/1024 bytes. Next: retain prepared coverage
across unchanged-camera frames and investigate remaining exact particle and
bounded clipping savings. Do not claim30fps or hardware timing.

The preceding particle-only pass18 completed its route.
Its final display is pixel-identical to the gameplay baseline, all 226 Python
and 299 direct native/Rust tests pass, final hazards are zero, and endpoint/error
counters match. Its slightly worse route mean does not establish a frame-rate speedup.
The shared exact square-root helper is retained for measured CPU savings:
combined particle/break tick and root samples fall12,264→11,284 (8.0%),
RAM-load stalls40.990M→36.950M, and the targeted burst samples867→806.
The static span also shrinks2,048 bytes. GPU timing remains the main limiter.
Pass18 actual-CUE Lifeblood/Town replays also completed and both final images
match pass16 byte-for-byte. All four earned-blue-health checks pass. Town
records one boundary-wait tick immediately after the Great Door warp and before
region93 activates (poll9946); CD/static VRAM counters remain unchanged. This
is a pending gate transition, but must not be reported as zero waits.


Reports live under `.hkpsx/<prefix>-profile/`. Every row still fails the strict
two-VBlank acceptance gate. The final display of passes 3–10 is pixel-identical
to the baseline; this does not prove every intermediate gameplay image. All
complete routes retain the baseline endpoint and report zero guest faults,
boundary waits, traversal CD reads and static VRAM uploads. Pass 3 regressed
because large by-value rectangle partitions introduced copying; final MIPS
inspection of pass 5 confirms those scenery/subtraction `memcpy` and `memset`
calls are gone. Pass 7's background-core addition regressed the mean and was
removed from the next experiment; its active-fade timer change is retained.
Some heavy regions still issue 35–40 ms of GPU work per frame
in the emulator. Do not describe any of these experiments as stable 30 fps.

Pass 8's textured rotated-core counters stayed zero: its targeted source reveal
mask is transparent along this live route. The static-camera coverage estimate
did not account for that state. This unused guest integration is removed from
the next experiment; its isolated pixel proof is not a performance result.
Scoped collision-edge readers and exact bounding-box rejection before partial
occluder copying/ranking remain. Actual pass-8 RAM snapshots in five slow
regions establish that the large solid black geometry stays visible and fully
opaque, unlike the reveal mask. Pass 10 selects one conservative
rectangle inside a legal convex solid quad, with cached geometry and unchanged
source order. It improves the route mean from 2.4719 to 2.3850 VBlanks (about
25.2 fps), with 924 frames still above budget. Region 6 improves from 2.817 to
2.115 VBlanks, while regions 16, 18 and 20 remain near three. The native GPU
proof checks 72,353,484 pixels plus 2,906,584 pixels from runtime geometry.
Pass 11 moves 312 bytes of temporary occlusion state into the PS1 scratchpad;
its build, target layout assertions, native tests and final hazard scan pass,
and its route completes with mean 2.3742 VBlanks, five four-tick frames (down
from thirteen), unchanged endpoint and zero guest/CD/input errors. Its final
image differs only inside the Knight animation bounds; scenery is identical.
Region activations are 44 rather than 42 as frame phases shift, with the same
two resident scenes and zero traversal CD/static VRAM activity. Pass 12 corrects the partial-occlusion bypass for oversized subdivided polygons.
It retains each actual child XY/UV and source order and uses the existing
optional packet reserve. A 117,964,800-pixel native differential covers source
order, reflections, both blend modes/framebuffers and reserve exhaustion.
The full replay improves to 2.2814 VBlanks (about 26.3 fps), with 712 frames
still above budget. Repair occlusion activates on 9,058 children; capacity
fallbacks and guest/CD/input errors remain zero. The final image is identical
to baseline, and endpoint/resident activity are unchanged. Region 6 is now
2.008 VBlanks, region 16 is 2.570, but regions 18/20 remain near three.
Pass 13 combines shared screen-bound calculations with two extra cached
solid-rectangle search rows. It reaches 2.2743 VBlanks, with three four-tick
frames and 697 frames above budget; final display is pixel-identical and all
error counters remain zero. All 224 Python plus 294 native/Rust tests pass.
Final MIPS confirms duplicate bound calculations are removed, but increased
register spills partly offset this CPU saving. Pass 14 refines shared bounds to i32 and submits the first packet sooner.
First-kick delay falls about 0.61 ms in regions 15/16, and the route mean is
2.2614 VBlanks (about 26.5 fps). Three four-tick frames remain; 668 frames
still exceed two. Endpoint and error counters are unchanged; final-image
differences are confined to the Knight animation bounds. Pass 15 added two cached bands inside a steep secondary solid and FRONT-only
whole dynamic-packet culling. It **regressed** to 2.2954 VBlanks with eleven
four-tick frames. The isolated band estimate did not survive runtime costs:
regions 18/20 save only about 0.2/0.55 ms of issued GPU work, and CPU overhead
outweighs that. The band integration is removed from pass 16; archived proof
and actual regression data are in `.hkpsx/gpu-30fps-audit/band-production-proof.json`.
Dynamic culling activates on 2,938 packets; it is retained for burst coverage.
The final image differs only in the Knight animation, and errors remain zero.
Pass-15 Lifeblood and Town CUE routes also complete with unchanged evidence
inputs and zero faults. Lifeblood passes all four earned-blue-health checks;
Town reaches region 93 after thirteen Great Door hits.

Pass 16 builds successfully with zero final MIPS hazards and cooperative presentation progress inside existing
input/particle checkpoints. Pass-14 burst frames showed 7–9 ms between final
DMA kick and queuing the flip while simulation ran; this change checks progress
inside those phases while preserving final-DMA completion and the SDK VBlank
GPU gate. Four native state-machine tests cover 65,536 busy/idle schedules.
The scene arena is reduced to 1,069,056 bytes, recovering 110,592 bytes after
the exact pinned SDK and incremental decoder both pass the current payloads
at all five slice budgets. The proven aligned minimum is 1,063,224 bytes,
not just the 1,063,220 raw bytes; sector padding and ambience startup reads are
included. Evidence: `.hkpsx/scene-arena-audit/report.json`. No assets are removed.
Its linked static span is 1,860,712 bytes, leaving 121,496 bytes before the
unchanged 48 KiB stack reservation; main frame is 25,552 bytes. The native input-wrapper harness was updated for the new service hook; the
full suite passes (226 Python and 295 native/Rust tests). The pass-16 route
completes with mean 2.2439 VBlanks, six four-tick frames, unchanged endpoint,
zero faults/input misses and a pixel-identical final display. The final-DMA
kick to flip-queue delay is now at most one scanline in burst regions, down
from 120–147 scanlines in the earlier slow-frame examples. 3,038 dynamic packets are culled. GPU-heavy regions and particle
update/construction bursts still miss the two-tick target. Lifeblood/Town replays on this CUE both pass with unchanged evidence inputs:
all four blue-health checks, thirteen Great Door hits, Town ambience mask 54
and six stem starts. Town final display is identical to baseline; Lifeblood
differs only inside Knight animation bounds. Pass 17 applies exact checked-product velocity scaling to legacy particles.
It improves mean to 2.2392 VBlanks, with five four-tick frames; final image,
endpoint and errors match baseline. Native full-pool differential tests cover
384 ticks and both checked/wide paths. Final MIPS keeps three native DIVs and
three guarded wide fallbacks; software-call samples decrease. Static span is
unchanged. Rotated-alpha cropping is rejected after fresh pass-16 packets show
less than 0.18 ms potential saving. Earlier pass-8 estimates are superseded in
`.hkpsx/rotated-alpha-audit/REPORT.json`. The next larger prototype is host-cooked
opaque coverage certified across every Q8 rounding phase, so runtime work is
translation/clipping rather than a new per-frame geometry search. It remains
ignored experimental code until actual packet-budget/CPU/GPU gains are proved.
These numbers still do not meet stable 30 fps.
Earlier exploratory coverage estimates that paired stale per-region texture
IDs with final scene mappings are explicitly invalidated in
`.hkpsx/black-mask-audit-corrections.json`; use hash-verified final packs and
actual replay results. The production opaque-core extraction and pixel proofs
used the correct final data and are unaffected by that diagnostic error.
The canonical disc is being replaced by these measured experiments during
active work; the historical build identity below is not its current identity.

## Previous verified gameplay baseline: Great Door progression and Lifeblood

The sole `~/Downloads/ps1 games/hk-psx.cue` is updated. The final normal build
contains a source-derived Great Door controller, smaller particle storage,
scalar debris comparisons and a corrected main-stack reservation. It retains
the existing tilemap fills, parallax, 95% approved static deduplication, Geo,
menus, read points and shared Tutorial/Town residency. No additional quality
reduction or disc copy was introduced. Source changes remain local on
`codex/bootstrap`; this continuation did not commit or push.

The final King's Pass exit was a distinct PlayMaker `Great Door`, not an ordinary
Breakable: its collider remained permanent and its tk2d MeshRenderer art was
omitted. The cooker now binds all three source poses. Nail hits change stages at
four/eight hits and open it at thirteen, with a nine-tick hit cooldown. After a
152-tick input lock the existing gate places the Knight at Town's `left1` entry.
Opened state survives scene return and death during this session; Select resets
it. Dedicated door sounds, stage debris, shake and original fade choreography
remain missing. See [GREAT_DOOR.md](GREAT_DOOR.md) for source and approximations.

Actual final-CUE validation, all using ordinary controller tapes and unchanged
EXE/map/disc/frontend/input hashes:

- `.hkpsx/progression-scalar-town/`: all 10,271 samples complete. All thirteen
  hits and stage thresholds are observed; transition fires exactly 152 ticks
  after the last hit. Ends in region 93 at `(2.5,44.390625)`, health 4. Surface
  ambience reaches mask 54 after the 30-tick fade, with six total stem starts:
  the shared stems are retained. Inspected all door stages, entry, settled Town
  and final software/hardware captures. `great-door-coverage.json` binds the
  evidence hashes. This proves the entrance, not complete Dirtmouth content.
- `.hkpsx/progression-scalar-lifeblood/`: all 6,168 samples complete. The cocoon
  releases two Scuttlers; striking them earns two blue masks after the source
  delay. Three hazard hits show `(blue,ordinary)` health
  `(2,4) -> (1,4) -> (0,4) -> (0,3)`. Touching a bug is not the reward trigger.
  `tools/validate_lifeblood.py` passes all four checks; the health capture
  sequence was inspected. Ends in region 44 with ordinary health 3, blue 0,
  Geo 30 and no death. See [LIFEBLOOD.md](LIFEBLOOD.md).
- `.hkpsx/progression-scalar-profile/`: the 5,871-sample user tape completes at
  poll 5,962, ending at `(163.153656,6.390625)`, region 8, health 4. Its final
  capture shows the level. All three routes have zero guest/CD/input/missed
  input/geometry-repair faults and zero boundary waits. Gameplay CD sectors
  remain 484, static VRAM bytes remain 694,048 and scene admissions remain two.

Frame pacing improved modestly. The freshly rescanned baseline had mean 2.568
VBlanks, maximum five, with seven five-VBlank frames. The final build has mean
2.552, p95 three, maximum five, with three five-VBlank frames (histogram
2:1,052 / 3:1,191 / 4:28 / 5:3). **The strict four-VBlank gate still fails.**
An intermediate feature build had a six-VBlank spike during debris/activation;
replacing four out-of-line array comparisons with exact scalar operations
removed that spike in the final replay. Frame alignment changes with pacing;
this is emulator evidence, not a hardware or frame-identical A/B claim. See
[PERFORMANCE.md](PERFORMANCE.md).

The 128-particle pool shrinks from 6,668 to 6,156 bytes using a nonzero lifetime
representation, preserving particle counts and physics. This is a 512-byte
saving in stack-resident world state, not BSS. Capacity rejections remain:
14 on the Lifeblood route and 23 on the longer Town route. The original crash
prefix's 14 rejections came from overlapping source bursts, not leaked slots
(`.hkpsx/particle-overflow-2026-09-14.json`). Do not claim complete effects or
increase the pool without measuring RAM, primitive and CPU cost.

The previous main frame used 33,216 bytes against a 32,768-byte reservation.
The build now derives a project linker script from the unchanged pinned SDK,
protects 48 KiB and verifies the final EXE's main prologue before packaging.
Current main frame is 25,560 bytes; total static span is 1,968,672, leaving
13,536 bytes before the reservation. Main-stack high-water remains unmeasured.
The two resident scenes total 1,063,220 bytes; arena space remaining is 116,428.
There are 20 static 4bpp pages, 1,209 of 1,248 CLUT slots used and only 608
unallocated VRAM bytes. Detailed budgets: [BUDGET.md](BUDGET.md).

Build/test logs: `.hkpsx/progression-scalar-build.log` and
`.hkpsx/progression-scalar-tests.log`. All 217 Python and 281 Rust/native tests
pass (498 total); final MIPS branch-hazard scan is clean. Build identity:

- EXE SHA256 `714737536355697e985c13eb069de43656c025794cbd735fe635d345cbca3cc4`
- BIN SHA256 `c8b402a1d41934ae51318068c98a082e867f3451ca61dc58a1a2640abd8d8f3c`
- CUE SHA256 `847e6ea3d0762d2163e0616a19c01b3ae85c469748318c588e14656bbc7f98fb`

Next: profile the remaining five-VBlank burst frames, measure main-stack
high-water and particle overlap demand, then finish Great Door/Lifeblood effect
families and expand beyond Town's entrance with bounded area residency. Title/
Dirtmouth music, flying enemies, abilities, NPCs, shops, saving, Shade recovery,
bosses and full progression remain incomplete. Earned Focus still lacks a
successful controller coverage route. Reproduction commands are in NEXT_PROMPT.md.
Everything below is historical milestone evidence, superseded by this section.

## Historical rescan: verified baseline at `6517d70`

No guest changes or disc rebuild were made during this rescan. The main checkout
and the existing Claude worktree are both at `6517d70`; their normal EXE and
map hashes match the current canonical disc's build report. The report referenced
the worktree's artifact paths. Those EXE/map files were subsequently found to be
hard links to the root outputs and have since been overwritten by builds. Use
this rescan's preserved `artifacts/` snapshots for historical comparisons.
`tools/doctor.py` finds the Windows CrossOver installation and Unity `6000.0.61f1`;
this checks the installation layout, not complete Steam asset integrity.

Fresh checks on September 14:

- `make test`: 210 Python and 279 Rust/native tests pass.
  Log: `.hkpsx/rescan-2026-09-14-tests.log`.
- `tools/replay_cue.py` with `.hkpsx/user-crash-4/extended.pxtape`:
  all 3,558 tape polls complete, zero guest/CD/input/missed-input/geometry-repair
  faults and zero boundary waits. Seven breakables were destroyed; the final
  wallet is 30 in region 44. The final software and hardware images are byte-identical
  to `.hkpsx/trim-crash/`. Inspected a 12-frame contact sheet and final capture;
  the displayed terrain remains present and black interiors cover the background.
  This route records 791 particle spawns and 14 pool-rejected spawns, so effects
  are still capacity-limited. Evidence: `.hkpsx/rescan-2026-09-14-crash/`.
- `tools/profile_tape.py` with the preserved 5,871-poll user tape and
  `--require-seamless`: emulator completes at poll 5,962 with unchanged input
  hashes and zero recorded faults. The strict gate correctly fails: 2,260 render
  intervals have histogram 2:1,021 / 3:1,202 / 4:30 / 5:7 VBlanks, mean 2.568,
  p95 3. These interval statistics exactly match `.hkpsx/trim-profile/`.
  During gameplay CD sectors and static VRAM upload bytes stay constant,
  with zero boundary waits across 43 region activations. Animation frame uploads
  still occur from RAM. Endpoint: (163.153656, 6.390625), region 8, health 4.
  Evidence: `.hkpsx/rescan-2026-09-14-profile/`; the inspected final capture
  shows the level. This is emulator evidence, not hardware timing.

The current pack contains **98 spatial views across Tutorial and Town entrance**,
not 98 distinct full game rooms. Both scenes are preloaded; this is not yet a
whole-game area streaming solution. `BUDGET.md` now reflects the actual packed
sizes and generated capacities: 1,060,140 resident scene bytes, 119,508 arena
bytes free, 1,120 texture slots, 480 scenery records and a 264-packet dynamic
reserve. Only 36,084 bytes remain before the reserved main stack, whose high-water
is unmeasured; gameplay VRAM has only 608 unallocated bytes. Older sections and
feature-specific audio reports describe earlier allocations and binaries.

Next concrete work: establish a controller-only route through the Lifeblood
cocoon and validate blue-health acquisition/damage, then natural Town traversal.
Profile the remaining activation/burst spikes against this baseline without
simplifying source parallax. Investigate particle pool rejections before claiming
complete destruction effects. Measure main-stack high-water and plan bounded
area residency before adding substantial content. Title/Dirtmouth music, flying
enemies, abilities, NPCs, saving, shops, Shade recovery, bosses and full progression
remain incomplete. The prior particle and Geo `QueueFull` regressions are fixed
on the replayed routes; they are not the current active blocker.

## Earlier milestone: geometry fills, destruction effects, Lifeblood and burst pacing

This build is validated on the sole disc and supersedes the Geo milestone's
hashes below. It restores the opaque black `tk2dTileMap` interiors the cooker
had skipped, spawns the source `Breakable` particle families for all 33
supported Tutorial breakables, adds the Lifeblood cocoon, Scuttler and
blue-health logic, and fixes the simulation stall that the first destruction
burst caused. Details: [GEOMETRY.md](GEOMETRY.md),
[BREAK_EFFECTS.md](BREAK_EFFECTS.md), [LIFEBLOOD.md](LIFEBLOOD.md).

### Destruction burst and input queue

The first actual-CUE replays after a burst panicked with `input sampler
contract`, fault `QueueFull`. Around 105 particles each rescanned every room
edge through a callback that returned 16 bytes through memory and tested them
with `memcmp`; a sim tick then cost more than one VBlank, the fixed-step loop
fell behind real time and the 16-sample poll queue filled after about 24
VBlanks (`.hkpsx/break-overload-profile/`, fault at poll 383).

The tick now integrates every particle first, unions their swept boxes, decodes
the non-empty room edges once per tick into a 128-entry scratch keeping only
edges that touch that union, then collides each particle against that subset in
edge order. Array equality tests became scalar tests, `lerp` uses an exact
32-bit division identity instead of `__divdi3`, and source-plane particles skip
a 64-bit division per draw. `tick_debris` builds its filtered edge cache in one
pass over the exclusion lists. Particle counts and trajectories are unchanged:
the synthetic full-pool differential, the fallback path beyond the scratch and
the generated-source differential (33 owners, 650 ticks, 2,745,600 slot states)
match the naive reference exactly, and both actual-CUE routes end at the same
positions as the intermediate build (`.hkpsx/burst-fix-*`).

| Long 5,871-poll profile | Worst frame | Frames over 6 VBlanks | Poll queue peak |
| --- | ---: | ---: | ---: |
| Before this fix | fault at poll 383 | n/a | 16 (full) |
| Edge scratch only | 21 VBlanks | 6 | 11 |
| Two-pass particle tick | 7 VBlanks | 0 | 4 |
| Current build | 5 VBlanks | 0 | 2 |

The user's first play recording on that build (2,958 polls, preserved with its
hash in `.hkpsx/user-crash-input/`) froze the screen in region 45 with the same
`QueueFull` fault from a second path: 16 loose Geo coins with seven broken
objects, each coin re-decoding and re-filtering every room edge through
`State::edge` per tick (`.hkpsx/user-crash-4/`, reproduced by extending the
tape 600 polls past the freeze; the tape ends at the freeze because a halted
guest stops polling). `State` now owns the filtered edge table, refreshed once
per simulation tick in `tick_debris` and invalidated by every mutator of the
Geo, Lifeblood and breakable exclusion sets, so coins, Scuttlers, enemies,
corpses and the Knight read one table. The extended tape now completes all
3,558 polls with a poll queue peak of 2, 17 coins active
at most, ending in region 44 with wallet 30
(`.hkpsx/coin-fix-crash/`). Both standard routes end at identical positions.

Any sustained simulation tick above one VBlank still halts the game by design:
the 16-sample poll queue faults rather than dropping input. That is the right
contract for a fixed-step simulation, but it means every per-body edge scan
must stay bounded; grep for `state.edge(` before adding another body type.

### Final validation

`python3 tools/build.py` wrote the sole disc; the final EXE has zero R3000
hazards (`.hkpsx/trim-build.log`). `make test` passes 210 Python and 279
Rust/native tests (`.hkpsx/trim-tests.log`). Actual-CUE replays completed
with unchanged EXE/map/disc/tape/frontend hashes and zero guest, CD, input,
missed-input and geometry-repair faults:

- `.hkpsx/trim-profile/`: the standard 5,810-poll user tape, 2,260
  render intervals, mean 2.568 VBlanks, p95 3, maximum 5; 7 intervals
  exceed four VBlanks, so **the strict four-tick gate still fails** (it also
  failed on the 2026-09-09 reference, mean 2.807). Endpoint (163.154,
  6.39062), region 8, health 4, zero faults. The guest culled 24,079 draws.
- `.hkpsx/trim-ascent/`: the 2,450-poll Lifeblood route ends in region 2 at
  (54.4249, 11.3906); this run ended with health 4 and the only differing
  pixels are the HUD health masks (pacing moved the hazard hit). The cocoon
  is still never reached.
- `.hkpsx/trim-geo-pause/` and `.hkpsx/trim-crash/`: same endpoints as the
  references; the crash frame is byte-identical and the 223 differing
  spawn pixels lie inside Knight packet boxes, the known pacing effect on the
  Knight's animation.

Captures every 30 or 60 route ticks were inspected around the first burst and
at the route ends: stone and plant particles animate and settle, platform
interiors are opaque black, and the inspected frames show no background wedge
under a platform. A dark King's Pass frame that previously showed a dark-blue
wedge on its right now shows black tilemap interior there. During a
100-particle burst frames take four to five VBlanks.

Linked code 334,608B, data 267,504B, BSS 1,360,396B, static span 1,962,508B; the gap
before the reserved 32KiB stack is 36,084B. Main-stack high-water remains
unmeasured. EXE 604,160B, SHA256 `cdc8524c1f1cfc3f7027dc06629382bca712d48e436382b91fbdb374a4476a9f`. BIN 3,544,464B, SHA256
`2e46213aa232d59efbb8a1c9852ad7c08fa80483b1d41c8ec149a7ad34159859`. Map SHA256 `d743b62d18ed19684b2ea4e7b0b1d67e662a21e56a711074256775215902197e`.

### Frame pacing

The renderer now kicks packets progressively as separate DMA lists so the GPU
starts 1.8 ms after the flip, draws cooked black-mask cores as flat quads and
subtracts the largest front-layer opaque black rectangles from earlier
axis-aligned draws, all pixel-exact. The long profile's mean render interval
fell from 3.262 to 2.807 VBlanks (444 of 2,068 frames at two), the stationary
spawn camera from 3.00 to 2.61. Frames fit two VBlanks below about 31 ms of
emulated raster; the route's median frame is 33.1 ms, so a consistent 30 fps
still needs 15 to 25% less raster than exact rendering of the source layering
produces. Analysis, tools and the decision options are in
[PERFORMANCE.md](PERFORMANCE.md). Because pacing moves region activations, the
ascent route now ends with health 3 and wallet 0 at the same position.

On 2026-09-11 two further exact changes landed: draws whose screen bounding
box lies entirely under later opaque black are skipped whole, and occlusion
subtraction now applies to rotated quads too, since the emulator clips spans
per scanline and samples texels from per-pixel plane equations. The long
profile's mean fell from 2.807 to 2.568 VBlanks (1,021 of 2,260 frames at
two, after a cheaper culling check);
completed final frames cost 22.1 ms at spawn, 32.8 ms beside the cocoon and
24.2 ms on the ascent. A Gouraud replacement for the fog sheets was measured
and rejected as inexact. The parallax breakdown that motivated this is in
PERFORMANCE.md; simplifying the layering was ruled out.

## Geo and functional menus: preceding validated build

The sole normal disc now includes five source Geo rocks, Crawler payouts,
animated loose coins, collection and a wallet HUD. Mining preserves original
hit counts, cooldowns and per-hit/final payouts; depleted rocks use original
broken art. Coins retain state across spatial views and use bounded deterministic
physics. The source collision matrix confirms rocks do not block the Knight.
Original pickup/rock-hit sounds are resident. Details and limits: [GEO.md](GEO.md).

The title has Start Game, Options and Controls. Start in gameplay opens a pause
menu. Sound-effect and ambience levels apply to playing voices, including full
mute, and persist for this run. Cross/Start on the initially selected Start Game
still enters directly, retaining the existing recording's launch sequence.

All 47 coin Idle/Air animation frames plus intact/depleted rocks use 57 frame
references to 50 unique textures. Their 5,154-byte 4bpp bank occupies disjoint
VRAM strips. Reducing the unused scenery CLUT reservation to 1,248 slots (1,206
used) provides space without altering current scenery textures. The new 13,904B
Geo sound bank leaves 1,520 SPU bytes. No mining or pickup event reads the CD.

Final linked code is 278,296B, data 239,840B, BSS 1,462,288B; the gap before the
reserved 32KiB main stack is 18,160B. Main-stack high-water remains unmeasured.
Preserve the particle, scene-admission, title, dialogue and Geo outlining
boundaries: inlining large functions can exceed MIPS PC16 branches. The actual
final EXE passes its R3000 hazard scan with zero hazards.

### Final validation

Actual-CUE replays completed with unchanged EXE/map/disc/tape/frontend hashes.
The 6,971-poll long route plus mining/pause tail depleted one five-hit rock,
registered two Crawler kills, spawned 19 Geo and collected 18, leaving one coin
and no pending payout. All five rock-hit events, one destruction event and
16 pickup sound events fired. The wallet stayed visible; source coins, intact/
broken rocks, title/options/controls, pause pages and the level were inspected
in 16 final-build captures, including software and hardware final views.

The initial feature replay exposed five missed input samples at 14 moving
coins. Production checkpoints between coins/emissions and bounded contact scans
remove all five in the final replay. Guest, CD, input, missed-input and geometry
repair fault counters stay zero. Pausing preserves the Knight's position across
280 route samples and resumes correctly. Title and pause settings both work;
a separate full-mute replay has zero nonzero PCM samples. Listening quality and
physical SPU output remain unverified.

The standard 5,871-poll route retains zero boundary waits, zero traversal CD
reads and zero static texture uploads: sectors481, scene admissions2 and
static VRAM bytes693,952 remain constant across41 post-start activations.
Its mean render interval is3.3995VBlanks, p954, maximum5. **The strict four-tick
frame gate still fails:20 of1,707 intervals take five ticks.** This is slightly
slower on average than the shared-only3.3797 result; changed pacing can also
alter trajectories, so the runs are not frame-identical A/B measurements.
No complete-seamlessness or physical-console performance claim is made.

The suite passes198 Python tests and244 Rust/native tests, with further native
scenarios run inside Python wrappers. Ten Geo scenarios include64 coins against
1,024 edges with differential physics checks. The final EXE hazard scan passes.
Evidence: `.hkpsx/gameplay-expansion-final-tests.log`,
`.hkpsx/gameplay-expansion-final-build.log`, `.hkpsx/gameplay-final-cue/`,
`.hkpsx/gameplay-final-menu/`, `.hkpsx/gameplay-final-muted/`,
`.hkpsx/gameplay-final-profile/analysis.json`,
`.hkpsx/gameplay-final-review/validation.json` and its contact sheet.

Replay without rebuilding or opening the GUI (choose a new ignored output dir):

```sh
python3 tools/replay_cue.py --tape .hkpsx/gameplay-expansion-input/geo-pause.pxtape --output .hkpsx/geo-replay --screenshot-interval 30
python3 tools/profile_tape.py --exe dist/hk-psx.exe --map build/hk-psx-normal.map --cue "$HOME/Downloads/ps1 games/hk-psx.cue" --tape .hkpsx/user-6246-input/latest.pxtape --output .hkpsx/geo-profile --require-seamless
```

The profile command intentionally returns nonzero while its frame gate fails.
Input-tape construction is recorded in `.hkpsx/gameplay-expansion-input/README.md`.

The canonical pair remains `~/Downloads/ps1 games/hk-psx.bin/.cue`.
EXE SHA256 `5dac23d7f1da68ff9975c7b748349060bf41cb0d2a654cbb02b34c0e8a835e2e`;
BIN SHA256 `345a178b71fa03fe2ec92093d45b5f4a1cfdad0f4071d69b278f80ac0c66f89e`.
`.hkpsx/build.json` binds exact output sizes/hashes, source reports and map.

Still missing: Geo spending/shops, saving, recoverable Shades, full rock gleam/
jitter/destruction effects, title animation/music, flying enemies and complete
scene scripting/progression. Death currently removes Geo permanently; this is
an explicit incomplete system. Rendering cadence and memory headroom need work
before larger gameplay/content expansion. No full-game or hardware parity claim.

## Previous milestone: shared scene residency

The single main disc now contains two HKSCNE01 scene banks instead of98
independent room payloads. All98 local region views and the currently authored
Tutorial/Town content stay in RAM and VRAM. Exact index-plane sharing with
independent palettes preserves sampled16-bit texels; no additional resolution
reduction or lossy merge was applied beyond the approved95% cook.

| Resident scene | Raw RAM bytes | Stored disc bytes | Static pages | Palettes |
| --- | ---: | ---: | ---: | ---: |
| Tutorial |929,692|452,831|18|1,065|
| Town entrance |116,756|46,395|2|141|
| Total |1,046,448|499,226|20|1,206|

The1,179,648-byte shared arena admits the larger scene first, then decodes Town
in the remaining suffix. Both pinned SDK and incremental decoding match exact
raw bytes and preserve the first admitted scene. Scene validation yields bounded
work; local draw/frame/clip/edge references retain their original ordering.
All98 serialized region geometry packet bounds pass. Source world metadata is
regenerated even on a cook-cache hit. Animation keys refer to global scene
textures, retaining slots across spatial crossings and resetting on scene changes.
Native review covers96 same-scene consecutive pairs; all reuse checks show zero
uploads. The single cross-scene pair resets its animation namespace.

The linked normal build uses225,100 code bytes,204,976 data bytes and1,461,452
BSS bytes. It leaves107,060 bytes before the reserved32KiB stack, versus44,428
previously. Main-stack high-water and hardware performance remain unmeasured.
The final EXE has zero R3000 hazards. Startup scene admission is outlined to
avoid the MIPS PC16 range error; preserve that boundary and the particle outlines.

### Current replay evidence

Both the actual main CUE and profiled EXE complete the same5871-poll long tape,
ending at(163.153656,6.390625), region8, health4, with two kills. Across43
post-start region activations the CD sector counter stays481, admitted scenes
stay2 and static VRAM bytes stay693,952: **zero gameplay CD reads, scene decodes
or static texture uploads**. Both boundary-wait counters stay zero. Input,
guest/CD and geometry-repair fault counters stay zero. All input VBlanks were
sampled. This removes measured loading freezes on this route.

| Long replay | Boundary waits | Worst wait | Mean frame interval | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Previous95% build |478ticks|93ticks|3.663VBlanks|5|6|
| Shared scene build |0ticks|0ticks|3.380VBlanks|4|5|

The strict seamless gate **still fails only its four-tick frame budget**:
16 of1,717 measured intervals take five VBlanks. Average interval falls7.74%,
but simulation/poll paths can diverge; matching inputs/endpoints do not make
this a frame-identical A/B. Rendering cadence is now the next performance task.
Startup loading is longer because both scenes are admitted before play.

Actual-CUE software and hardware final captures show the level;22 sampled
captures from the107-image title/loading/gameplay sequence were inspected.
The first-door replay shows four active/drawn fragments and source grass
particles. The long route finishes with20 settled fragments and66 bounces,
275 particles spawned and zero pool drops. Particle totals differ with route
and pacing. Full effects/shaders remain incomplete. This tape stays in Tutorial;
Town banks and all views pass native validation, but natural Town traversal and
its audio change have not been newly demonstrated.

Validation:422 tests pass (186Python +236Rust/native), including exact scene
mapping, palette sharing, bounded validation and sequential decoder checks.
Evidence: `.hkpsx/shared-residency-tests.log`, `.hkpsx/shared-residency-build.log`,
`.hkpsx/packed-scenes.json`, `.hkpsx/shared-residency-profile/analysis.json`,
`.hkpsx/shared-residency-cue/`, `.hkpsx/shared-residency-door-cue/`.
The actual CUE and EXE/map/tape/emulator hashes are bound in replay reports;
inputs remained unchanged during validation. The prior report is retained in
`.hkpsx/shared-residency-baseline/` without another disc copy.

Current playable file: `/Users/ebonura/Downloads/ps1 games/hk-psx.cue`.
No candidate/backup/telemetry disc was created, and no GUI was opened.
EXE SHA256: `f50f1ef460e770d68792c8f905227800b35552bd28f9aec6162fc03cd63aab9d`.
BIN SHA256: `703ad32325a15e71720392832d842070d35d42bef3bf956c2d02e018d3d2f91e`.

Next: profile the remaining renderer/activation cost, verify natural Town
progression, then continue source effects, Geo and gameplay coverage. The
whole-game inventory has unresolved dependencies and no admitted whole-game
budget; fitting this current subset does not establish full-game residency.

## Previous milestone and investigation history

Everything below records the preceding95% region-streaming build. Its hashes,
allocations and timing figures are historical; the section above describes the
current playable disc and supersedes its next-step streaming proposals.

## Production95% consolidation — built and replayed

The user's95% cutoff is now applied to all eligible same-material static pairs,
including different original sprites. The fresh cook has37qualifying pairs in
4direct-representative groups and15global aliases; the lowest admitted member
score is95.1491%. Every member matches its FINAL representative, with no
transitive threshold chaining. No80% merges were applied. Animation/effect frames
and shaped masks are protected. Exact full-quad strict-black masks also share a
4x4constant tile, with unchanged CLUT/index and complete sampled-word verification.
The larger sourcecanonical proposal was not applied: source identity alone did
not meet the approved95% metric.

Across98packs this removes669texture entries,13pages and471476raw bytes versus
the same fresh cook before merging. These are summed per-region savings, not
471476bytes of simultaneously resident VRAM. Final maxima:5pages,392textures,
244968raw bytes. `host/similarity_dedup.py` verifies geometry/material/frame/clip/
collision preservation and stages every pack before publication. Full proof:
`.hkpsx/similarity-dedup.json`. `tools/build.py` fingerprints the matcher/helpers.

Deterministic MaxRects packs unchanged texels. Four416-CLUT banks occupy4096
previously unused VRAM bytes, with all allocations checked together for overlap.
Texture pages, framebuffer, font, HUD and animation-cache reservations are intact.
The common4x4tile preserves oversized-quad UV-grid repair;1x1was rejected by the
geometry admission check. The final constant GPU proof compares12902400pixels
across tints, fades, rotations and both framebuffers with exact results:
`.hkpsx/lossless-room-fallback/gpu-report.json`.

This build also integrates the prepared original grass/death particles, bouncing
and settling persistent door fragments, deterministic scenery dimensions/cache
keys, fully-soft-black material admission and the corrected black endpoint.
All particle families remain present. Large particle spawn/tick/draw functions
are outlined to resolve the MIPS PC16 branch-range compiler failure; final EXE
hazard scanning passes. These systems remain bounded approximations of Unity.

The earlier80% visual gallery is historical review data. Optional lossless
palette-plane sharing and HKSCNE01 shared scene banks remain host/parser work;
the legacy guest still uploads region banks. Do not infer seamless traversal
from offline global-bank capacity results.

## Current playable build

The sole playable entry is
`/Users/ebonura/Downloads/ps1 games/hk-psx.cue`, referencing `hk-psx.bin` beside it.
Every build replaces that pair. No candidate, telemetry, backup or versioned
disc entries are kept. `.hkpsx/build.json` identifies the current EXE/map/disc.
The user opens PSoXide themselves; validation below used isolated headless runs.

Current normal EXE SHA256:
`a172ed26e52493716a5fcddd47790b71238e4de0fd961f72e1f320fbdcef2d7d`

Current BIN SHA256:
`67cb0c385fadc1ddfc0015c0787a08e2eeda112db7b7a64ac14b539cb42c06e8`

This is an incomplete Tutorial/Dirtmouth port. Existing source-derived systems
include layered/parallax scenery and terrain, Knight movement/nail attacks,
breakable doors and grass, three Crawlers, damage/health/SOUL, recoil/pogo,
hazard checkpoints, Focus, six resident sound effects and cave ambience. The
minimal title uses original assets. Six source visibility controllers and safe
geometry extents address the previously disappearing level. Continuous VBlank
input sampling remains active through CD/decoder/GPU work.

## Latest recording and changes

The6246-sample host profile and5871-poll input tape(start91) are preserved with
hashes in `.hkpsx/user-6246-input/`. Host frame timing is not guest PS1 timing.

- Crawler deaths now play the original Air/Land corpse clips and retain the last
  corpse frame. Dead actors cannot cause damage or award repeated SOUL. Contact
  response is a bounded approximation, not a full Unity/Box2D reproduction.
- Cut grass now shows the original slash-impact clips. Seven authored doors
  activate their four original fragments, with28 persistent source-indexed
  records. Source launch/gravity and approximate rotation are implemented;
  fragments bounce and settle on terrain. Source grass/death particle subsets
  are active with a128-particle pool; full source effects remain incomplete.
  Grass impacts and fragments share a32-packet draw limit.
- Verified premultiplied black-mask materials now darken through PS1 averaging
  instead of ineffective additive black. Opaque cores and transparent texels
  are unchanged. Intermediate alpha is an explicit50% blend approximation;
  mixed-color materials are not silently converted. This is not full shader parity.
- Three tutorial tablets use original text, Perpetua font, source prompt markers
  and exact collider triggers. Up/Down opens; Cross advances/closes; Circle
  closes. Close/attack input is consumed until released. Movement locks while
  reading; damage and scene changes close it. Source dialogue choreography,
  alignment and presentation effects remain incomplete.
- Completed compressed reads are retained in existing RAM arenas for possible
  reuse. Urgent inactive VRAM uploads can be cancelled after GPU completion.
  Vertical motion can prioritize an approaching direct exit. New region entry
  installs its wishlist before pumping. Background reading and decoding overlap;
  only an explicit urgent demand can defer an unrelated new decode.

The source audit decoded all88 Tutorial FSMs. The first recorded read attempt
atx113.80 was slightly outside the first tablet trigger. There is no read trigger
at(166.75,6.39); the nearby Geo Rock is attack-driven and its payouts remain
unsupported. Do not widen source triggers arbitrarily. The other pickup found
in the audit is inactive and elsewhere. Evidence: `.hkpsx/read-point-followup/`.

## Validation and remaining stutters

All404 tests pass (179Python and225Rust/native), recorded in
`.hkpsx/dedup-95-final-tests.log`. The final EXE has zero R3000 hazards.
All98packs pass pinned and incremental LZ4 decoding in five256KiB arenas.
The link leaves44428bytes before the reserved32KiB main stack. Main-stack
high-water and physical-console timing remain unmeasured. See BUDGET.md.

Both the profiled finalEXE and actual mainCUE complete all5871polls, with identical
endpoint(163.153656,6.390625), region8, health4 and two kills. Actual software/
hardware final captures, first-door captures and a97-capture gameplay sequence
were checked; a21-frame contact sheet includes the darkest capture. The level
remains visible. Evidence: `.hkpsx/dedup-95-profile/`, `.hkpsx/dedup-95-cue/`,
`.hkpsx/dedup-95-door-cue/`. The route has zero guest/CD/input/geometry-repair
faults and zero missed input VBlanks. It spawns400particles with zero dropped,
records66debris bounces, and finishes with20settled persistent fragments.

| Long recording | Boundary-wait ticks | Longest wait | Mean frame interval | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Earlier baseline |648|86|3.633VBlanks|7VBlanks|
| Previous main |646|90|3.636VBlanks|7VBlanks|
| Current95% main |478|93|3.663VBlanks|6VBlanks|

Total waiting falls26.0% against the previous main, with82loads and4989sectors
(previous84/5218). The worst boundary wait increases90→93ticks, mean frame
interval is slightly higher and p95 remains5VBlanks. **The strict seamless gate
still fails.** This is a combined art/effects/packing build, not a controlled
single-variable performance test, and matching tape/endpoints do not guarantee
identical trajectories. Do not claim overall rendering performance improved.

The first integrated experiment had438waiting ticks but followed a different
upper route. Subsequent builds exposed a decoder-priority regression; it was
removed after demonstrating40ticks of idle decoder time followed by a21tick
crossing stall. Fresh-wishlist activation alone did not improve aggregate waits,
and its proposed exact eviction mechanism remains inferred. These experiments
are retained under `.hkpsx/user-6246-update1/`, `user-6246-final/`,
`user-6246-activation/` and the associated stall audits. They are not alternate
disc files. Do not compare poll-bound totals as identical-trajectory benchmarks.

A targeted actual-CUE first-door run verifies four active and four drawn pieces;
its before/after capture sequence and both renderers were inspected. The original
actual-guest tablet test opens source12118 at(110.758,31.390625) and Cross closes
it, using controller input without patched RAM. That test used the first
integrated EXE; later controller replays take a different route and do not
activate tablets. Native production GPU tests cover all four source pages in
both framebuffers. Evidence: `.hkpsx/read-points/guest-1/`,
`.hkpsx/read-points/native-gpu-report.json`, and first-door CUE reports.

## Next work and commands

The immediate performance task is to preserve/read the needed rooms earlier
across vertical and brief-return transitions. The latest full trace still has
ordinary traversal holds; do not label the port seamless because input is sampled
continuously. Compare the first divergent transition and exact read/decode/upload
leases before trusting an aggregate score. A separate compressed cache or new
prediction policy needs measured admission and RAM/VRAM evidence.

Remaining gameplay includes Geo drops/rocks, full particles/shaders, other enemy
families, complete scene scripting, NPCs, saving, inventory, abilities, bosses,
music, title options and complete-game progression. The existing earned-Focus
validation tape does not meet its coverage assertion; do not claim newly tested
healing without a successful route. Source physics/effect approximations are
recorded in the ignored destruction/door-debris audits.

Build: `cargo hk-build build`. Tests: `make test`.
Replay with `tools/profile_tape.py --exe dist/hk-psx.exe --map build/hk-psx-normal.map`,
the main CUE, preserved tape, a fresh ignored output and `--require-seamless`.
Actual-CUE helper: `.hkpsx/user-6246-input/replay_main_cue.py --output ...`.
Never launch the user's GUI or create a second playable disc to validate changes.
