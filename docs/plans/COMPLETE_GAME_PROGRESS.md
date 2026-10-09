# Complete-game execution ledger

This ledger tracks implementation of `COMPLETE_GAME_SOL.md`. A package is
validated only when its acceptance criteria have current evidence. Source
extraction, cooking, runtime binding and playable validation remain separate.

## Current authority

- Source branch: `main`; implementation began from commit `8337d68`.
- Windows source: CrossOver Steam installation, Unity `6000.0.61f1`.
- Playable disc: build 76; King's Pass, Dirtmouth and seventeen Forgotten
  Crossroads scenes (01 to 10, 12 to 14, 16, 18, 19, 21) connected through the
  authored gates: 19 scenes, 329 views, 200 disc chunks.
- Disc policy: the user authorized rebuilds on 2026-09-16; every build now
  replays the route tapes in `tools/tapes` and fails on any fault.
- Current packages: P05-P08, admit checked metadata, build the catalog-wide
  source matrix, remove scene-multiplied Geo state, and correct the first
  cross-layer tile-occlusion failure before adding streaming admission. The
  playable disc is still preserved.

## Milestones

| Milestone | Packages | State |
| --- | --- | --- |
| M0: recover import | P00-P03 | validated |
| M1: scalable content | P04-P08 | in progress |
| M2: connected world | P09-P12 | in progress |
| M3: persistent adventure | P13-P18 | in progress |
| M4: full combat roster | P19-P22 | in progress |
| M5: complete content | P23-P27 | pending |
| M6: release proof | P28-P31 | pending |

## Package state

| Package | State | Current evidence / next action |
| --- | --- | --- |
| P00 | validated | First-read documents, Git, doctor, source version, disc policy and live-process state rechecked. No importer/emulator process was active. |
| P01 | validated | All 501 BuildSettings scenes completed with source/code verification and zero scene failures. The latest run isolated three signal-11 workers and retried only those scenes successfully in fresh processes; exact attempt records are retained. |
| P02 | validated | Every scene has an evidence-backed role and remains included. All 1,119 TransitionPoints and 318 scripted loads retain geometry, flags, destination dataflow and progression-condition status with downstream owners. All 95 completion fields join serialized access to exact managed readers/writers; installed cap CIL proves the 112 maximum. Fifteen source reference cases cover every observed edge/condition category. |
| P03 | validated | All reusable observed geometry families are extracted across 501 scenes. 463 scenes are geometry-complete; the other 38 retain only 757 TextMeshPro and four legacy TextMesh runtime glyph-shaping cases as explicit renderer/cooking work. All 106 canonical Tutorial/Town regions pass source-geometry and terrain comparison, including 20 Town rows resolved through per-region provenance without `base_path`. |
| P04 | validated | Source-order-independent IDs cover the complete imported catalog with collision checks and zero unresolved assets. Current sparse Breakable/grass IDs have stable aliases. The bounded central state API separates global, mode and active-scene ownership and passes reset, reload, idempotence and exact-size tests. |
| P05 | in progress | **The per-scene RAM slope is the constraint on finishing the game, and it is now measured rather than guessed.** Admitting one scene cost about 3,232 linked bytes, against 501 scenes in the game. Attributing every byte of `.data` by relinking and walking the pointer graph found the waste was mostly empty slots rather than payload: the lifeblood and geo binding tables held one slot per region and 712 of 723 and 680 of 723 were empty, so sparse lists took 16,816 bytes for no arena cost. The disc's own chunk directory was worse and entirely redundant: `init` already proved every field of it against the generated manifests, so keeping the proven copy took 23,988 bytes now and about 221,000 at 501 scenes. It is 2,132 bytes now and stops growing. A stale coverage reservation gave back 2,728 more. The arena is a ratchet rather than a slope: it pays for scene 0 specifically, which leads the second-largest bank by 16,116 bytes, so it moves only when a new scene beats scene 0 on raw bytes or bank size. What still caps the catalogue: `ActorSpec` is 172 bytes per placement and does not fit a 48-byte bank object, and `ATLASES` is validated before any bank is admitted so it needs its own bootstrap chunk. Earlier builds 79 to 87 moved hazards, checkpoints, grass, breakables, mask fades, remote masks, neighbours, actor references and the region table itself into the loadable bank behind checked views. |
| P06 | in progress | Build 90 cooks and packs the whole Forgotten Crossroads (45 scenes, 693 views) with explicit omissions for the surface audio, atmos channels and actors the guest does not support yet. Previously:  in progress | `tools/world_pack_matrix.py` joins all 501 verified scene artifacts, dependency inventories and current+neighbor windows, with deterministic hashes and source-cost peaks. `tools/cook_scene_pack.py` now cooks any catalog scene into an isolated, write-guarded pack with the canonical cooker/similarity/scene-bank path, and the matrix merges its measured PS1 costs per scene. The whole-catalog batch is complete: 457 of 501 scenes cook with a passing native decoder, 41 have no source envelope (cinematics, menus, boss-defeated and preload helpers) and three exceed the 128-view HKSCNE room limit. Eleven shared abort causes became explicit recorded exceptions or lossless transformations. Shared art canonical mappings across scenes, neighbour-window costs and production admission remain. |
| P07 | in progress | Build 94 admits the Crossroads Climbers (kinematic surface follower on the resident terrain, rotated body and sprite, freeze stun, corpse); build 89 admits the Crossroads Zombie Runners (controller, clips, corpses, audio bank on voices 21 to 23; dust and death effects recorded omissions) and replays the routes concurrently; build 88 streams break-effect art per scene through one disc chunk per scene decoded at admission and uploaded to the fixed effect VRAM rectangles; 210 emitters across 18 scenes with no VRAM omissions. Previously: persistent Geo state is now a flat 256-entry source-state pool keyed by scene and authored rock slot, covering the verified 207 GeoRock instances without a `[scene][32]` allocation. Version-2 host plans preserve all 850 directed gates, and the bounded room-residency runtime can safely evict only completed slots outside a keep-set. Native residency/Geo tests pass and the no-disc link is hazard-free with 16,232 bytes before the stack floor. Predictive CD residency and guest streaming admission are still pending. The break-effect catalogue now separates a port gap from an object the original disables too: infected vines read 0 admitted, 8 refused, 5 inert in the source, where they used to read 0 of 13. The eight refuse on two authored outputs that are structural rather than tuning values, Mecanim blob animation against a project that reads only tk2d, and a runtime GlobalPool.SpawnBlood call with no serialized particle system to read. Nothing cooks vines yet, so an admission today would ship nothing anyway. |
| P13 | in progress | **HKS5: the world persists, and a journey proves it** (section "The world persists" below). A save now carries the port's own PlayerData (boss, ability and spell bits) and a SceneData list of what differs from the authored world (persistent breakables, Geo rocks, the Lifeblood cocoon, one-way secret masks, arena `Activated`), 166 bytes for an untouched world plus 4 per item; HKS4 still loads. Four chained routes take one card from Start Game through King's Pass, the Crossroads and a fair False Knight kill to a reload where the arena stays won. Earlier: the save record became the port's real PlayerData store: HKS1 at 56 bytes became HKS2 at 78 for the charm board and the NPC conversation cursors, then HKS3 at 146 for the script bank's flags. It carries a fixed 16-slot reserve rather than following the cooked field list, because a length that moved with every cook would invalidate every card fixture, and an fnv over the ordered field list, because slot order belongs to the cook and a record written by one bank and read by another would put one flag where another belongs with the checksum none the wiser. `tools/migrate_cards.py` carries the committed fixtures across each growth, including the psx-mc container header that makes a read self-describing. `tests/save_runtime.rs` compiles and runs the record for the first time: its in-file tests had never executed, because no cargo target path-includes a file that pulls in `psx_mc`. Earlier: Build 105 admits the five catalog benches (rest, heal, respawn marker; `town-bench` route). Earlier: shared persistent stores now have a versioned `HKSV` fixed-buffer snapshot codec for global and mode owners, with strict length, ordering, checksum and capacity validation; active-scene flags reset on load. Memory-card I/O and actual quit/reload routes remain pending. |
| P08 | in progress | Tile-coverage event chains now separate back and front owners, preserving front masks through the complete back pass. The first regression and full hk-format coverage suite pass. Material-family completion, black-fill source coverage, dynamic mask invalidation, actor/effect cache expansion and representative visual replay remain. |
| P09 | in progress | Build 104 runs the source entry sequence (fade, lead, forced walk; hidden top drop) for every gate; build 103 applies the source gate rules (side gates need the Knight facing in, no transition while recoiling, delayed top colliders); build 102 adds the source CameraLockArea limits (world bank kind 11) and a damped camera with look-ahead; per-entry behaviour descriptors (forced walk, fades, control delays) remain. Earlier: Crossroads_01 is scene 2 of a single host scene catalog (`host/quality.py SCENE_TABLE`) that every cooker, generated table and the guest scene count derive from. The authored Town `bot1` gate drops the Knight into Crossroads_01 `top1` and the return gates resolve to Town `bot1`; the `town-well` route tape replays on the final CUE with three scene loads and the Knight walking inside Crossroads. Build 75 adds Crossroads_02 and Crossroads_07 through `GRID_SCENE_LAYOUTS` (one catalog entry per scene); their gates are generated from source and the linked headroom is 376,680 bytes. The `crossroads-floor` route walks the Crossroads_01 floor to the right wall; reaching `right1` needs an authored jump route. Build 78 gives top-side gate entries the source's -12 units/s initial drop (`Gate.entry_vy`, verified for the well; assumed for other top entries). The delayed top-gate collider, source camera locks, the Crossroads enemies beyond the Crawler and return replays remain. |
| P19/P20 | in progress | **182 of 236 placements admitted across 47 scenes**, counted by `host/remaining_actors.py`; 29 refused inside a packed region and 27 parked outside every region. The denominator grew when Greenpath's entry scene was admitted, which brought 16 unadmitted placements with it. Runtime spawning landed as a cook-time reservation rather than an allocation, because the source Hatcher never instantiates anything: it parks a fixed cage of babies and Fire picks a random child, so every baby is seated before the scene runs, death recycles a body back to the cage, and the cook refuses any scene where a release could reach the 32-actor cap. That took 5 Hatchers and 45 babies. The Blocker followed for 344 bytes and no wider spec, its trigger ranges kept out of `ActorSpec` as constants the recognizer proves each placement against, because widening the spec would have charged every one of the other 180 placements. It is very nearly melee-immune by design and the intended answer is a spell, which nothing here grants yet. The Zombie Shield landed as its own controller, not a Runner parameterization, on one authored field: its Walker has `pauses = 0`, so it never idles. Crossroads_15 went from admitting nothing to admitting both its placements. Earlier: Runners (with Bargers, Hornheads and Leapers through the parameterized Runner), Climbers, Vengeflies, Gruzzers, Baldurs, Aspid Hunters and the last four ceiling Climbers. A silent correctness bug was closed on the way: `walker_control` resolved an object's FSM ids against the file it was serialized in, so for an additively merged object it read whatever lived at that id, and on an id that happened to land on a `Crawler` FSM it would have admitted an FSM belonging to another object. What is left needs its own controller each, and each has a measured reason: the Zombie Guard spawns paired ground shockwaves the port's gravity-body projectile pool cannot model and physically blocks the hero, which nothing here does; the Mawlek is a five-part rig against a renderer that draws one sprite per actor; the Giant Fly is the one sleeping Gruz Mother, boss-shaped work for a single placement; The Mender Bug stays refused because its own FSM destroys it on the save this port ships. Four Crossroads_10 placements and one hidden Runner are held out on purpose. |
| P14 | in progress | Builds 112 to 121. Step 1's audit corrected the jump velocity and added the jump, ledge and head-bump buffers the port was missing. Step 2 implemented the Mothwing Cloak, the Mantis Claw's wall slide and jump, the Monarch Wings, the Crystal Heart and the Shade Cloak, all from source constants and the Superdash FSM. Step 3 catalogued every PlayerData traversal flag and found acid already covered by the generic hazard path, `hasAcidArmour` and `hasLantern` unreachable, and one unimplemented bounce shroom owed to P11. Step 4 took the input priority from the Can* methods. Step 5 added the Dream Nail with its SOUL reward. Step 6 mapped L1, R1 and Triangle and extended the replay schema and the native probe. Step 7's combinations are hk-sim tests. The art landed afterwards and this row was stale: all 16 clips are cooked through the Hollow Shade route, 97 frames in 36,190 linked bytes reaching VRAM through the shared 64x64 animation slots, so Dash, Wall Slide, Walljump, Double Jump and the four Dream Nail clips play their own art rather than Fall or Idle. Remaining: no ability has an acquisition route (step 8), and no route on the disc exercises any ability, so every one of them is verified in hk-sim and the native probe only. |
| P15 | in progress | Build 122 adds Vengeful Spirit from `Spell Control`, `Fireball Cast` and the projectile's own FSMs, with the source's one-button cast-versus-Focus split. Two of those remainders are now done and this row was stale: step 4's centralized damage resolution is `apply_hurt`/`respond_to_hurt` in game/src/main.rs, and the `Ball`/`Ball End` art is cooked through the Hollow Shade route rather than needing a reserved VRAM rectangle. Remaining: Desolate Dive and Howling Wraiths (catalogued only, and both gated on progression no admitted scene grants), nail arts, nail damage tiers, and the verification matrix of step 7. |
| P10 | **frozen, by measurement** | The script runtime is as wide as it is worth making, and the row below is the evidence rather than a plan. 256 of 1,821 FSM instances compile and 9,257 of 41,603 actions are supported. Four of the cheapest remaining widenings landed their predicted action gains to the unit and compiled **zero** further FSMs, because an FSM needs every one of its actions before any of it runs. The blocker this row named for weeks was stale: `FindChild` never needed a runtime object model and already compiles 881 of 1,018 at cook time, next to `GetOwner` at 847 of 847, because the cook-time object model is built and is why they compile. Asking "can the port answer what this action names" of each remaining unit collapses a naive 245 unblockable instances to about 8: `ActivateAllChildren` looks like 67 and is 1, and the 23 cross-FSM sends are 0, because all 46 sibling FSMs they send to are themselves blocked, so an instance table would unblock senders whose events land on receivers that are not in the bank. Everything past that needs mutable per-object guest state (a transform, a renderer, a collider, an animator, a body) in a guest with no heap. **P12, P17, P18 and P23 no longer wait on this and should be unblocked natively.** What does run on the disc: Town's `Area Resetter` takes TOUCH from its well-mouth trigger and writes `currentArea`, observed on two routes, from 10 cooked instances and 27 ops in 2,565 linked bytes. Two real bugs closed on the way, including a `SendEventByName` that silently dropped a non-zero delay on six instances and compiled them to a send on the wrong frame. |
| P11 | in progress | Steps 1 and 4 done for the admitted world. `tools/physics_catalog.py` shows terrain is boxes, polygons and edge colliders only, all exactly representable; no one-way platforms exist in the admitted world; and `LiftPlatform` is a 0.09-unit cosmetic dip rather than a moving platform, so step 3 has nothing to implement here. The `BounceShroom` in Crossroads_38 is implemented as the down slash target it actually is, with its `fatGrubKing` gate recorded rather than evaluated. Owed: 161 pogo targets the cooker refuses, against 282 it cooks. Re-measured at build 155, because the number this row used to carry counted the wrong population: it read "127 of 166 TinkEffect surfaces the static pogo pool does not cover", and `NailSlash.OnTriggerEnter2D` never reads `TinkEffect` at all. It reads the collider's layer and then `NonBouncer`, `BigBouncer` and `BounceShroom` on that same object; `TinkEffect` is the spark, sound and camera shake, on its own 0.25 s throttle, and never touches hero velocity. The 161 refusals are all modelling limits and none of them is capacity: 128 because a PlayMakerFSM owns the object and the cooker cannot tell a cosmetic FSM from one that moves or removes it, 18 for a moving Rigidbody2D, and 15 `InfectedBurstLarge` blobs in Crossroads_22 whose only collider is a circle, which has no exact segment form. The per-scene cooker bound is 128 targets and the busiest scene uses 34, so nothing is queued behind a pool. |
| P16 | in progress | Steps 1 to 4 have a real slice. `host/charms.py` cooks all 40 charms with the equip contract asserted out of the `UI Charms` FSM rather than assumed: a full board never overcharms and never counts an attempt, and overcharming is unlocked the way the source unlocks it, by four refused attempts whose notches are handed back and a fifth that goes through and sets `canOvercharm`. The board is a pause row, the collection and notch budget ride the save record, and six effects are implemented from real numbers (Grubsong, Stalwart Shell, Soul Catcher, Soul Eater, Fragile Heart, Fragile Strength). The other 34 cook as no effect and the board refuses to equip them, each with the missing port system named. All 40 icons are on the board at 16x16, through the Hollow Shade route: 5,248 linked bytes and four resident CLUT rows out of the fourteen the Shade reserved at x320, y482, which is outside the per-view CLUT banks and so costs no texture slot in any view. Six visible rows beside the four the frozen view reserves is 10 of the 24 animation slots. The per-view figure was never the budget to price this against, and the atlas route stays refused for the reason that did not move: art resident in every view needs resident texels too, and 16-pixel icons want 2,560 VRAM halfwords against the 304 `residency::SPARE_HALFWORDS` leaves unclaimed, in fragments no wider than sixteen. The screen picked the size rather than either budget, because six rows of 24-pixel icons plus the title, notch counter, six-line description and footer need 281 of the screen's 240 scanlines; `host/charms.py::panel_layout` derives that and the cook refuses a size it cannot seat instead of dropping a row. Previously: step 1 done. `host/items.py` catalogs 40 charms with notch costs and PlayerData fields, 26 equipment entries, and the fragment fusion rules from the prefabs the pickups spawn. The slice contains 6 charms, 1 notch, 3 mask shards, 1 vessel fragment, 3 relics and the City Crest, none of it implemented. Charm effects belong to steps 5 and 6 and are not read. |
| P17 | in progress | Steps 1 to 3 have a real slice. `game/src/shop.rs` runs the whole purchase: `BuildItemList`'s listing predicate decoded from the IL branch targets, `CanBuy`, the base and alternate list swap, the Defender's Crest discount, `Confirm Control`'s fixed order and Sly's six delivery branches. A charm purchase writes the same `gotCharm` bit the charm board owns, and the runtime harness compiles the real shop against the real charm module so the two halves test against each other. Prices come from the decrypted sheet, and the stale count was wrong: 11 of 14, not 9. The economy is 1,167 Geo earned per clear, not 1,177, against 230 Geo of sinks; Sly's full alternate stock alone is 9,260. **The Knight is in Sly's shop on the disc.** Everything after this sentence used to say he could not be, and every clause of it is now out of date; it is rewritten rather than appended to, because it was still briefing work that had already landed. `mod shop;` is in the guest (`game/src/main.rs`), Room_shop is admitted as scene 45, and the `town-shop` route resumes in Dirtmouth, walks into `door_sly`, presses UP, crosses the room and opens the shelf: `HK_SCENE_GATE_LOADS` 1 into region 694, `HK_SHOP_OPENED` 1 with `HK_SHOP_CLOSED` 1 and `HK_SHOP_OPEN` 0, which is a shelf that went up and came back down rather than a tape that ran out with it still showing. **A door's destination is not in the component the cooker reads.** A `TransitionPoint` carrying a `Door Control` FSM departs through `BeginSceneTransition` and the FSM's own string variables, so `door_sly` serializes an empty pair. `regions.door_destination` reads that FSM, after two checks that make it evidence rather than a guess: the action's inline `sceneName` is a stale editor default (`Room_temple` on every door here), and no enabled action in any admitted scene writes `New Scene` or `Entry Gate`. Across the 46 scenes, 16 TransitionPoints carry a readable `Door Control`: 12 shipped an empty pair and are filled from the FSM, 3 already agreed and are untouched, and `Crossroads_01/door1` has its two fields transposed, which is recorded as `disagrees_with_serialized` and left alone because the resolver fills an empty pair and never overrides one. Only 2 of the 12 name a scene inside the slice, so only 2 cook a gate: `Town/door_sly` into Room_shop and `Crossroads_06/door1` into the Shaman Temple, which had been admitted and unreachable the whole time. `door_station` and `door_bretta` resolve cleanly to `Room_Town_Stag_Station/left1` and `Room_Bretta/right1` and cook nothing, because neither scene is admitted; Town's `door_dreamReturn` is not this shape at all and never was, since it carries no `Door Control` (only `Set Compass Point`), so no FSM can supply it a destination. **Admitting the scene was not the last blocker.** `world::gate` point-tested the Knight's origin, which sits 1.39 units above his feet, against a door collider that is a strip of floor 0.23 units tall, so no door in the game could ever have fired. Doors now cook as side 5 and the guest answers them with the hero body plus a fresh UP press, which is what `Door Control` itself does. Room_shop's own measurement, re-read from the cooked packs: 6 views, chunks 694 to 699, tightest view 87 of 416 CLUT slots, not the 90 this row used to claim, which is the view's texture count against a separate 640 limit. What is still owed (two clauses of this list were stale by HKS4 and are corrected here: a purchase does survive a quit, through `Save::shop_slots` and `shop_counters`, and a fused mask shard does apply, because `shop::vitals` sets `max_health` from the shop's mask total under the charms): `HK_SHOP_PURCHASES` is 0 on the route because the card carries 6 Geo against a 60-Geo cheapest item, so proving a purchase needs a route that earns Geo first; and Sly is not drawn. |
| P18 | in progress | Step 1 done. `host/npcs.py` catalogs 27 NPCs, their gates and 154 decrypted dialogue keys. On a fresh save only Elderbug and the Jiji door are talkable in Dirtmouth; the other eight are gated on progression the port cannot reach. Dialogue selection is a priority list, verified from PlayMaker.dll and now pinned by a test in the script executor. Steps 1 and 2 have their first instance: Elderbug stands in Dirtmouth and holds his whole fresh-save conversation. His talk range is npc_control's own trigger cooked as world-bank kind 14, his six pages are Conversation Control's entries, his facing comes from the `Hero Is Right`/`Hero Is Left` compare against his own `localScale.x`, and his three clips ride Town's room bank for 18 of its 212 spare texture slots. Verified on the disc, not only in the cook: the `town-elderbug` route walks into his trigger, opens with UP and pages through all six entries with X, and a captured frame shows him drawn in front of the bench. Both of the first two owed items are now closed. Conversation selection is a chain rather than a branch, because the state that speaks also writes the flag that changes the next pick: Elderbug runs intro, then history, then a generic line that writes nothing and repeats. The met flag is a two-bit cursor per NPC in the save record, advancing on the page carrying the source's own write. Myla joins him in Crossroads_45 at 223 of 416 in the view she stands in. 27 NPCs surveyed, 12 present on a fresh save, 5 talkable, and every refusal has a reason: the Stag speaks nothing on a fresh save and its idle frames are 91x87 against the 64x64 cache, and Jiji Door, Goam Inspect, both Tram Call Boxes, Cornifer Card and Gravedigger carry no animator at all. Owed: four scenes (Tutorial_01, Crossroads_03, Crossroads_01, Crossroads_13) have too little texture headroom to hold an NPC, and Myla has no route that reaches her. |
| P21 | in progress | **The False Knight can be killed, and can kill.** The `boss-fight` route takes it through all three phases to the death event, and `boss-death` proves the other half: a mortal Knight who walks into the trigger and presses nothing is dead in 1,002 polls on five hits, leaves a Shade in the arena, respawns at the seat with five masks, resets the arena to Waiting, restores the boss to 65 and 40, and can walk back in for a second go. The two blockers this row used to name are gone: `BossLoader` is answered by cooking the additive scene into its host under `Scene.sid`, and the animation cache went from 8 slots to 24, its upload budget from 16,384 bytes to 49,152 and its per-frame tile cap from 4 to 20, with four slots reserved every frame. The arena seals on build 154: `host/cook.py` bakes all fifteen `BG Control` gates now instead of the three the source loads shut, 41 region bindings on the disc, and the runtime lifts the open ones. **Two things this row said that were not true.** The kill ran with the Invincibility cheat on (`HK_CHEATS` 1, bit 0 is `invincible`, set by the tape's own pause-menu polls), so "the Knight never loses a mask" was the cheat and not the fight; strip those seven events and the same tape dies without landing a stagger. And the room below and right of the arena was a guaranteed hang for anyone who walked into it, because it cooks 129 terrain edges against a 128 cap, which disabled its edge table entirely and turned every terrain query into a bank rescan until the pad queue overflowed; fixed, and the only region of 693 that was over. **The fight is won fairly**: `boss-fight` now kills the boss with `HK_CHEATS` 0 and `HK_HEALTH` 5 at every poll, three staggers, three conversions and all 24 barrels flung and broken, in 239 events. It exists because the arena seals: the 0.69-unit recoil on every landed hit can no longer walk the Knight off the floor, which is what stranded the previous tape at 10 hp. What is still owed: the staff has no bounce model, the Death Head is refused on room bytes (26,198 of 39,676) and the floor break on geometry, because removing `Break Floor` leaves no layer-8 surface. 26 hk-sim tests cover the phase machine underneath all of it. |
| P12, P22-P31 | pending | Follow dependency order in the master plan. |

## P01 evidence

### Finding

The previous run completed 249 source scenes. When the parent process received
the requested termination, its `ProcessPoolExecutor` invalidated every submitted
future. The parent then wrote the same generic pool exception as 196 individual
scene failures. This does not identify `Mines_02` or any later scene as corrupt.

### Implemented supervision

- Submit at most `--jobs` isolated one-scene processes at a time.
- Record each worker PID, attempt, start/end time, exit code and signal.
- Bind result files to the exact worker attempt to reject stale output.
- Retry only the isolated timed-out/crashed scene.
- Preserve prior attempt records across resume.
- On SIGINT/SIGTERM, stop owned children, retain completed checkpoints and leave
  active/queued scenes pending instead of recording scene failures.

### Checks and complete-run evidence

- Targeted world import/geometry unit suite: 23 tests passed after the final
  attempt-number and child-termination corrections.
- Three-scene diagnostic with two workers: 3/3 processed, zero worker failures.
- Twenty-scene interruption diagnostic after correction: 12 completed, two
  active workers marked interrupted, zero scene failures and the correct total
  of eight unfinished scenes. The parent exited with the deliberate status 130.
- Latest complete import: 501/501 processed, 463 imported, 38 partial, zero failed;
  source inputs and importer code remained unchanged through final verification.
- Supervision recorded 504 attempts. The first attempts for `Fungus2_14`,
  `Fungus2_15` and `Fungus2_34` ended on signal 11; only those scenes were
  retried and all three completed in fresh workers.
- Extracted source inventory: 414,255 SpriteRenderers, 12,745 tk2d sprites,
  58,852 colliders, 124,947 terrain edges, 18,011 meshes, 3,928 tilemap fills,
  1,696 camera locks and 1,119 transition points.
- Room graph: 914 edges map to an exact destination gate; 196 have an empty
  serialized target, seven name an unresolved scene and two map to a scene but
  not a destination gate. Runtime/PlayMaker overrides remain P02 work.
- Verified dependency inventory remains complete: 1,409 source hashes, 2,615
  textures, 21,741 sprites and 313 animation libraries.
- Targeted importer, geometry and coverage tests: 28 passed.

## P02-P03 current evidence

- `tools/world_transitions.py` parsed all 307 `BeginSceneTransition` and 11
  `LoadLevel` actions with zero parse errors. Literal and variable-bound fields
  remain distinct; declared initial values are candidates, not runtime claims.
- Dynamic destination dataflow accounts for all 286 actions: 130 read
  PlayerData/static/constants, 89 retain serialized defaults, three have exact
  same-object `SetFsmString` branch overrides, and 64 are runtime/API injected.
  The last group is 63 Godhome boss destinations plus one finale continuation
  and is assigned to P09/P22 instead of receiving invented scene names.
- Dynamic entry-gate dataflow records 63 unresolved runtime values, 63 storage
  reads and 87 serialized defaults. Eighty-seven source/default combinations
  produce 91 exact gate matches; two additional Tram defaults remain explicit
  alongside their serialized branch overrides.
- `tools/world_catalog.py` classifies all 501 source scenes with evidence: 402
  gameplay, 63 boss arenas, 13 dream variants, eight cinematics, seven
  transition/bootstrap helpers, three additive boss-defeated variants, two
  credits scenes and one each for menu, completion and shared Knight content.
  No source scene is excluded or left without a primary role.
- All 1,119 TransitionPoints have resolved trigger geometry and retain exact
  serialized door, direction, delay, fade, hazard, offset, snapshot and initial
  activation fields. The catalog covers all 468 component types with owning
  packages and 17,419 serialized PlayerData actions across 815 resolved keys;
  1,328 runtime-variable key actions remain explicit under P13/P14.
- `tools/completion_catalog.py` extracts and hashes the installed
  `PlayerData.CountGameCompletion` and `CountCharms` CIL: 44 boolean/group rules,
  six integer rules, four nested Godhome rules, the exact 40-charm count and the
  33/66/99 SOUL-vessel cases. `SetupNewPlayerData` proves caps 9 and 99;
  `AddGGPlayerDataOverrides` proves three spell caps of 2, four nail upgrades and
  exact cap copying. The installed completion maximum is therefore 112.
- `tools/managed_field_usage.py` indexes all 95 contributing fields across the
  installed Assembly-CSharp CIL. Thirty-three methods contain 426 direct field
  operands: every field has a managed writer, 91 have readers and four nested
  structures have address users. Direct managed callers and serialized
  PlayMaker producer/consumer counts are retained with assembly/method hashes.
- `tools/world_progression.py` attaches progression evidence to all 318 scripted
  loads: 18 have same-state conditions, 176 have broader FSM-reachable
  candidates and 124 have no serialized PlayerData condition. It preserves 21
  same-state postconditions, same-object activation candidates for 204 of 1,119
  TransitionPoints, 25 one-way serialized candidates and 15 exact source
  reference cases. Every unresolved condition is assigned to P09/P13/P14.
- The full Python suite passes 389 tests after the completed P02 catalogs.
- Exact affine boundaries cover all 5,274 CircleCollider2D instances across 292
  scenes. These are dynamic/trigger objects rather than static layer-8 terrain.
- All 12,729 runtime-generated tk2d MeshFilters now reference their extracted
  canonical tk2d geometry rather than appearing as duplicate missing meshes.
- The source resolver retains serialized external paths and maps Unity's shipped
  `Library/unity default resources` pseudo-path to the Windows `Resources`
  file. All 477 Plane and 100 Quad instances now use their exact source meshes.
- All 28,814 ParticleSystem and 28,814 ParticleSystemRenderer payloads are
  retained with transforms, modules, renderer settings, material/mesh
  dependencies and event/sub-emitter references.
- All 831 TrailRenderers retain their serialized curves, timing, materials,
  sorting and emission flags. Sixteen null tk2d collection references resolve
  unambiguously through their animator libraries, including Broken Vessel/Lost
  Kin effects. TextMeshPro submeshes bind to generators through ancestry and
  the four legacy TextMesh renderers are classified explicitly.
- The current verified import completed 501/501 scenes: 463 imported, 38 partial,
  zero failed. It contains 4,505 world meshes, 12,745 tk2d generated-mesh aliases
  and zero extraction errors.
- `tools/compare_world_regions.py` verifies final pack sizes/hashes through
  `.hkpsx/regions-provenance.json`, resolves all 86 Tutorial base-pack rows and
  all 20 Town provenance-only rows, and compares 22,520 cooked draw occurrences
  plus 3,425 terrain-edge occurrences. All source geometry matches exactly after
  applying the cooker's documented near-axis terrain snap.
- Remaining explicit cases are 757 TextMeshPro and four legacy TextMesh runtime
  glyph meshes in 38 scenes. They belong to text shaping/rendering and remain
  visible in the coverage ledger rather than masquerading as missing scenery.
- Representative checks cover TrailRenderer-heavy `Room_Fungus_Shaman`, all four
  animator-resolved Broken Vessel/Lost Kin effect scenes, Tutorial/Town sprite,
  tilemap and terrain cooks, and legacy/TMP text owners. The full Python suite
  passes 370 tests after the final import and comparison changes.

## P04 evidence

- `tools/world_identity.py` builds a disk-backed collision-checked registry for
  501 scenes, 1,997,203 components, 1,119 gates, 815 PlayerData keys and 7,004
  assets. All source references resolve. Scene/component IDs use source scene
  paths and PathIDs instead of `levelN`/BuildSettings order; external art IDs are
  explicitly source-version scoped.
- The first relocation retains 37 current Breakable `scene×128` aliases and 39
  grass `scene×1024` aliases. Duplicate region copies cannot claim different
  owners. Spawned-instance identity requires prefab, authored owner and stable
  slot IDs.
- `shared/hk-sim/src/persistent.rs` provides sorted fixed-capacity global/mode
  stores, checked central mutations, idempotent rewards and a bounded active
  scene owner. Reset fixtures cover scene leave/return, bench, death, quit/load,
  dream return, challenge/mode reset and snapshot reload.
- Exact current-source capacities cost 84,536 bytes for all 5,283 global keys,
  8,200 bytes for 512 mode keys and 1,176 bytes for 1,024 active flags plus 64
  timers: 93,912 bytes total, with no per-scene multiplication.
- The complete Python suite passes 393 tests and the complete hk-sim suite passes
  228 tests after P04. The disc remains build 71; this host/state package did not
  recook or replace it.

## P05 first slice evidence

- `tools/world_metadata.py` emits versioned `HKWMTA01` banks from the canonical
  region catalogue without rewriting `data/regions.rs` or the disc. The source
  catalogue hash is recorded in the ignored metadata report.
- `shared/hk-format/src/world_meta.rs` provides checked little-endian views with
  fixed strides, checked arithmetic, zero-padding checks, global neighbour IDs,
  stable state IDs, bounded object/polygon spans and allocation-free iterators.
  It rejects wrong magic/stride, trailing bytes, out-of-range spans and invalid
  polygon point references before publishing a view.
- Current measured banks are Tutorial: 86 regions, 452 objects, 112 polygons,
  1,026 points, 32,072 bytes; Town: 20 regions, five objects, five polygons,
  118 points, 2,844 bytes. `check_world_meta` parses both generated banks.
- The guest `State` edge cache now uses an explicit world generation and global
  region ID and borrowed room-view key. Scene admission, reset and retry revoke
  it before any arena reuse; the view key disambiguates equal-sized room
  payloads while generation protects the reusable arena.
- `host/pack_scenes.py` now emits the metadata banks into the ignored pack
  report, assigns them after scene coverage, and binds raw/stored checksums plus
  the source fingerprint into `WORLD_META_MANIFEST`. `game/src/disc.rs` stages
  those banks in a dedicated bounded buffer, validates the wire view and checks
  every global region owner before publishing the scene. The build 71 link
  report remains bound to the unchanged playable EXE/BIN/CUE; no disc
  replacement is claimed by this slice.

### Next concrete action

Run the pinned guest build with the new metadata chunks and compare Tutorial/Town
metadata against geometry and coverage traces before replacing the playable
disc. Then move the same admission contract to Geo, Lifeblood, dialogue, Great
Door, render-budget and disc mapping tables. In parallel, use the catalog-wide
pack matrix to parameterize production cooking and identify the first scenes
whose exact cropped/paletted PS1 costs can be admitted. Runtime text shaping
remains with the renderer/UI packages.

## P06 first cooking slice evidence

- `tools/cook_scene_pack.py` replaces the ad hoc `.hkpsx/town67` and
  `.hkpsx/crossroads70` probe scripts with a tracked tool. It reuses `cook(...,
  write_shared=False, stage_for_similarity=True)`, `postpack_checkpoints`,
  `postpack_masks`, `bind_regions`, `postpack_similarity(root=...)` and
  `pack_scenes(residency='scene_gate', generate_world=False)`, assigns grass
  state indices exactly like the canonical cook, and writes only under
  `.hkpsx/scene-packs/<scene>/`. An audit-hook write guard aborts on any other
  path; canonical `data/regions.json`, `data/room.hk`, `.hkpsx/packed-scenes.json`
  and `.hkpsx/regions-provenance.json` kept their previous timestamps.
- Envelope rule: active tk2d tilemaps plus gate triggers and gate spawn points,
  padded 1/5/1/1 world units, camera bounds equal to the tilemap; camera-lock
  trigger volumes are excluded after the first Crossroads run cooked a wasted
  row of empty views below the map. Views use the canonical 24x16 stepping and
  halve on the cooker's budget errors.
- Crossroads_01 (`level37`): 20 views, 103,960 resident bytes, 63,161 stored,
  7 pages, 501 palettes, 507 textures, native scene-gate decoder PASS, geometry
  packet bound PASS, 10 breakables, 27 grass, one unsupported reveal controller
  (Secret Mask `level37:4778`), six inventoried actors of which only Crawler is
  movement-supported, 39.7 seconds. The earlier hand-laid 12-view probe measured
  98,808 resident bytes with the same atlas; the difference is view bookkeeping.
- `tools/world_pack_matrix.py --scene-packs .hkpsx/scene-packs` fills
  `ps1_ram_bytes`/`ps1_vram_bytes` only for `cooked` packs and labels
  `over_budget`/`failed`/`no_envelope` scenes explicitly; `pack_admission`
  counts every label. Tests: 408 Python tests pass including seven new
  envelope/layout/split/merge cases.
- Commands: `.venv/bin/python tools/cook_scene_pack.py Crossroads_01`,
  `.venv/bin/python tools/cook_scene_pack.py --all`, then
  `.venv/bin/python tools/world_pack_matrix.py`.
- Not claimed: playable rooms, actor banks, audio, effects, camera behavior or
  neighbour-window residency. The whole-catalog batch was started after this
  slice; its results are recorded when complete.

## P06 whole-catalog cooking evidence

- Three `tools/cook_scene_pack.py --all` passes with four `--shard` processes
  cooked every catalog scene: 457 cooked, 41 `no_envelope`, three over the
  128-view HKSCNE room limit (Abyss_21 136 views, White_Palace_13 135,
  White_Palace_20 156). 11,538 views were cooked in 336 CPU minutes; 258 views
  were halved on cook-time budgets and no scene needed a pack-time recook after
  the similarity guards landed.
- Measured isolated resident costs: median 91,076 bytes, maximum 316,892
  (Tutorial_01, 18 pages, 623,968 VRAM bytes for pages plus palettes). No scene
  exceeds the current 354,032-byte scene arena on its own; two scenes need 16
  or more of the 20 pages and Cliffs_01 needs 1,084 of the 1,248 palettes. These
  exclude actor banks, audio, effects and neighbour windows.
- Recorded exceptions: 1,697 unsupported cook records across the measured
  scenes and 2,000 actor records inventoried, with only Crawler and Runner
  families movement-supported. `.hkpsx/world-import/pack-matrix.{json,md}`
  carries the per-scene costs, `pack_admission` counts and
  `pack_failure_causes`.
- Shared causes fixed during the batch, each with a regression test and a
  source-only commit: unsupported GrassCut topology, soft-edged reveal masks,
  near-camera or over-stretched sprites, actor frames above the 64x64
  animation cache, markerless or one-shot hazard checkpoints and unreadable
  GameObjects are recorded instead of aborting; hazard polygons above 16
  vertices are tiled into exact pieces (`host/polygons.py`); the constant 4x4
  collapse and approximate replacements keep every draw's packet bound and
  every region's packet reservation; two tilemaps may share a render root;
  views exceeding runtime page/texture/byte/packet/actor budgets are halved.
- Remaining structural ceilings for P05/P07: the 128-room scene header limit
  for the three tallest scenes, and the 20-page/1,248-palette caps once
  neighbour windows and actor banks are added.

## P09 first slice and disc validation evidence (builds 72 to 74)

- `cargo run --release -- build` (root `hk-psx-build`) replaced `tools/build.py`;
  it cooks with caches, builds the guest, replaces the sole pair, writes the
  report and replays `tools/tapes/{focus,gate-return,town-reset,town-resident,
  town-well}.pxtape`, failing on faults or incomplete tapes.
- Build 72 (first disc since 71) failed three ways before passing: WorldMeta
  neighbour ids compared 1-based to 0-based indices; the bank staged in the
  scene-arena tail was overwritten by the coverage/atlas/scene in-place
  decodes, which relocate compressed bytes to the end of the arena they are
  given; `world_metadata()` re-ran the checked parse per spatial lookup and
  overflowed the 16-slot input queue (fault 4). Fixes: compare `expected+1`,
  admit the bank last and decode it inside the tail, extend
  `check_scene_gate_load` with `--meta`, and return the admission-validated view.
- Build 74 adds Crossroads_01 (20 views, chunk ids 107..126, 104,652 resident
  bytes, 7 pages, 501 palettes, 6,480-byte metadata bank). The link first
  overflowed RAM by 34,800 bytes because baked particle tracks grew from
  399,432 to 448,800 bytes with ten new emitters; with `BAKE_TRACKS=False`
  every emitter uses the scalar path the tracks were proven against and the
  link has 401,256 bytes before the stack floor. The stale linked-table budget
  in `host/opaque_tiles.py` now scales with the scene count because the guest
  streams per-scene certificate bundles instead. Ambience for Crossroads
  uses the King's Pass channel set (mask 57) read from its SceneManager.
- Replays on build 74: focus 850 polls; gate return (3 scene loads, 2 gates);
  Town reset; Town resident (x 174.6); Town well (13,074 polls, 3 scene loads,
  2 gate loads, final region 110 at x 90.6, y 5.4). All report zero faults and
  zero missed VBlanks. `.hkpsx/validate/*/` holds the captures.
- Not claimed: original well entry velocity or delayed collider, source camera
  locks, Runner/Climber/Mender behaviour, Crossroads audio cues beyond ambience,
  performance parity in Crossroads.
- Build 75 (five scenes, 154 views) surfaced and fixed: the music report cache
  ignoring the scene catalog, break-effect emitters outside the bounded particle
  model (now recorded per breakable), the Geo art quantization sheet exceeding
  256 rows because rock prefabs repeat across scenes (distinct images are now
  quantized once), and Geo rocks whose art exceeds the 32-pixel cell (recorded
  as `unsupported_rocks`). Six routes pass; headroom 376,680 bytes.
- Build 76 (19 scenes, 329 views) needed: the region sanity bound raised from
  256 to 1024 (no guest table depends on it), the flat-floor catalogue widened
  to eight heights (a guest slice), music regions without a snapshot tolerated,
  break-effect art admitted per scene in catalog order inside its 7,680-byte
  VRAM reservation (scenes 5 and later have their 107 emitter parts recorded as
  omissions until effect art streams per scene), and the scene arena derived
  from the pack (largest resident scene plus largest metadata bank, sector and
  LZ4 in-place margins, now 354,068 bytes) instead of a fixed constant that the
  wider similarity pass overflowed by 36 bytes. Per-region cook reuse
  (`data/regions/region-NNN/cook-cache.json`, keyed by spec, provenance inputs
  and the shared `host/cook_inputs.txt` code list) makes catalog-only growth
  cost only the new views. Six routes pass; linked headroom is 219,292 bytes,
  about 1 KB of static region payload per view, which makes the P05 migration of
  region payloads into the loadable bank the next structural requirement.

## P05 payload migration: measured order for the next executor

Static region payload measured on build 76 (329 views): `.data` grew 142,528
bytes for the 175 Crossroads views, about 815 bytes per view. Text share of the
generated `data/regions.rs` fields, as a proxy for their binary weight:

| Field | Share | Why it is large | Guest consumers |
| --- | ---: | --- | --- |
| `actors` | 188 KB | the scene actor bank is appended to every region pack, so each supported actor's `ActorSpec` is emitted once per region with region-specific clip indices (9 distinct actors, 330 emitted specs) | `enemies::sync_region` |
| `breakables` | 69 KB | per-region props with edge lists and hit polygons; 297 emitted for 187 distinct | `world::State::apply`, strike/fade paths |
| `neighbours` | 61 KB text, small binary | index lists; already present in the `HKWMTA01` bank | region activation |
| `hazards` | 15 KB | polygons already present in the bank as records | `world::hazard_contact` |
| `reveal_bindings` | 12 KB | per-region draw bindings | reveal masks |
| `checkpoints` | 6 KB | already present in the bank | `world::checkpoint_record` |

Step (1) is done in build 77: the appended scene actor bank pads each region's
clip table to one scene-wide clip index, so `data/regions.rs` now holds nine
`static ACTOR_S*` specs referenced from 329 regions instead of 330 inline
copies. Measured effect on linked RAM: none (`.data` 497,984 vs 498,160 bytes,
headroom 218,628 vs 219,292), so the text share above overstated the binary
weight of actors; the per-view growth sits in the remaining nested arrays
(breakables with edge lists and polygons, grass patches, hazards, `Region`
itself at 196 bytes). Link-map attribution on build 77: `hk_psx::world::REGIONS`
is 64,484 bytes (329 x 196) and the anonymous constant sections holding the
nested per-region arrays total 306,265 bytes across 1,714 sections, about
930 bytes per view, so the nested arrays, not the `Region` records or the
actor specs, are what the bank must absorb.

Order: (1) actor specs shared per scene (done), then move them into the bank;
(2) hazards and checkpoints (done in build 79); (3) breakables, their mask
fades and remote masks (done in build 80); (4) grass patches (done in build
81); (5) neighbours (done in build 82); (6) actors and reveal bindings (done
in build 83); (7) the `Region` scalars themselves (done in build 84). Each
step must keep the route replays green and record the new headroom.

Build 84 removed `world::REGIONS`. `Region` is now a `Copy` value that
`world::resident(slot)` builds from the admitted bank (identity, bounds,
collision bounds, camera) plus per-scene statics (`SCENE_EMITTERS`,
`SCENE_REVEAL_MASKS`) and three small variant catalogues (`DEBRIS_CATALOG`,
`PARTICLE_BANKS`, `IMPACT_CATALOG`) indexed by the region's
`KIND_REGION_STATICS` bank object, which `host/world.py` annotates on the
region rows before the bank is cooked. Gates carry `target_region` and the
Great Door entries carry `region`, both located by the cookers, so the guest
no longer needs any cross-scene bounds table; spatial region changes use
`disc::Cache::locate`. The only per-view table left in the world module is
`REGION_SCENES` (one byte per slot); `REGION_SCENE_LOCAL` in the scene
manifest (eight bytes per slot), `SCENERY_PACKET_BUDGETS` and the Great Door
`REGIONS` bindings are the remaining per-slot statics for P05 item 8.

Two scene-identity faults surfaced on the way. The disc module works in
manifest scene indices (allocation order) while `Region.scene` and the bank
carry catalogue scene ids; Tutorial is 0 in both, which had hidden that
`Cache::locate` was compared against the wrong kind by every caller, so
checkpoint spawns and enemy cross-region terrain never resolved outside the
first scene, and a first build 84 candidate rejected the Town bank at
admission. `Cache::locate` now takes catalogue scene ids and checks them
against the admitted bank, and the admission differential relies on the
existing owner check. Measured against build 83: `.data` 430,048 -> 386,832
bytes (-43,216), `.text` 417,820 -> 422,120, scene arena 365,140 -> 369,268
(one statics object per region in the largest bank), linked headroom 259,416
-> 294,200 bytes; seven routes pass. The guest's permanent memory no longer
grows with per-view metadata except for the tables named above.

Build 85 narrowed `REGION_SCENE_LOCAL` to `(u16, u8)` (manifest scene index,
room index inside the scene bank) with the packer bounding both; `.data`
386,832 -> 384,784, headroom 294,200 -> 296,248 bytes; seven routes pass.

Build 86 made the transient state tables resident-scene-local: `fade_left`,
`fade_active` and the grass cut bits are indexed by the state within the
resident scene (every scene leave resets its scene and exactly one scene is
resident), while `broken` keeps every scene's session-persistent bit. At 501
scenes this removes about 190 KB of main-frame tables (`fade_left` alone was
128 KB); on the nineteen-scene disc the main frame shrinks by 7 KB inside the
unchanged stack reservation. The recurring one-VBlank miss at the Tutorial
return was finally located in the route CSV: it happened during loading, in
`WorldMeta::parse`, whose reference validation grew with every object kind
moved into the bank and ran without a poll. `WorldMeta::parse_with` now
steps a callback after each region and the disc module polls there, and the
admission differential polls per region too; the region-entry checkpoints
from builds 81 and 83 stay as belt and braces. The `town-reset` tape ended a
few polls short of its final Tutorial reload once loading polled more often,
so it gained 180 idle samples. Headroom 294,200 bytes (`.text` +2 KB for the
stepped parse); seven routes pass with zero missed VBlanks.

Build 87 finished P05 item 8 for the tables the guest links: the Great Door
bindings are a sorted `(slot, Binding)` list of the 86 slots that see the
door instead of an `Option` per catalogue slot, and `SCENERY_PACKET_BUDGETS`
is `u16`. `.data` 386,432 -> 382,016, headroom 294,200 -> 298,296 bytes;
seven routes pass. Per-view statics now total four bytes per slot
(`REGION_SCENES`, `REGION_SCENE_LOCAL`) plus two for the packet budget.

## P07/P25 first slice: break-effect art streams per scene (build 88)

Break-effect texels no longer share one linked sheet admitted in catalog
order. `host/break_effects.py` cooks one art bank per catalog scene inside the
same fixed VRAM rectangles (largest sheet 4,800 bytes), writes each as an HLZC
chunk (`.hkpsx/break-effects/scene_N.hkfx.z`, four zero bytes for a scene
without emitters) and emits per-scene `SCENE_STYLES`, `SCENE_EMITTERS`,
`SCENE_ART` and `SCENE_UPLOADS` tables plus `EFFECT_ART_MANIFEST`. The pack
gains one chunk per scene after the world metadata chunks (`build_guest.py`
copies them as `chunk_N.effectart`); `disc::Cache::prepare_effect_art` reads
and decodes the admitted scene's chunk into the arena front before the scene
decode and uploads it to VRAM (`break_effects::upload_scene`), and
`is_ready` includes the effect scene. The particle pool already threaded the
scene through `spawn_break`/`tick_break`, so the guest now indexes styles,
emitters and art by the particle's scene. Result: 210 supported emitters
across 18 scenes with no VRAM omissions (build 87 admitted 103 and omitted
36 parts of scenes 8 and 16); the remaining `other_parts` are unsupported
particle styles and non-particle debris, unchanged. Baked particle tracks
and their retired proof harness (`tools/test_particle_tracks.py`,
`tests/particle_tracks_source.rs`) are removed; every emitter uses the
scalar path that build 74 made canonical. Cost: `.data` 382,016 -> 395,104
(the doubled emitter and per-scene style tables are still linked), `.text`
422,836 -> 426,144, headroom 298,296 -> 281,912 bytes; seven routes pass.
Emitter and style metadata are per breakable and belong in the world bank
with their owner (P05 item 7 continues there); Geo art has no omissions on
this disc (24 rocks, 5,154 of 8,192 VRAM bytes) and follows the same chunk
pattern when it overflows.

## Crossroads Zombie Runners admitted (build 89)

The strict recognizer's `pending_movement_control` is now promoted to a
supported `ZombieSwipeWalker` control in `host/actors.py`, so the two
Crossroads_01 Runners (and the Runners in Crossroads_05 and _21) cook their
seven live and two corpse clips into the scene actor banks and emit
`ActorController::Runner` specs. The corpse "Single" clip uses tk2d wrap mode
6, which the scene format rejects; `cook.guest_wrap` maps a one-frame Single
to Once and fails closed on other modes (both cookers, `cook.py` and
`effects.py`, use it). `host/runner_audio.py` is an asset script writing
`data/runner-audio.adpcm` and `.rs` (20,464 bytes at SPU 495,792 to 516,256,
8,032 bytes left); `game/src/runner_audio.rs` uploads it after the Geo bank
and plays the walk loop per Runner on voices 21 and 22 at the authored
initial pitch (a third simultaneous Runner is counted as dropped) and the two
chase calls on voice 23 at the controller's pitch clamped to the authored
bounds. `main::runner_event` binds the sink: AudioPlay/AudioStop/Destroy drive
the loops, ChaseSound the calls; DustStart/DustStop are counted in
`HK_RUNNER_DUST_EVENTS` and not drawn, and source death effects are not
presented (both recorded as actor limitations). Two boot assertions in the
bank check were wrong on first contact with real data (the loop's authored
pitch sits below its nominal rate; the ADPCM header byte carries a filter
nibble) and were fixed. On the routes the Runners chase and swipe: the
`crossroads-floor` replay records 24 loop starts, 14 chase calls, 28 dust
events and 2 enemy hits with the invincibility cheat holding health at 5;
seven routes pass. Memory: `.text` 426,144 -> 428,144, `.data` 395,104 ->
395,152 plus the 20 KB bank, headroom 281,912 -> 279,832 bytes.

## Build speed (user priority, 2026-09-17)

Measured before: region cook with every per-region cache hit about 200 s
(postpack, actor banks, dedup and generate ran regardless), full recook 12 to
30 min whenever any cooker file in `host/cook_inputs.txt` changed, asset
scripts 45 s plus `pack_scenes` with its decoder proof 43 s every build, guest
link and disc about 10 s, seven route replays 7 min sequential; about 16 min
for a guest-only change.

Changes, each verified byte-identical on the cooked chunks and green on the
seven routes:

- Route replays run concurrently in the driver (1:42 wall for seven; the
  emulator is deterministic).
- `alpha_covers.record` persists its results in `.hkpsx/alpha-covers-cache.json`
  (72k calls per warm cook were two thirds of its time), and
  `texture_dedup.deduplicate_room` caches each (pack, replacements, code) result
  under `.hkpsx/dedup-cache/`; the region report is checkpointed only after
  fresh cooks and at most every tenth region instead of after every cached
  one. Warm cook: 200 s -> 23 s.
- The driver's asset cache (`.hkpsx/assets-cache.json`, keyed on every host and
  tool Python file, the region report and the cook verdict, verified against
  the hashes of everything under `data/` and the packer state) skips the
  twelve asset scripts and the guest prepass (`build_guest.prepass`: scenery
  budgets, `pack_scenes`, opaque tiles and groups, scene certificates) when
  nothing they read changed; `build_guest.py --no-prepass` reuses the packed
  scenes on disk.
- Cache misses are cooked by one worker process per scene (`regions.py
  --precook SCENE`), up to cpu_count - 2 at once, region 1's scene first because
  it writes the shared knight and gameplay files; the parent loop then reuses
  every worker result through the per-region cache. Full recook: 253 s.

Measured after: no-change or guest-only build 1:48 (of which the seven
parallel replays are about 1:40), cooker-code change about 5:00 including the
full parallel recook. Not done: a split between the geometry cook and the
metadata/postpack stages (the cook is one function whose art appends depend on
the effects code), which would make actor and effects changes cost the 23 s
warm path instead of the 4 min recook; and speeding the replays themselves,
which are bound by the longest route. Cook workers occasionally die with
SIGSEGV at startup under parallel load (never alone); the parent retries a
signalled worker up to twice and caps workers at eight.

## Build 90: the whole Forgotten Crossroads (45 scenes, 693 views)

All remaining Crossroads scenes joined `host/quality.py SCENE_TABLE` with the
envelopes the isolated pack matrix measured (`.hkpsx/scene-packs/*/summary.json`):
11_alt, 15, 22, 25, 27, 30, 31, 33, 35 to 40, 42, 43, 45, 46, 46b, 47 to 50,
52 and ShamanTemple (boss, preload and defeated variants excluded). The
parallel cook did 693 regions in about five minutes. Three cookers stopped on
new content and now record explicit omissions instead: `actor_sources` bounds
the 32-slot pool by supported actors only (Crossroads_50 and ShamanTemple hold
more HealthManagers than that as records), Runners whose depth is not the
measured guest plane stay unsupported, `cook_audio` substitutes the Dust
footstep set for the two scenes with another environment (Crossroads_30,
ShamanTemple) and `cook_music` omits atmos channels without a resident loop
(channel 15 in Crossroads_46b and Crossroads_50; the guest keeps six resident
loops on voices 6 to 11 and the SPU has 8 KB free). Seven routes pass; the new
scenes are reachable through their authored gates but no route enters them yet.
Actor inventory across the 45 scenes: 127 HealthManager actors, 14 supported
(Crawlers and Runners); unsupported families by count: Buzzer 20, Climber 15,
Fly 12, Roller 11, Zombie Barger 9, Spitter 8, Zombie Hornhead 8, Zombie
Runner variants 7, Zombie Leaper 6, Hatcher 6, Zombie Guard 2, Blocker 2.

Memory: `.data` 415,168 -> 558,400 bytes, headroom 259,352 -> 109,540. The
growth is the per-scene auxiliary tables P05 item 7 names: texture flags and
black/opaque cores for every texture of every scene (4,192 textures, about
38 KB), break-effect emitters/styles/art/uploads for all scenes (411 emitters,
about 55 KB), Pogo targets (281), atlas/coverage/metadata descriptors (about
20 KB), opaque groups and gates. At 5.5 KB per scene the full game cannot link
these; the next slices move them into the per-scene chunks (texture flags and
cores into the scene bank, effect tables into the effect-art chunk, pogo
targets into the world bank).

Build 91 moved the per-texture flags and black/opaque cores into the resident
scene bank: `with_texture_attributes` appends one 12-byte record per texture
after the reference arrays (offset word 104, `hk_format::scene::ATTRIBUTE_STRIDE`),
the stepped scene validation checks the section, and `render::init` fills two
`[[u8;4]; MAX_TEXTURES]` arrays from the admitted `Scene` view instead of
borrowing linked slices. `SCENE_TEXTURE_FLAGS`, `SCENE_BLACK_CORES` and
`SCENE_OPAQUE_CORES` are gone from the manifest. The opaque tile and group
caches now key on the packed scene order too (the attribute bytes reordered the
allocation and their per-index bindings went stale). `.data` 558,400 ->
421,536 bytes, `.bss` +22 KB (the core arrays and the larger resident banks
in the arena), headroom 109,540 -> 222,316; seven routes pass.

Build 92 moved the break-effect tables into the effect-art chunk. The chunk is
now `HKFX0001`: a 32-byte header (style, emitter, art, upload and frame counts,
texel base), 168-byte style records that reference the linked `CURVES` table
by index and a per-chunk frame table by span, 64-byte emitters, 8-byte art,
12-byte uploads with absolute texel offsets, then the texels.
`break_effects::load_scene` parses it at admission into bounded resident
tables (`FX_MAX_*`: 16 styles, 96 emitters, 48 art, 48 uploads, 96 frames;
the 45 scenes peak at 10/71/33/36) and uploads the texels; `scene_styles`,
`scene_emitters` and the art lookup read the resident scene only. The
per-scene `SCENE_STYLES`/`SCENE_EMITTERS`/`SCENE_ART`/`SCENE_UPLOADS` statics
are gone; only the eleven distinct sample curves still link. `.data` 421,536
-> 384,368 bytes, headroom 222,316 -> 257,132; seven routes pass (the
`crossroads-exit` statue breaks spawn from the resident tables).

Build 93 moved the static NailSlash (pogo) targets into the world bank:
`postpack_pogo` attaches each scene target to every region whose activation
envelope it touches, the encoder writes them as `KIND_POGO` objects (polygons
in the polygon section, flag 1 for horizontal-and-up, state = owning
breakable id or none) and `enemies::pogo_contact` iterates
`world::pogo_targets(region)`; `data/pogo.rs` is no longer generated. `.data`
384,368 -> 363,296 bytes, headroom 257,132 -> 271,340 (the world banks grew
by the targets, arena 386,968); seven routes pass. The chunk manifests
(atlas, coverage, metadata, effect descriptors, about 20 KB) are the last
per-scene linked tables.

## Crossroads Climbers (Tiktik) admitted (build 94)

`host/climber.py::recognize` admits the placed Climber instances that match
the audited contract (.hkpsx/climber70/CONTRACT.md: speed 2, spin 0.25 s,
padding 0.1, min turn 0.25, the 1.09375 x 0.921875 body at offset
(0.015625, 0.4765625), kinematic body, freeze-in-place recoil, Walk 4@10 loop,
Stun 7@12 loop-section, Death Air 8@30, Death Land 3@15, looping
play-on-awake audio source) and `actors.py` promotes them; `cook.py` binds
Walk (also as the turn clip, since the source walk animation continues through
turns) and Stun; `effects.py` accepts the Climber corpse (fling speed 20,
bounce 0.45, `resetRotation`, zero spawn offset) and `hk_sim::CorpseSpec`
gained `fling_speed`, scaling the 15-unit launch table. The shared
`ActorController::Climber { stun_clip, start_right }` selects a
`ClimberRuntime` in the guest actor pool: the kinematic controller from
`shared/hk-sim/src/climber.rs` owns position, velocity, rotation and phase;
the runtime advances position by the cardinal velocity each tick, casts the
controller's point rays against the resident terrain edges
(`climber_ray_hit`: TransformPoint with rotation and scale, TransformDirection
with rotation, nearest segment hit within the ray length) for attachment,
ground and wall sensing, dispatches the immediate first Walk iteration after
attachment, drives turns and the stun timer, freezes in place on an accepted
hit (`Recoil.OnHandleFreeze`) instead of displacement recoil, and dies into
the shared corpse path. The body box rotates with the transform (axis aligned
again at every cardinal rest) and the sprite is drawn rotated by the
controller's Q16 degrees through the debris sine table. Admitted: Crossroads_01
(2), Crossroads_07 (7 of its 8 actors), Crossroads_05 and Crossroads_50; 15
placed Climbers in total. Route telemetry (`HK_CLIMBER_X/Y/HP` for
level37:5145) shows the Crossroads_01 Climber circling its 6 x 2 block at
2 units/s with cardinal turns at each corner; seven routes pass. Not yet
presented: the climb loop audio (recorded limitation); the rotated sprite has
telemetry but no frame-level visual check on the routes, whose cameras never
reach a Climber. `Actor` is 208 bytes (pool 6,656). `.text` +22 KB.

Build 83 removed `Region.actors` and `Region.reveal_bindings`. Supported
actor objects (flag 64) carry their index into the new per-scene
`SCENE_ACTORS[scene]` static, which `host/world.py` assigns as `spec_index` on
the actor records before the bank is cooked; a region's reveal bindings are
one `KIND_REVEAL_BINDINGS` object whose index list interleaves (controller,
draw) pairs, bounded by the scene's controller count that `pack_scenes.py`
passes to the encoder. `enemies::sync_region` and the per-actor spec lookups
read `world::region_actors`; `reveal_masks::State::apply` takes a binding
iterator. The Tutorial return missed a VBlank again with the bank-driven
entry passes, so region entry now polls the pad before the passes, between
them and after `render::init`; the longest stretch between polls on the
`gate-return` replay is 315k CPU cycles in gameplay (a VBlank is about
564k). Measured against build 82: `.data` 435,632 -> 429,712 bytes (-5,920),
`.text` 414,288 -> 418,400, scene arena 361,732 -> 365,140 (the Tutorial bank
grew by the binding lists), linked headroom 260,776 -> 259,416 bytes. The
arena scales with the largest scene bank while the static tables scaled with
every view, so the trade is the intended one even though this step's net is
slightly negative on the nineteen-scene disc; seven routes pass.

Build 82 removed `Region.neighbours`. `world::upcoming` resolves neighbours
through the admitted bank (global chunk ids, bounds from the bank; neighbours
in another scene's bank are left to the scene gates) and the admission
differential checks each neighbour id against the catalogue instead of the
static list. `preload.rs` still has its own `Cell` abstraction and is not
linked into the guest. Measured against build 81: `.data` 444,208 -> 435,632
bytes (-8,576), linked headroom 252,584 -> 260,776 bytes; seven routes pass.

Build 81 removed `Region.grass`: grass objects carry their off and on draw
indices as payload words, `world::grass`/`region_grass` read them, and
`State::apply` became one outlined pass over the region's objects (grass and
breakable visibility, own and remote fades) instead of three. The first build
81 candidate missed one VBlank on `gate-return` and `town-reset` at the scene
gate return into Tutorial: region entry runs `render::init` plus every state
pass without a pad poll since the previous frame, and the bank-driven passes
tipped that stretch over a VBlank. Region entry now services the pad
(`input::checkpoint`) between `render::init` and the state passes. Measured
against build 80: `.data` 459,232 -> 444,208 bytes (-15,024), `.text`
413,216 -> 413,896, linked headroom 238,248 -> 252,584 bytes.

Build 81 adds the seventh route, `crossroads-exit`: the `crossroads-floor`
tape plus a climb authored offline with `tools/route_sim.py` (the shared
`hk_sim` step over the cooked room edges of chunks 110/111/115/116) up the
three floating blocks to the Crossroads_01 right ledge, the gate into
Crossroads_02 (scene 3, region 127), four statue breaks there (state ids past
the old three-scene tables: `HK_BREAK_COUNT` 13 -> 17), and the return gate
into Crossroads_01 (`State::reset_scene(3)` on the bank), ending on the ledge
at (99.5, 13.39) with five scene loads, four gate loads and zero faults. It is
a gate route (`validate_scene_gates.py` checks the resident bank).

Build 80 removed `Region.breakables` and `Region.remote_masks`. The bank gained
a fifth section of u16 index lists (stride 2); a breakable object carries its
off/on/edge lists as three (first | count << 16) payload words, its flags as
persistent | semi << 1 | door_sound << 2 | fade_ticks << 6, and is followed by
its `KIND_MASK_FADE` objects (draw list word, ticks); `KIND_REMOTE_MASK`
objects carry the owner state id, draw list, ticks and owner total.
`tools/world_metadata.py` now owns every breakable validation that
`host/world.py` used to apply before emitting Rust. The guest reads the
admitted bank through `disc::admitted_world_metadata` and
`world::bank_region` (`world::breakables`, `world::remote_masks`), with the
edge helpers outlined into `broken_edge` after a first iterator-heavy version
grew `.text` by 29 KB. `State::reset_scene` walks the admitted bank instead of
the static table. Host tests select a bank per thread through
`world::TEST_BANK`; `tools/test_world.py` cooks the scene 0 bank and a
synthetic fixture bank for them. Measured against build 79: `.data` 486,448 ->
459,232 bytes (-27,216), `.text` 402,384 -> 413,216 (+10,832), scene arena
354,068 -> 361,732 (banks grew), linked headroom 223,444 -> 238,248 bytes;
six routes pass.

Build 80 also fixed a latent fault: `world::SCENES` was a hard-coded 3 while
the disc carried 19 scenes, so `State::broken`, `fade_left`, `fade_active` and
`grass` were sized for three scenes. Breaking an object in scene 3 or later
would have indexed past the tables and leaving any such scene would have hit
`assert!(scene < SCENES)` in `reset_scene`. The generated `data/regions.rs`
now emits `pub const SCENES` from the catalog and `world.rs` asserts it equals
`disc::SCENE_COUNT` at compile time. No route reached those scenes, which is
why the six replays never caught it; a route into Crossroads_02 or later is
still owed. For the full game these tables must not scale with the catalog
either: `fade_left` alone would be 128 KB at 501 scenes, so transient fades and
grass should become resident-scene state while `broken` stays global (8 KB).

Build 79 removed `Region.hazards` and `Region.checkpoints`. The HKWMTA01
object record grew from 36 to 48 bytes with three kind-specific payload words
(hazard: origin x, damage | respawn << 16; checkpoint: spawn x, spawn y,
facing), `tools/world_metadata.py` owns the DamageHero respawn convention, and
`world::hazard_contact`/`world::checkpoint` read the admitted bank region
(resolved once per region entry by `disc::world_region_index`). Measured:
`.data` 497,984 -> 486,448 bytes (-11,536), linked headroom 218,628 -> 223,444
bytes; six routes pass. The remaining static payload per view is breakables
(297 records, 1,188 polygon points, edge/draw lists), grass patches (443 x 28
bytes), neighbours (1,760 x 4 bytes), mask bindings and `Region` itself, so
even the complete migration of the nested arrays leaves about 300 bytes per
view static; the full game needs the `Region` table itself to become
per-scene bank data, which is the P05 end state.

## Vengeflies (Buzzers) admitted, actor simulation bounded (build 95)

`host/vengefly.py::recognize` admits the Buzzer instances whose `chaser` FSM
parameters, transitions, body (1.25 x 0.625 box at offset (0, -0.1875),
dynamic gravity-free Rigidbody2D), Recoil (15 for 0.25 s), alert circle
(7.804 units) and clips (Idle 5@12, TurnToIdle 7@12 loop-section, Startle 4@12
once, Chase 4@12, TurnToFly 6@12) match .hkpsx/vengefly/CONTRACT.md (CIL of
IdleBuzz, ChaseObject, Decelerate, FaceDirection, LineOfSightDetector, Recoil
in the same directory). `shared/hk-sim/src/vengefly.rs` is the controller:
IdleBuzz roaming (per-axis random acceleration over 0.75 to 1 s windows,
roaming-edge damping), sight from the alert circle against the hero body plus
the existing `runner_senses::line_of_sight`, Startle (velocity zero, faces the
hero, 20 ticks), ChaseObject (0.045 per fixed step toward the hero, 5 units/s
clamp, one extra DoBuzz per frame while seen for the In Sight / Out of Sight
ping-pong), the ten-second attention span and Stop's 0.12 deceleration, with
a 50 Hz fixed-step accumulator inside the 60 Hz tick so the per-step
constants stay authored (docs/VENGEFLY.md). The guest `VengeflyRuntime`
(`ActorController::Vengefly { startle_clip, chase_clip, turn_fly_clip }`,
walk = Idle, turn = TurnToIdle) integrates the body with the bounded solver at
zero gravity, reuses the generic recoil displacement pass, and the corpse is
the source `breaker` variant: `CorpseSpec` gained `gravity` (42 here, 48 for
the earlier corpses) and `breaker` (removed on landing; break pieces not
presented). 18 of 20 placed Buzzers are admitted (two share a body-collider
variant the recognizer rejects), three of them in King's Pass, so every route
now meets a live Buzzer.

That exposed two guest problems the seven routes had never hit. First, a
Buzzer kill near the King's Pass exit left three coins and death particles
alive while the knight cut grass: `State::strike` invalidated the edge table
and the rest of that tick (coins, enemies, Lifeblood) fell back to the
per-query breakable scan (`broken_edge` rebuilds the bank region and decodes
every object per edge), several VBlanks in one frame; the input queue (16
polls) filled and the sampler faulted. `main.rs` now calls
`state.refresh_edges` right after a break or depleted rock. Second, with a
whole scene resident every actor of the scene advanced every tick, each
re-decoding the 128 edges of its own view plus the broken-object exclusions
(`Room::edge` under `advance_in_view` was the largest single enemy cost).
`enemies.rs` keeps four keyed terrain copies (`ActorEdges`, keyed by target
view, active view and a new `State::edge_epoch` that every exclusion change
bumps) and only advances actors within one view (24 x 16 units) of the active
view; farther actors hold their state. That gate is a recorded departure from
Unity, which keeps simulating them, and is marked `ponytail:` in the code for
review. Input queue peak on `gate-return` went from 11 back to 7 (the scene
load) with pending 1 to 2 in gameplay; `HK_INPUT_PENDING` is new telemetry.
`regions.py::remap_actor_clips` now remaps every `*_clip` binding into the
scene clip space: the Climber's `stun_clip` and the new Vengefly clips were
left in region-local numbering, which drew a stunned Tiktik or a Buzzer with
the knight's frames. `Actor` is 256 bytes (pool 8 KiB; folding the three
optional runtimes into one enum would give back about 120 bytes each).
Linked headroom 193,508 bytes, scene arena 415,488; seven routes pass; hk-sim
(Vengefly tests included), Python (424) and world runtime (47) suites pass.

## Gruzzers and Baldurs admitted (builds 96 and 97)

Build 96 admits the Gruzzers (Fly, 19 placed across Crossroads_07, _25 and
_50; `host/gruzzer.py`, `shared/hk-sim/src/gruzzer.rs`, docs/GRUZZER.md,
.hkpsx/gruzzer/CONTRACT.md). The `Bouncer Control` FSM waits for the main
camera within 44 units, then flies at 5.2 units/s along a random angle; the
controller keeps the angle and the FSM's `Facing Right` flag, the guest turns
the angle into a velocity through the debris sine table, moves the
gravity-free body with the bounded solver and reports the blocked axis as the
bonk side in the CheckCollisionSide order (up, right, down, left), each of
which re-aims from the authored ranges. Its corpse is a breaker that bounces
twice (`CorpseSpec.smash_bounces`, Corpse.Land counts one bounce per frame and
smashes once the count reaches smashBounces) with 0.7 bounce and 0.7 gravity.
Two cook-side fixes came with it: `host/cook_inputs.txt` now lists the
recognizer modules (climber, vengefly, gruzzer, baldur), which the whole-cook
cache had never fingerprinted, so a recognizer change alone did not recook;
and the grass-impact variant budget went from 8 to 32 (`host/world.py`,
`tools/world_metadata.py`), because per-scene clip indices shift as actor
art is appended and the deduplicated catalogue outgrew 8 with more actor
scenes. Headroom 177,124 bytes; seven routes pass.

Build 97 admits the Baldurs (Roller, 11 placed in Crossroads_ShamanTemple;
`host/baldur.py`, `shared/hk-sim/src/baldur.rs`, docs/BALDUR.md,
.hkpsx/baldur/CONTRACT.md). The `Roller` FSM idles facing the hero with x
velocity held at zero, wakes on the 21.14 x 1.9 alert box plus line of sight,
plays Start, draws a 2 to 3 s roll time, accelerates 0.45 units/s per frame
toward the hero up to 11 units/s, flips and launches at 12 units/s along 115
or 65 degrees on a wall contact, resumes the roll the other way on landing,
and stops, rests half a second and idles when the roll time runs out;
Recoil's horizontal event resets the roll speed. The guest `BaldurRuntime`
runs the gravity body (scale 0.8) with last frame's blocked axis and grounded
flag as the WALL and GROUND senses, resolves the spawn once like the
Crawlers, and adds the generic Recoil displacement (25 for 0.15 s). Placed
Rollers start mirrored (scale.x -1, the FSM sets the sign itself) and some
carry their body box twice; both are accepted. The corpse prefab has no
Corpse component: its `corpse` FSM lands, plays Death Land while rolling
faster than 2 units/s, then shrinks and destroys itself; the port gives it
`CorpseSpec.remove_after_land` 120 ticks with the box bounds of its 0.43
circle. `Actor` is 312 bytes; folding the five optional runtimes into one
enum is the obvious next saving.

## Husk Bullies and Hornheads through the parameterized Runner (build 98)

Every Zombie Swipe placement in the catalog runs the same PlayMaker FSM: a
structural fingerprint (`runner.py::fsm_fingerprint`: states, transitions,
enabled actions and their scalar fields, variables other than `Lunge Speed`)
is identical across Runner, Barger and Hornhead objects in seven scenes; the
old whole-serialization sha differed per scene only through embedded owner
references, which is why the Runners outside Crossroads_01 read as
"unverified variants". The differences are Walker fields (walk speed 1.5 or
2.5, pause wait/time ranges) and the FSM `Lunge Speed` (6, 14, 9), plus each
placement's body and alert boxes, clip frame counts and corpse prefab
(`Corpse Zombie Basic One/Three/Five`, same structure). `hk_sim::runner` now
takes `Params { walk_speed, lunge_speed, walking_wait, paused_wait }`
(`Params::RUNNER` is the level37 variant; the wait sampler receives the
endpoints), `runner_senses::Shape` carries the body and alert boxes
(`Shape::RUNNER` for the old constants, `from_boxes` from the actor spec), and
`ActorController::Runner` gained `params` and `alert`. The recognizer accepts
depth within 0.01, continuous collision detection (Hornheads), libraries
without `Fall` (never played) and variant clip counts, and the corpse admits
the three Basic prefabs by name and player data. Admitted: 9 Runners, 9
Bargers, 8 Hornheads (from 4 Runners). Still unsupported in the family:
Leapers (their own `Zombie Leap` FSM), Shields (no pauses, own FSM), Guards,
Myla, one Runner with an `enemy_corpse` FSM and one that starts inactive.
Headroom 168,932 bytes; seven routes pass.

## Leapers through a Leap attack on the Runner walker (build 99)

The Leaper shares the Walker with the Runner family and layers the `Zombie
Leap` FSM: Ready (alert by name plus sight) -> StopWalker, Jump X Speed =
(Hero X - Self X) * 1.25, face -> Anticipate (velocity zero, Attack clip,
wait for its trigger frame 3 of 11 at 12 fps) -> Launch (velocity (Jump X,
20)) -> Lunge until bottom contact -> Land clip -> Idle .5 s -> Reset
(StartWalker). `hk_sim::runner::Params` gained `attack: Attack::Swipe |
Attack::Leap { trigger_ticks, jump_speed_y, jump_x_factor, idle_ticks }` and
`gravity` (Runners fall at gravity scale 1, Leapers at .8), `Senses` gained
`grounded`, and the Leap FSM has no TOOK DAMAGE or RECOIL HORIZONTAL
transitions, so those callbacks are no-ops for it. `host/runner.py`
fingerprints the Leap FSM separately (`LEAP_FSM_SHA256`), reads the trigger
frame from the Attack clip and binds Attack as the anticipate and lunge slots
and Land as the cooldown slot; `Corpse Zombie Leaper` joins the admitted
Runner-family corpses. Admitted: 5 of 6 Leapers (the mirrored placement in
Crossroads_37 is still rejected, as the Walker facing of a mirrored start is
unverified). The Leaper's own attack samples are not in the Runner audio bank;
the guest plays the Runner chase sound in their place. Seven routes pass.

## Aspid Hunters and the first projectiles (build 100)

`host/aspid.py` admits the Spitters (18 placed, including the `startAlert`
arena placements that skip Idle) against .hkpsx/aspid/CONTRACT.md;
`shared/hk-sim/src/aspid.rs` ports the `spitter` FSM (IdleBuzz roaming,
7.804 alert circle plus sight, DistanceFly at 7 units with 0.1 per fixed step
and 4 units/s, 1.5 to 2.25 s checks of range 14 and a clear ray, Fly Back at
8.25 for half a second, Fire Long anticipation to its trigger frame 9 at 9
units, one FireAtTarget shot at 15 units/s, the 12.1 unalert circle with an
8 s Range Out Timer, no damage transition), sharing IdleBuzz and DistanceFly
through the new `shared/hk-sim/src/buzz.rs` (the Vengefly now uses the same
IdleBuzz). The guest gained its first projectile pool: `EnemyWorld.shots`
(eight slots) holds `Spitter Shot R` bodies flying straight under gravity
0.05; a shot ends on a terrain contact of its movement segment or on a hero
box overlap (one damage through the existing cheat-aware hurt path), plays
Impact for 18 ticks and leaves. The shot Idle and Impact clips are cooked
from the shot library at the prefab scale (0.7 x 0.8) by a shared
`_ClipCooker` that the corpse art now uses too; the fire cue travels as a
`RunnerEventKind::Fire` collected inside `EnemyWorld::tick` and spawned after
the actor pass. `Corpse Spitter` is `massless` (no collider; the source lets
it fall out of the scene) and is removed on its first landing here.
`host/similarity_dedup.py` still capped one cooked view at 256 KiB from the
per-room era; it now uses `quality.ROOM_BYTE_BUDGET` (384 KiB), since the
resident bank is bounded by the scene arena, after a Crossroads_03 view with
the new shot art reached 264 KiB. 118 of 144 placed actors are admitted;
headroom 146,404 bytes (Actor is 400 bytes: the runtime enum fold is due);
seven routes pass. Not presented: shot rotation and stretch, dribble spatter,
Aspid audio.

## One runtime per actor (build 101)

`enemies::Actor` carried six optional controller runtimes side by side; it
now holds one `Runtime` enum (Walker, Runner, Climber, Vengefly, Gruzzer,
Baldur, Aspid) with small accessors, so an actor costs the largest runtime
rather than the sum: `Actor` 400 -> 176 bytes, the 32-slot `EnemyWorld`
13,024 -> 5,856 bytes. The two ShamanTemple Buzzers that carry their body box
twice are admitted (`runner.py::body_box`, shared with the Baldur); the two
duplicate-box Climbers in Crossroads_03 and _05 are not, because their views
sit at the 416 texture budget after similarity dedup and the extra Climber art
pushed one to 421. That is the first time actor art has hit the texture
budget; a cook-side policy that drops an actor's art (leaving it an
unsupported record) when its view overflows is the next step if more
placements need it. 120 of 144 placed actors are admitted; headroom
148,452 bytes; seven routes pass.

## Source camera locks and a damped camera (build 102)

P09 step 3, first slice. The 122 enabled `CameraLockArea` objects of the
catalog now ride in the world bank as kind 11 objects (trigger bounds plus one
two-point polygon with the camera-centre limits; flags 1 preventLookDown, 2
preventLookUp). `host/regions.py::postpack_camera_locks` attaches every lock
whose trigger touches a region and resolves the source `-1` (and 9999) sides
the way `CameraController.LockToArea` does, to the scene camera bounds (14.6 /
xLimit / 8.3 / yLimit in camera-centre units); Crossroads_18 authors an
inverted x pair, which the guest clamps to the sorted pair as Mathf.Clamp
effectively does. The guest's new `camera.rs` replaces the per-view instant
clamp: the camera target follows the hero with the source one-unit look-ahead
and 0.075 s damping, the camera follows the target with 0.15 s damping at
most 30 units/s (per-tick exponential approach with the serialized
CameraTarget/CameraController time constants, not SmoothDamp's spring), both
clamped to the lock whose trigger the hero body overlaps (the last in bank
order, as the source keeps the most recently entered zone) or the view's
camera bounds; a hero displacement over 20 units in a tick (gate, respawn)
snaps. Fall catcher, look up/down, quake and super-dash offsets are not
modelled. `tools/world_metadata.py` joined the cook inputs. Headroom 136,620
bytes; seven routes pass.

## Source gate rules: facing into side gates, delayed top colliders (build 103)

P09 steps 2 and 4. `TransitionPoint.TryDoTransition` only fires a left or
right gate while the Knight faces into it; a recoiling Knight never
transitions (the source zeroes its velocity and translates it back out of the
trigger); top and bottom gates fire on contact. Top gates ship with a disabled BoxCollider2D and a
`Delay Collider` FSM (Wait 3 s, SetCollider) so the arriving Knight cannot
bounce straight back out; in the admitted catalog that is Crossroads_01
`top1` (the Tutorial `top1` leads outside the catalog). `host/regions.py`
records each gate's `collider_delay` and initial collider state,
`host/world.py` emits `Gate.side` (1 left, 2 right, 3 top, 4 bottom) and
`Gate.delay_ticks`, and `world::gate` takes the recoil state and a per-scene
tick counter (`scene_ticks`, reset when the scene changes). The development
`gate_cooldown` (90 ticks after any transition) stays as a guard. Headroom
136,620 bytes; seven routes pass.

## Every gate enters through the source sequence (build 104)

P09 step 2. The Great Door's audited HeroController.EnterScene side sequence
(settle 10 ticks, camera fade after 7 over 30, lead 12, then the forced walk
at RUN_SPEED for 28 ticks with input ignored) now runs for every left and
right gate: `great_door::World` takes the walk-in direction explicitly
(`begin_gate_entry(scene, direction)`; a right exit enters at the
destination's left gate walking right) and no longer marks the Great Door's
one-time entry consumed for ordinary gates. Top gates use the audited top
sequence (`begin_top_entry`: hidden with gravity off through the settle and
lead waits, then the -12 units/s drop with input ignored for 0.33 s;
`.hkpsx/crossroads68/RESULT.md`); the Knight is not drawn while hidden.
Bottom gates keep the immediate placement. The `town-well` and
`crossroads-exit` routes end a few units earlier because their tapes were
recorded with instant control; both still complete. Headroom 134,572 bytes;
seven routes pass. Not modelled: entryDelay and delayBeforeEnter fields
(zero on every admitted gate), super-dash entries, the door-entry animation
and the horizontal enter wait extra.

## Benches: rest, heal and respawn marker (build 105)

P13 step 3, first slice. `host/benches.py` cooks every active RestBench with
a `Bench Control` FSM (Town, Crossroads_04, _30, _47, ShamanTemple; the
inactive Crossroads_01 Ascender is skipped) into world bank kind 12 objects
(trigger box, seat, Knight sit clip base); the Knight's Sit, Sit Idle and Get
Off clips are cooked only into views that contain a bench trigger, at a
constant index after the focus clips. `game/src/bench.rs` follows the source
FSM subset: UP with the hero body inside the trigger while grounded and in
control starts the rest, the Knight slides to the seat over 0.2 s and sits
(Sit then Sit Idle), Rest Burst at 0.5 s heals to full and sets the respawn
marker (scene, seat, facing, slot), any jump/attack/direction press after
0.6 s plays Get Off and control returns 0.85 s later. Death now respawns at
the last bench seat instead of the new-game spawn (standing, not the source
wake-on-bench). Not modelled: the map, charm prompt, sleep, bench tilt, Save
Game (next slice: memory card) and the bench audio. New route `town-bench`
(the town-resident tape cut at the Dirtmouth bench, UP, wait, LEFT) must end
with `HK_BENCH_RESTS` 1 (`REQUIRED` in the build driver); eight routes pass.
Headroom 132,524 bytes.

## Bench Save Game on the memory card and title Continue (build 106)

P13 steps 5 to 7, first slice, on the SDK's `psx-mc` crate (`game/Cargo.toml`;
no card protocol code was written in the port). `game/src/save.rs` holds a
40-byte record (magic `HKS1`, bench scene, standing position, facing,
catalogue slot, Geo wallet, Great Door hit count, FNV-1a checksum) in one
retail-style file (`BASLUS-00000HKPSX000`, title "HOLLOW KNIGHT PSX") on the
port-1 card. The Rest Burst that sets the respawn marker also requests a
save; the write runs at the next frame boundary inside the input sampler's
scene-load bracket with a pad checkpoint after every card frame. A card frame
transfer is longer than a VBlank and cannot be interrupted by a pad poll
(SIO0 is shared), so the sampler gained a blocking-transfer mode: those poll
gaps count in `HK_INPUT_BLOCKED_VBLANKS` (20 for this write) instead of
`HK_INPUT_MISSED_VBLANKS`, which stays a route fault. An unformatted card is
formatted first; no card or a card error counts in `HK_SAVE_ERRORS` and play
continues. At boot, before the pad sampler exists, the record is read; when
it decodes and names a slot the catalogue still has for that scene, the title
menu shows a fifth row "Continue" (`menu_state::State {has_save, continued}`;
`menu::run` returns `(Settings, bool)`) and choosing it starts standing at
the saved bench with the saved wallet and door hits
(`geo::World::restore_wallet`, `great_door::World::restore`). Masks are the
constant 5, so the record stores no max health. Found on the way: the build
105 respawn marker used the source seat y (the sprite's), which puts the body
inside the floor and drops it through; the marker and the record now keep
the Knight's standing y. Checks: `tests/test_save.py` (record round trip and
corruption rejection through a stub card), `tests/test_menu.py` (Continue
offered only with a save), `shared/hk-sim/tests/input_sampler.rs` (blocked
gaps), and two card routes: `town-bench` boots with a fresh formatted card
(`--memcard` in `tools/replay_cue.py`; `MEMCARDS` in the driver copies
`tools/cards/<route>.mcd` to the validate directory, a missing fixture boots
freshly formatted) and must end with `HK_SAVE_WRITES` 1 and `HK_SAVE_ERRORS`
0; new `town-continue` (tape: UP, X at the title, then walk) boots from
`tools/cards/town-continue.mcd`, a town-bench run's written card, and must
end with `HK_SAVE_LOADED` 1, `HK_MENU_ROW` 4, the Town slot, wallet 6 and 13
door hits. Nine routes pass. Headroom 116,124 bytes (psx-mc's filesystem code
and the save path cost 16.4 KB; the trend since build 94 is now 246 KB to
116 KB and needs a review before the next large runtime). The fixture card
depends on the Town catalogue slot: after a catalogue change, copy
`.hkpsx/validate/town-bench.mcd` over it. Not done (next slices): the source
save menu (three slots, file select, delete), power-interruption tests (step
8; the SDK's `Card::write` is not atomic across its frames), a versioned
global-flag codec once there are flags to save, and step 4 (Geo loss and
Shade).

## Death Geo loss, the Hollow Shade and the soul limiter (build 107)

P13 step 4. `shared/hk-sim/src/shade.rs` ports the `Shade Control` FSM of the
`Hollow Shade` prefab and `game/src/shade.rs` runs it; `host/shade.py` audits
the prefab and cooks its art (.hkpsx/shade/CONTRACT.md, docs/SHADE.md). Death
now drains SOUL, moves the whole wallet into the pool the Shade carries,
records where and how strong it is (`shadeHealth` = clamp(maxHealth / 2, 1,
99) and HP = `nailDamage` times that, which reproduces the prefab's 10), saves
the card and starts the soul limiter, which caps the vessel at 66 instead of
99. A second death overwrites the pool, so the earlier Shade's Geo is forfeit
as in the source. Death saves even with no bench rested; that record points
Continue at the new-game spawn. No object in the 45 admitted scenes carries
the `Shade Marker` tag (verified by scanning every admitted level), so the
Shade always appears at the death position through the FSM's own fallback.

The Shade spawns on entering the scene that recorded it, idles until the hero
is inside its 7.27-unit alert circle with a clear ray, then startles and
chases (ChaseObject 0.2 per axis then ChaseObjectV2 0.16 along the direction,
both clamped to 4 units/s), holds 3 units out while levelling with the hero's
y, and lunges at 8 units/s with a half-second wind-up when the hero is within
5 units and 0.2 of its own height. Its damage box is live for the Slash state
alone. Past 25 units from the spawn it retreats and resets. Killing it returns
the pool, clears the record and ends the limiter. `DistanceFly`'s
`targetsHeight` branch was missing from the shared helpers and is now
`buzz::distance_fly_height`: only x uses the far/near test while y always
seeks the target, which is what makes the level-with-the-hero slash condition
reachable.

Art: the Shade can appear in any scene, so its 72 frames cannot live in a
per-view atlas. They sit in linked RAM (30,386 bytes) and reach VRAM through
the shared 64x64 animation slots under keys above `SCENE_TEXTURE_CAPACITY`,
with only two 16-colour CLUTs resident in the free rows above the dialogue
palette. VRAM had 608 spare bytes, so no new rectangle was available; the
cache's upload closure gained a second source rather than a new reservation.
The largest frame is 740 bytes against the 2,048-byte slot.

Checks: six controller tests in hk-sim, `tests/test_shade.py` (the real guest
runtime against the cooked art: record, spawn rule, clip table, every frame
inside one slot), `tests/test_save.py` extended to the grown record with the
checksum moved to the end so it covers the Shade fields, and a new
`town-shade` route, and a `kings-death` route once the probe was repaired. The
first attempts at a death on tape failed because no admitted scene puts a
reachable enemy or hazard on a recorded path by accident; the repaired
`tools/route_probe.py` found the answer natively in seconds. `kings-death`
walks the King's Pass traversal (its four jumps kept) with the nail silent past
the fifth breakable, so the Crawler at x 125.5 survives, the sixth obstacle
stays unbroken and the Knight stops beside it at 126.78 and is killed where it
stands. It must end with `HK_DEATHS` 1, `HK_ENEMY_KILLS` 0, `HK_SHADE_PRESENT`
1, `HK_SHADE_HP` 10 and one clean card write; reading that card back shows the
Shade recorded at exactly the death position, which is the Set Shade fallback,
and Continue pointing at the new-game spawn because no bench was rested. Its
wallet is zero, so the Geo transfer itself stays `town-shade`'s job. The
seeded-card route still boots from
`tools/cards/town-shade.mcd`, a card whose save record already carries a Shade
six units from the Dirtmouth bench with a 99-Geo pool. It must end with
`HK_SHADE_KILLS` 1, `HK_SHADE_PRESENT` 0, the wallet at 105 and the Knight on
2 masks, which exercises spawn, chase, contact damage, the nail, death and the
Geo return. Ten routes pass, and all 428 host tests.

Headroom fell to 68,988 bytes from 116,124: the cooked art is 30,386 of that
and the controller, runtime and grown save record the rest. The trend since
build 94 is now 246 KB, 132 KB, 116 KB, 69 KB, and the next large runtime
needs a budget pass before it lands rather than after. Not done: a route that
the Shade's particles, orbs and audio. The save screen is in: Start Game opens
four profiles, each owning two card files, showing Geo or Empty or Damaged with
an explicit line for no card, full, damaged and unformatted. Play time,
completion and the map zone that a source slot also shows are not tracked here.

One deliberate departure from the source, at the project owner's request: the
card is never written automatically. The source saves on a bench rest and on
death; this port heals and sets the respawn marker on the rest, then asks
"Save game?", and death records the Shade and Geo pool in memory only until the
player accepts a bench prompt. The `bench-save` route exercises that path and is
what the power-cut check interrupts. Step 8 is
covered, and it found a real defect: `Card::write` frees a file's directory
entry before rewriting it, so the single-file save had a roughly 190 ms window
in which an interruption left the card with no loadable save at all, the
previous one included. Measured by stopping the emulator at cycle points across
the write rather than by poll, since a poll-bound tape pauses with the guest and
cannot land inside the transfer. The save now alternates between two files with
a sequence number and takes the newer valid one at boot, so an interrupted write
never touches the copy in use; `tools/validate_power_cut.py` runs after the
routes and requires a loadable save at four points across the transfer. Step 9 is covered: `kings-return` boots from the
card `kings-death` wrote, finds the Shade that real death recorded and recovers
it, so quit, reload and recover is verified on genuine save data rather than a
seeded fixture. Twelve routes pass.

## P14 step 1: the movement audit against the original (build 112)

Before adding dash, wall jump, super dash or double jump, the plan requires
re-auditing the movement already shipped against original traces. The port's
constants are not hand-tuned: `host/cook.py` reads `RUN_SPEED`, `JUMP_SPEED`,
`DEFAULT_GRAVITY`, `MAX_FALL_VELOCITY`, `JUMP_STEPS` and `JUMP_STEPS_MIN`
straight off HeroController, with the collider box and the project's gravity
and fixed timestep, and resamples the jump hold from the source's 50 Hz steps
to the guest's 60 Hz. Every constant the remaining abilities need is in that
same serialized prefix and is already captured in
`.hkpsx/gameplay-source.json`: DASH_SPEED 20 / DASH_TIME 0.25 /
DASH_COOLDOWN 0.6 / DASH_QUEUE_STEPS 10, WALLSLIDE_SPEED -8 /
WJ_KICKOFF_SPEED 16 / WJLOCK_STEPS_SHORT 5 / WJLOCK_STEPS_LONG 10 /
WALL_STICKY_STEPS 3, DOUBLE_JUMP_STEPS 9, SUPER_DASH_SPEED 20, and the
backdash and shadow-dash sets.

Measured against the existing `movement-phase-1` reference capture (the
instrumented Windows original under CrossOver, 300 gameplay frames of a
recorded input schedule) using a new flat-ground `jump-probe` tape on the port:

| quantity | original | port | delta |
| --- | ---: | ---: | ---: |
| ground run speed | 8.300 u/s | 8.2993 u/s | 0.0% |
| jump rise | 3.705 u | 3.886 u | +4.9% |
| airtime | 0.617 s | 0.633 s | +2.6% |

Run speed is exact. The jump is about 5% too high and hangs 2.6% too long. The
hold duration itself resamples correctly (the source holds `JUMP_SPEED` for
`JUMP_STEPS + 1` = 10 steps of 0.02 s, which the cook turns into 12 ticks of
1/60, both 0.2 s), so the discrepancy is in how the port integrates gravity
around that hold rather than in the constants. It is small but it is a real
mismatch, and the plan is explicit that these are fixed before complex moves
land on top of them.

Both were then fixed, and the second was a genuine missing feature rather than
a constant:

The held velocity. Unity applies gravity inside the same physics step that
`Jump()` pins the velocity, so the source rises at `JUMP_SPEED - g * 0.02` =
15.702, which the reference trace shows directly. The cook stored `JUMP_SPEED`
unchanged and the guest subtracted gravity over its own smaller step, holding
15.860 for the whole 0.2 s. The cook now pre-compensates the step-length
difference, and the guest measures 15.701 against the source's 15.702, the
residue being Q16 rounding.

The jump queue. `HeroController::.ctor` sets `JUMP_QUEUE_STEPS` to 2, and the
source retries the jump each step while the button is held and the counter is
within that, so a press up to two steps before landing still fires. The port
required a strict rising edge on an already-grounded frame and dropped those
presses entirely. This is the "release buffering" the package text names, and
it is now implemented with the constant taken from the CIL, since it is a
constructor literal rather than a serialized field.

Consequence, and it is not small: five recorded routes (gate-return,
town-reset, town-resident, crossroads-exit, town-bench, with town-well and
crossroads-floor stalling in the same place without an assertion to catch it)
no longer complete the King's Pass platform climb. They are human recordings
whose jump timings were tuned against a jump that rose 1% too high, and they
now fall short of the first platforms. The level itself is not less
traversable: the port now jumps within 0.001 units of the original and the
previous build jumped higher than the original, so nothing reachable before has
become unreachable.

Diffing old against new physics natively through the probe, which replays a
tape's poll stream as a probe script, located the break exactly. At tick 2047
the lower jump lands one tick earlier than the old one did, and that single
tick falls inside the new two-step input buffer, so the Knight jumps where the
old build swallowed the press. Everything after that is a different route. A
one-tick landing difference is all it takes, which is the real lesson: a
recorded traversal is a knife edge, and P14 is about to change movement four
more times.

So the route suite was rebuilt around saves rather than traversals. A card
fixture puts the Knight where a route needs to be and the tape does only the
local work, which no longer depends on jump timing at all. `well-drop` resumes
at the Dirtmouth bench and walks right into the well, exercising the Town to
Crossroads gate; `town-reset2` resumes, walks, and resets with Select. Those
plus the existing save-resumed routes replace gate-return, town-reset,
town-resident, town-well, crossroads-floor, crossroads-exit and town-bench,
whose tapes are deleted rather than left encoding physics the port no longer
has. Eight routes pass.

Owed at the time: the King's Pass platform climb, the Town to King's Pass gate
pair, and the deeper Crossroads gates. The last of those is back as
`crossroads-gate`; see the route coverage entry below for what blocks the other
two, which is more specific than "wants a probe-authored route".

## P14 step 2: the four movement abilities (builds 116 to 118)

The Mothwing Cloak, the Mantis Claw's wall slide and wall jump, the Monarch
Wings double jump and the Crystal Heart, all bound to source constants rather
than to feel. None of the admitted scenes contains an ability pickup, so each
one is reachable only through its own Cheats row for now: L1 dashes, R1 holds
to charge the Crystal Heart, and the Claw and Wings need no button of their
own.

Where the numbers come from. `DASH_SPEED` 20, `DASH_TIME` 0.25, `DASH_COOLDOWN`
0.6 and `DASH_QUEUE_STEPS` 10 are serialized on HeroController, as are
`WALLSLIDE_SPEED` -8, `WALL_STICKY_STEPS` 3, `WJ_KICKOFF_SPEED` 16,
`WJLOCK_STEPS_SHORT` 5, `WJLOCK_STEPS_LONG` 10 and `DOUBLE_JUMP_STEPS` 9.
`DOUBLE_JUMP_QUEUE_STEPS` 10 and the three-step wing flourish are CIL
literals, read the same way `JUMP_QUEUE_STEPS` was in step 1. The Crystal Heart
is not in HeroController at all: its `SUPER_DASH_SPEED` field of 20 is never
read, and the Hero's Superdash PlayMaker FSM supplies its own `Superdash Speed`
of 30, a `Charge Time` of 0.8 s, a `Cancelable Time` of 0.2 s and a 0.5 s Hit
Wall recovery. `host/superdash.py` binds those from the FSM and asserts the
shape it read, including that `Superdash Speed neg` is the negation Init
computes rather than the zero it serializes with.

Behaviour worth recording, because it is not what a re-implementation from
memory would produce. `WALL_STICKY_STEPS` is an unstick counter, not a stick
one: letting go of the stick keeps the Knight on the wall, and only holding
away for three steps detaches. A wall jump locks horizontal velocity for
`WJLOCK_STEPS_LONG`, shedding the kickoff down to `RUN_SPEED` across the lock,
but pressing back releases it early once `WJLOCK_STEPS_SHORT` have passed. The
double jump spends its first steps on the wings and cannot be cancelled by
releasing the button. And the source's jump decision is ordered: wall jump
first, then a ground jump, then a double jump, so a jump pressed against a wall
never becomes a double jump.

`Params` gained fourteen fields across these, and the P14 tail will add more.
Every previous addition broke roughly ten struct literals belonging to the
enemy, corpse, Shade and lifeblood bodies, which reuse `Player::step` purely
for its collision and set four fields out of twenty-four. Those literals now
spread `Params::ZERO`, so the cascade does not recur.

The wall probe. `CheckTouchingWall` is a raycast in the source, so contact
holds whether or not the wall is being pushed into, which a collision clamp
alone cannot reproduce. A probe of one sixteenth of a unit outside each side,
against the same vertical edges the sweep uses, does reproduce it. It costs an
extra edge sweep per body per tick, and every enemy body goes through the same
`step`, so it runs only for a body that has the Claw.

Verification: 77 hk-sim tests, including 5 for the dash, 6 for the wall and
double jump and 5 for the Crystal Heart, plus the 8 routes and the 4
power-cut interruption points on the final CUE.

Not done here, and recorded rather than hidden: the Wall Slide, Walljump, Dash
and Double Jump clips exist in the Knight's animation table but are not cooked,
so an ability currently plays the Fall clip; the Superdash's NORM CANCEL out of
the Cancelable window is wired to jump only, not to dash or attack, which waits
for the attack integration in step 4; SLOPE CANCEL, the Zero Timer that ends a
travel stalled against a slope, is not modelled; and none of the charge, blast,
trail or Hit Wall presentation is cooked.

## P14 steps 3 and 4: the ability catalog, input priority and two missing buffers (build 120)

Step 3 asks for the extracted ability catalog to be used as exhaustive
authority rather than a guess at which traversal states exist, so
`host/abilities.py` reads the PlayerData booleans out of the CIL and asserts
that every one it names is still there. Eleven of them gate movement or
traversal. It then walks the 45 admitted scenes for the components that put the
Knight into a special state, and the answer is narrower than expected:
`AcidCorpseSplash` and its `DamageHero` volumes in Crossroads_11_alt and
Crossroads_35, and one `BounceShroom` in Crossroads_38.

Acid therefore needs nothing new. Its damage already arrives through the
generic `DamageHero` hazard path with a `HazardRespawnTrigger`, which is what
the source does; `hasAcidArmour` would cancel that damage, and since nothing
grants Isma's Tear the gate is catalogued rather than written. `hasLantern`
gates darkened rooms and no admitted scene is darkened. The bounce shroom is a
real unimplemented surface, and it belongs with the pogo machinery rather than
here: `SHROOM_BOUNCE_VELOCITY` and `BOUNCE_SHROOM_TIME` sit next to the pogo's
`BOUNCE_VELOCITY`, and P11 step 4 owns bouncy targets. Recorded as owed there.

The Shade Cloak completes the "upgraded dash behavior" half of step 2.
`SHADOW_DASH_SPEED` and `SHADOW_DASH_TIME` are identical to the ordinary dash's,
which the cooker now asserts, so the upgrade is exactly two things: the dash
becomes invulnerable, and it is gated by its own `SHADOW_DASH_COOLDOWN` of 1.5 s
rather than the dash's 0.6 s. `TakeDamage` returns immediately while
`cState.shadowDashing`, which means a Shade Cloak dash passes through a hazard
instead of respawning at it, deliberately unlike the Invincibility cheat.

Step 4's input priority came out of the Can* methods rather than from taste.
`CanJump` refuses a jump while dashing, already jumping, wall sliding or inside
`HEAD_BUMP_STEPS` of a ceiling, and grants one while `ledgeBufferSteps` is
still running. `CanDash` refuses a dash inside `ATTACK_RECOVERY_TIME` of a
swing, and allows one off a wall even when the air dash is spent. `CanAttack`
refuses a swing while dashing. The jump chain itself is ordered wall jump, then
ground jump, then double jump.

Two of those are constants the step 1 audit missed outright, both `.ctor`
literals: `LEDGE_BUFFER_STEPS` 2 is coyote time, and `HEAD_BUMP_STEPS` 3 locks
out the jump after a ceiling. `ATTACK_QUEUE_STEPS` 5 is the third, and the nail
now buffers its press the way the jump and dash already did. That buffer is
shorter than a dash, so a swing pressed during one is genuinely lost, which a
test pins so nobody later "fixes" it into lasting longer.

`AttackParams` picked up the same `ZERO` treatment `Params` got, for the same
reason: the enemy nails and the test fixtures set five fields out of seven.

Verification: 86 hk-sim tests, and the 8 routes plus the 4 power-cut points on
the final CUE. `town-shade` had to move its expected `HK_HEALTH` from 2 to 3:
the same tape now kills the Shade sooner because a swing pressed a few ticks
early lands instead of being dropped, so the fight costs one mask less. It
stays an exact value rather than a range, so a later regression still shows.

A note worth keeping. Gating the wall probe on the Claw was not a micro
optimisation. Before it was gated, `kings-return` finished 6.8 units short of
where it had, with no change to any movement rule, purely because one extra
edge sweep per body per tick changed how much simulation fits between polls.
Build 120 puts it back exactly where build 116 left it. Per-tick cost is
observable in route positions, so a movement-neutral change is not automatically
route-neutral.

## P14 steps 5 to 8: the Dream Nail, the control map and the combinations (build 121)

The Dream Nail is not another nail swing, and the source keeps that clear by
putting it in three separate places. The Hero's `Dream Nail` PlayMaker FSM
drives the states; each state's length is its tk2d clip rather than a
serialized time, since every phase ends on `Tk2dPlayAnimationWithEvents`; and
the reward lives on the target's `EnemyDreamnailReaction` rather than on the
swing. `host/dream_nail.py` binds all three and asserts the shape it read, so a
later source change fails the cook instead of quietly shortening a phase.

DN Start 0.333 s, DN Charge 0.583 s, DN Slash Antic 0.417 s and DN Slash
0.556 s, which resample to 20, 35, 25 and 33 ticks. `CanDreamNail` wants both
feet on the ground, a velocity above -0.1, no dash running and no attack still
inside its recovery. Letting go during the charge cancels it; past the charge
the swing is committed and a release does nothing. Take Control runs from Start
to End, so the Knight has no input for the whole sequence, which the guest
handles through the same lock the Focus and the bench already use. The hitbox
is the `Dream Effects > Hitbox` polygon, live for exactly the Slash phase, and
`RecieveDreamImpact` pays 33 SOUL once per target.

That reward is real in the shipped scenes: 29 of the 45 carry
`EnemyDreamnailReaction`, so each actor now cooks a `dream_soul` and keeps a
`dream_taken` flag, matching the source's one-impact-per-enemy state.

Step 6, the control map. L1 dashes, R1 holds to charge the Crystal Heart,
Triangle holds for the Dream Nail, and both control screens say so. The replay
schema had only movement, jump, nail and Focus; `tools/validate.py` now knows
the whole pad, and `tools/route_probe.py` accepts l1, r1 and triangle and
drives the abilities natively, printing the dash, wall, super dash and Dream
Nail state every tick. A probe run confirms the dash moves 0.33333 units a tick
with gravity suspended, which is `DASH_SPEED` 20 over 60.

Step 7's combinations are hk-sim tests rather than routes, because the probe
proved the sim answers them in milliseconds where a tape costs a build. Wall
slide into wall jump into dash; a dash into a wall stopping flush rather than
tunnelling; a spent air dash coming back from a wall but not from the air;
mashing a button never beating its cooldown; and the wings surviving a wall
jump. One of those fixtures found its own bug: holding jump from before landing
does not re-fire on touchdown, because the source only starts the queue on a
fresh press. That is correct, and the test now says so.

Step 8 is where this package stops short and says so. Nothing in the admitted
scenes grants any of these five abilities, so each is reachable only through
its Cheats row, and no acquisition route exists to validate progression
against. The Cheats page is now ten toggles deep and wants paging rather than
tighter line spacing.

Also not done: the Wall Slide, Walljump, Dash, Double Jump and the four DN
clips all exist in the Knight's animation table and none are cooked, so an
ability currently plays the Fall or Idle clip. That is 33 frames for the
movement set and 26 for the Dream Nail against the 103 the Knight ships with,
so it wants the streamed treatment the Shade already uses rather than a bigger
per-room bank. Dream dialogue, the GENERIC line a target shows, is owed with
it.

Verification: 95 hk-sim tests plus the harness suites, the 8 routes and the 4
power-cut points on the final CUE.

## P15 first slice: Vengeful Spirit and the cast-versus-Focus button (build 122)

One spell lives in three FSMs and the extraction reads all three.
`Spell Control` on the Hero decides what is cast and pays for it, the spawned
`Fireball Cast` object aims and launches, and the projectile's own
`Fireball Control` and `damages_enemy` pair carry the damage, the lifetime and
the wall behaviour. `host/spells.py` binds them and asserts the shape, so a
changed source fails the cook rather than quietly shipping a different spell.

The numbers: 33 SOUL (`MP Cost`), 40 units a second, 15 damage (20 with Shaman
Stone, which needs charms), a 0.45 s flight before it dissipates on its own,
and a 2.611 by 2.125 box. The projectile passes through what it hits: its Idle
state listens only for the terrain layer, so the enemy's own invulnerability
window is what stops a second hit rather than the ball stopping.

The ordering caught something a re-implementation would get wrong. `Fireball 1`
plays the cast clip, spawns the ball and sends FINISHED in the same frame, and
`Fireball Recoil` then watches that same clip finish. So the ball leaves as the
cast starts, and the 0.3 s cast clip is the recoil, not a wind-up. The first
version here had the ball leaving at the end of the clip, which would have felt
sluggish and been wrong by 18 ticks.

Step 5's spell-versus-Focus priority is the interesting half. One button does
both: `Button Down` waits `Button Down Time` of 0.25 s for a release, and a
release inside that casts while continuing to hold starts a Focus. The port had
only the hold, so the Focus now begins 15 ticks later than it did, which is
what the source does. The SOUL is debited when the cast starts and a refused
cast takes nothing.

Not done in this slice, and recorded rather than implied. Desolate Dive and
Howling Wraiths are catalogued with their `quakeLevel` and `screamLevel` gates
and nothing more, since no admitted scene grants or needs them. Nail arts,
nail damage tiers, the centralized damage resolution of step 4 and the
verification matrix of step 7 are untouched. And the spell is invisible: the
`Ball` and `Ball End` clips are 4 and 3 frames, small enough for a resident
bank like the Lifeblood's 14, but they need a reserved VRAM rectangle in the
hand-allocated 320 to 384 column and a cooker to go with it.

One thing worth watching. `Player` is 80 bytes now, up from about 40 before
P14, because every ability's state lives on it. The 32 actors, the corpses, the
Shade and the Lifeblood bugs all embed one purely for its collision, and the
Lifeblood's own RAM budget assertion had to move from 224 to 384 bytes because
of it. The budget has 166 KB free so this is not urgent, but the next time that
assertion needs raising the answer is to split the hero-only fields out of
`Player` rather than to raise it again.

Verification: 26 test binaries green, the 8 routes and the 4 power-cut points
on the final CUE.

## P11 step 1, and why the ability clips do not fit (builds 123 to 125)

The physics catalog first, since it decides how much of P11 there is to write.
`tools/physics_catalog.py` walks all 45 admitted scenes and refuses to pass if a
family appears that the guest cannot represent, so it cannot go stale as more of
the map is admitted.

What it found. Terrain is 190 polygons, 400 boxes and 509 edge colliders, every
one of which the cooked segment model represents exactly; the only solid circles
anywhere are seven Baldur bodies on the enemy layer. There is no
`PlatformEffector2D` and no collider marked `usedByEffector` in any admitted
scene, so P11 step 2's directional one-way rules have nothing to preserve here.
The only mover is `LiftPlatform`, 25 of them in Crossroads_07, and reading its
`Update` shows it is not a moving platform at all: it sinks its two parts by
0.75 a second for 0.12 s to a floor of 0.09 units and holds. It cannot carry,
block or tunnel a rider, so step 3 has nothing to implement either. Contact
damage and hazard recovery already run through the cooked `DamageHero` volumes
and `HazardRespawnTrigger`s, which is also how acid gets its behaviour.

That leaves two real gaps, both measured rather than described. One
`BounceShroom` in Crossroads_38, which `pogo_sources` explicitly refuses as a
special bouncer; its hero side is `SHROOM_BOUNCE_VELOCITY` 25 and a
`shroomBouncing` flag that clears when the rise ends. And the nail's tink
surfaces: of 166, the static pogo pool already covers 39. Of the rest, 23 sit on
the Terrain layer outside the pogo layer policy, 68 have a PlayMaker FSM on the
owner so the object may move or vanish, and 36 keep their collider on a child
rather than the tink owner. Those are policy decisions belonging to
`pogo_sources`, so the catalog measures them rather than quietly widening them.

Both catalogs are tools, not build steps. `tools/abilities.py` and
`tools/physics_catalog.py` parse every admitted scene, which costs minutes, and
build speed is a recorded priority.

Then the ability animations, which turned into a budget measurement. The
Knight's clips ride in each region's own texture bank, and that bank is at its
limit: `TEXTURE_BUDGET` is 416 CLUT slots across four disjoint banks. Adding all
ten ability clips (68 frames) put the tightest region at 419. The four movement
clips alone (33 frames) put region 17 at 421. Dash, Wall Slide and Double Jump
(24 frames) still put it at 418. So region 17 carries about 19 free texture
slots and every useful clip set needs more than that.

The clips were reverted rather than trimmed to whatever squeaked under, because
picking clips by what fits is not a decision worth encoding. The two real
options are the streamed path the break effects already use, one chunk per
scene decoded at admission, or splitting region 17 so the per-region budget has
room. Until one of those happens, a dash plays the Fall clip. That is now a
budget item with a number against it rather than a vague omission.

## Every ability's source animation, through the Shade's route (build 126)

The previous entry left the ability clips as a budget item: a dash played the
Fall clip because the Knight's per-region bank is at its 416-slot CLUT limit and
the tightest region has about 19 free slots. The answer was already in the
repository. The Hollow Shade has the same problem, since it can appear in any
scene the Knight died in, and it solves it by keeping its frames in linked RAM
and reaching VRAM through the shared 64x64 animation slots. Nothing new is
reserved but a CLUT row.

`host/ability_art.py` cooks all ten clips that way: Dash, Wall Slide, Walljump
and Double Jump for P14, the four Dream Nail phases for its step 5, and the two
Fireball cast clips for P15. 68 original frames, one palette, 26,044 linked
bytes. The CLUT row comes out of the 14 rows `shade.py` reserved at y482 and
never used, four of which are now the ability block at (320,484,16,1);
docs/BUDGET.md carries the split. Linked headroom went from 152,500 to 121,780
bytes.

On the guest side `ability_pose` decides which clip owns the Knight's body this
frame, ordered as the source's own states are: a cast or a Dream Nail takes
control outright, then the dash, then the wall, then the wings. When one does,
its key replaces the room body frame in the animation working set and
`ability_art::draw` puts it on screen through the slot UV, mirrored by the
Knight's facing. The animation cache dispatches on the key range, so the Shade's
range is untouched.

Build 127 then closed the last two gaps the same way. The Crystal Heart's four
body poses (SD Charge Ground, SD Wall Charge, SD Dash and SD Hit Wall; its Fx,
Trail and Crys clips are effects rather than the Knight) and Vengeful Spirit's
own projectile clips, which come from the fireball's own sprite collection
rather than the Knight's and are reached through the Fireball Cast FSM rather
than by prefab name. 97 frames, two palettes, 36,190 linked bytes, and the spell
is no longer invisible: the ball carries its own animation key and is drawn at
its own position and facing.

Verification: 97 frames cooked across 16 clips, the 8 routes and the 4
power-cut points on the final CUE, with 111,540 bytes of linked headroom left.

## P10 step 1: what the admitted scenes actually script

P10 is the package the rest of the plan is waiting on. P12's secrets, P17's
economy, P18's NPCs and quests and P23's story all need a shared event and
variable runtime, and step 1 asks for the action inventory to be ranked by
required instances rather than raw count, because that ranking is what decides
which slices get built.

`tools/playmaker_inventory.py` walks every PlayMakerFSM in the 45 admitted
scenes. The size of the problem: **2,028 FSM instances across 200 distinct FSM
names, running 48,037 enabled actions of 280 distinct types.** About a third of
those instances (15,731) are operations the guest already has a native system
for, which is the ranking, not a completion claim.

The shape of the first slice falls out of the ordering. Four actions carry the
bulk of the declarative flow and all four are already native families:
`SendEventByName` 2,332, `Wait` 2,040, `BoolTest` 1,993 and
`Tk2dPlayAnimation` 1,731, each in 44 or 45 of the 45 scenes. Underneath them
sit the primitives with no native equivalent yet, led by `CallMethodProper`
1,271 in 34 scenes, which is the escape hatch into native code, and
`Trigger2dEvent` 1,026 in all 45, which is the interaction primitive every
room uses.

Two entries in that list matter more than their count suggests.
`GetLanguageString` 1,067 and `SetTextMeshProText` 1,064 appear in only 8 to 10
scenes, but they are the whole of dialogue and signage, so P18 cannot start
without them. And `SpawnObjectFromGlobalPool` 786 in 39 scenes is the pooled
object mechanism every projectile, effect and reward already goes through by
hand today.

By FSM name rather than action, the repeated behaviours are `FSM` 219 (the
unnamed default), `Spawn Offset` 157, `flyer_receive_direction_msg` 117,
`enemy_message` 112 and `Control` 110. `Music Control` and `PlayMaker Unity 2D`
appear in all 45 scenes. The ones a native controller already covers are
tagged: `damages_enemy` 84, `Geo Rock` 36, and the Hero's own Superdash, Spell
Control, Dream Nail and Nail Arts.

What this does not claim: state reachability is not analysed, so an action in
an unreachable state still counts; prefab FSMs reached only through a pooled
spawn are counted where a scene holds the prefab and not otherwise; and nothing
here implements anything. It is the authority step 1 asks for, with numbers
against every candidate.

## P10 steps 2 and 4: the IR, the executor and an honest coverage number

docs/SCRIPT_IR.md is step 2's decision, made against the inventory rather than
in the abstract. Three measurements shaped it. Instances outnumber definitions
ten to one (2,028 against 200), so a program is cooked once per definition and
bound per instance. 280 action types is far more than is needed, and the four
that carry the declarative flow are already native families, so the IR has a
small fixed opcode set plus one typed escape into native code rather than an
opcode per PlayMaker action. And a third of the instances belong to FSMs a
native controller already runs, which are tagged and never compiled.

`hk_sim::script` is the first slice of step 4: waits, bool and int sets,
`BoolTest`, `IntCompare` with a selector, synchronous `SendEventByName` to self,
`NextFrameEvent`, and `Native`. Eight tests cover the semantics that are easy to
get wrong. A send runs the target state inside the sending op, which is what
PlayMaker does and what step 5's note about the Runner's same-callback restart
depends on; a queue would turn it into an extra frame. A `NextFrameEvent` does
not fire in the tick that queued it. And every bound refuses rather than
dropping: a self-transition loop returns `Halt::Depth` instead of hanging, a
program with a bad index returns `Halt::BadProgram` instead of reading past
itself, and an overfull pending queue returns `Halt::PendingFull` instead of
losing an event, which is the failure mode that would be hardest to notice.

`host/script_ir.py` compiles PlayMaker FSMs to that IR and refuses, by name,
every action the slice does not implement. Running it over the admitted scenes
gives the number this package should be judged on rather than a claim:

**245 of 2,028 FSM instances compile, and they carry 0 ops.** Every one of them
is an FSM with no enabled actions. The first slice runs nothing real yet.

That is the point of the exercise, because the refusals rank what comes next
precisely, and the first two were answered immediately.

The variable cap blocked 295 instances. The distribution runs from zero to 107
slots, with 16 covering 85.5% of FSMs and 24 covering 95.9%, so a fixed array
wide enough for the tail would be mostly padding. Variables moved into a pool
the caller owns, with a span per definition, and the cap is gone.

`GetOwner` blocked 385, and once the variable cap stopped hiding it, 557. An
FSM's owner never changes, so it resolves at cook time into the object's stable
id and compiles to the `SetInt` the IR already had. The obstacle turned out to
be one layer down: `action_fields` decoded the compact scalars but not
`FsmGameObject` references, so the compiler could not see which variable slot
`storeGameObject` meant. Adding that one parameter kind unblocked all 557, and
it is opt-in for a reason worth recording. Decoding it by default read fine to
the four FSM extractors and broke the Runner recognizer in build 128, because
the actor recognizers match on the exact field set an action decodes to, so a
new field changes what they see. It is now a parameter the script compiler
passes and nothing else does.

The ranking after both: `Trigger2dEvent` 188, the interaction primitive every
room uses; `FindChild` 171, which wants the same cook-time resolution
`GetOwner` got, where the target is static; `GetVelocity2d` 122;
`Tk2dPlayAnimation` 115; and `PlayerDataBoolTest` 104, which is the progression
flag read that P18's dialogue conditions are built on.

The compiled count did not move, and that is not a failure of the slice: an FSM
compiles only when every one of its actions does, so unblocking an early action
just moves the refusal later.

The third blocker answered was `PlayerDataBoolTest`, at 104, and it is the one
that matters most for what comes after, because it is the progression-flag read
every dialogue condition in P18 is built on. The guest already had the store for
it: `persistent::Store` is a catalog of `StateId` to `i32` with save and load, so
a script reads and writes the same values the save file holds. The executor grew
`PlayerDataGet`, `PlayerDataSet` and `PlayerDataBoolTest`, and the closure the
tick used for native calls became a `Host` trait, so later slices can add
services without rewriting every call site.

Half of those 104 were blocked by something subtler than a missing opcode. The
field name is an `FsmString` with `useVariable` set, pointing at a declared FSM
variable whose serialized initial value is the actual name: Crossroads_47's
Grate Control reads a variable called "PlayerData bool name" holding
"openedCrossroads". Folding that is only safe if nothing writes the variable,
so the compiler counts how many enabled actions mention each name and folds
only the ones mentioned exactly once. A second mention might be the write. The
genuinely dynamic case, Crossroads_01's Activate Infected, whose field name is
set at runtime by another FSM and serializes empty, stays refused.

With that, the first authored behaviour actually compiles: 246 instances and 7
ops, where before every compiling FSM was empty.

What the remaining list says is unambiguous. `Trigger2dEvent` 198,
`FindChild` 176, `GetVelocity2d` 122, `Tk2dPlayAnimation` 115,
`SetMaterialColor` 92, `Collision2dEvent` 89, `GetParent` 83, `SetParent` 76,
`FindGameObject` 71, `ActivateAllChildren` 67 and `GetHero` 61. Every one of
them needs the script to *address an object*, and objects are not addressable
from a script yet. The next slice is not more opcodes; it is a guest object
model: what a script may refer to (the Knight, a region object, an actor, its
own owner), how a cooked stable id resolves to it, and what a native call may
do with one. That is step 5's scope and reference work, and it is the gate on
P12, P17, P18 and P23.

Nothing is cooked into the disc yet. This is the compiler and the executor with
a measured gap between them and the authored behaviour, which is what the next
slices close.

## P10: ranking by what completes an FSM, not by what refuses first

The blocker histogram was answering the wrong question. An FSM compiles only
when every one of its actions does, so a list of first refusals is a treadmill:
implementing the top entry moves the refusal one action later and the compiled
count does not move. `host/script_ir.py` now reports the whole set of actions
each FSM is still missing, and ranks by how many instances a single action would
complete.

That list is short and specific. `ActivateAllChildren` alone would complete 67
instances; `SendEventByName` to another FSM, 31; `ActivateGameObject`, 24;
`Trigger2dEvent`, 15. Underneath them the common missing sets are coherent
behaviours rather than scattered actions: 117 instances want the same six
physics actions, 112 want the four that make up enemy contact messaging, 92 want
the seven of a colour fade.

The 67 are worth naming, because they are one authored behaviour repeated. 64 of
them are `Activate Infected`, and the whole FSM is six states: wait a frame, read
a PlayerData bool, test a local bool, then activate or deactivate all children of
its owner. It is the Crossroads infection variant switch, and it is the reason
scene variants are a scripting problem rather than a cooking one. Whether
implementing it changes anything visible today depends on whether the authored
default state already matches what the FSM sets on its first frame, which is
worth checking before building for it.

The measured position is unchanged at 246 instances and 7 ops. What moved is the
map: the next slice is a guest object model, and this ranking says what it has to
support first, which is activation of a static object's children.

## P10: the denominator was wrong, and so was half my reason for thinking so

The 2,028 FSM instances included ones sitting on enemies the port already runs
through native Rust controllers, whose PlayMaker side is not script work at all.
Excluding them gives **1,879 script-side instances with 149 on native actors**,
and coverage moves from 12.1% to 13.1%.

The interesting part is what did not get excluded. The two candidates were
`flyer_receive_direction_msg` at 117 and `enemy_message` at 112, and only the
first partly qualifies: 35 of its instances sit on supported flyers, while 78
sit on Hatchers, which have a HealthManager but no native controller yet, and 4
on a Spitter with no HealthManager at all. `enemy_message` loses **none**. Every
one of its 112 instances is on a detector or trigger volume that forwards a
message *to* an enemy: `EnemyDetector` 82, `GO UP Message` 12, `Enemy Msg` 10,
and a few others. `ActorHealth` does not own those, so excluding them would have
been wrong. The overstatement was 149, not the 229 the two names suggested.

That distinction only came out because the exclusion is reported as a list
rather than a count, which is worth keeping: an exclusion you cannot inspect is
an exclusion you cannot check.

Two other fixes came with it. The survey now builds one `Scene` per scene and
reads the FSM list off that single parse instead of re-reading every typetree
afterwards, which halves the per-scene cost. And a scene that fails to parse now
lands in an `unparsed_scenes` key and leaves the denominator, where before it
silently produced an empty native-owner set and quietly reinstated the
overstatement for that scene. It is empty for all 45 today.

### An opcode that had already drifted

`host/script_ir.py` declared its opcode numbers under a comment claiming they
matched `hk_sim::script::Code`. They did not. Adding `NativeConst` to the Rust
enum shifted every later variant, so the compiler was emitting `PlayerDataGet`
as `NativeConst`, `PlayerDataSet` as `PlayerDataGet` and `PlayerDataBoolTest` as
`PlayerDataSet`. Nothing has cooked a script bank yet, so nothing caught it.

The numbers are part of the cooked format, so a comment asserting they match was
never going to hold. `script_ir.py` now reads the Rust enum's variant order on
import and refuses to run when it differs, and the enum carries a note saying
that adding a variant anywhere but the end changes every cooked bank.

### What to implement next, revised

`ActivateGameObject` (23 instances) and `ActivateAllChildren` (67) are the same
primitive, set a GameObject's active flag, over either one cooked object or a
parent's children. Ranking them apart as two entries makes them look like two
jobs. Together they are 90 of the 1,879 instances, 4.8 points of coverage, for
one native call plus a scope selector, and `script.rs` already has the passing
test for the constant-operand form they need.

The blocker is not the IR, and it is further away than it looks. Checking what
the bank actually carries: a region object's `state_id` is a persistence key for
the store, not a runtime active flag, and nothing in the renderer consults such
a thing. Scenery becomes draws at cook time. So `ActivateGameObject` needs the
renderer to be able to drop a draw at runtime, which is a P08 capability, and
`ActivateAllChildren` needs bank parentage on top of that.

That reframes the 90 instances. They are not 4.8 points of coverage waiting on
one compile arm; they are waiting on a rendering feature the port has not built.
Worth knowing before anyone writes the compile arm and finds the host has
nothing to call.

`SendEventByName` to another FSM (31 instances) is a different kind of work and
should not be compared like for like: the action already compiles to self, and
what those 31 need is the cross-FSM instance table from step 5. Architecture,
not an opcode.

## P15: the nail tiers and the two spells the port does not reach

The nail damage table is source-derived now rather than a Windows-override
guess. `nailDamage` indexed by `nailSmithUpgrades` is [5, 9, 13, 17, 21], and
the upgrades cost 250, 800, 2000 and 4000 Geo plus 0, 1, 2 and 3 Pale Ore.
`game/src/cheats.rs`'s hardcoded `PURE_NAIL_DAMAGE = 21` is confirmed correct:
the extractor asserts the Nailsmith's `Upgrade 4` against
`PlayerData::AddGGPlayerDataOverrides` and they agree.

Two things about where those numbers live are worth recording, because neither
is where the plan's text would lead you to look. The tier table is **not** in
PlayerData, which carries only the starting 5 and the Godhome override 21. It
is four int variables on the Nailsmith's `Conversation Control` FSM in
`Room_nailsmith`, which is not an admitted scene. And the Geo prices are not in
that FSM either: each upgrade state calls `GetLanguageString('Prices',
'NAIL_UPGRADE_N')` and converts it, so the costs come out of the encrypted
`EN_Prices` sheet. So a nail tier cannot be earned in the shipped slice, and
saying it is "source-derived" means the table is, not the acquisition.

Desolate Dive and Howling Wraiths are extracted but deliberately not
implemented. Both cost the same 33 MP as Vengeful Spirit, and the held
direction picks between them: up is Scream, down is Quake, neither is Fireball.

Quake, gated on `quakeLevel`, is a 0.25 s antic that launches +11.0 when
grounded and 0.0 when not, then holds -50.0 every frame until the fall ends at
`|Y Speed| <= 0.2`, so the dive has no fixed length, then a 0.75 s landing. It
damages 15 through `Q Fall Damage` on the way down and 20 through `Q Slam` for
0.10 s on impact. It spawns nothing: it activates children the Knight already
carries.

Scream, gated on `screamLevel`, is 0.60 s in four phases, damaging 13 through
three `Scr Heads` polygons for 0.40 s, and spawns a purely visual roar emitter.

Two extraction hazards came out of it. Two of the four cast phases do not get
their length from a clip at all: `Quake1 Down` plays a Loop clip and ends on
landing, and `Scream Burst 1` plays a LoopSection clip and ends on a Wait, so
reading a clip length there would have invented a duration. The extractor
asserts each clip's wrap mode so that a source change fails the cook instead.
And `Spell Control` serializes duplicate bool variables, `Can Cancel` and
`Is In Dream Focus` each twice, which a name-keyed variable map silently
reduces to the last one. That pattern is in `spells.py`, `superdash.py` and
`focus.py` too and is a live hazard in all of them, not something introduced
here.

## P11 step 4: the BounceShroom, and two things the brief had wrong (build 131)

The one bouncy target in the admitted world is implemented end to end: a
`KIND_SHROOM` region object carrying the trigger box, a `shroom_sources` cooker
that refuses any collider which is not an axis-aligned box (the guest carries
only bounds, so anything else would silently grow the target), a `shrooms()`
reader, and `Player::shroom_bounce` doing what `HeroController::ShroomBounce`
does: both air abilities back, `shroomBouncing` set, velocity replaced with
`SHROOM_BOUNCE_VELOCITY` 25.0, and the flag cleared when the rise ends.
`BOUNCE_SHROOM_TIME` is zero, so there is no hold, which the cooker asserts.

Two things I had wrong when I scoped it, both caught before the build.

**It is not a platform.** I assumed the Knight bounces by landing on it.
`HeroController::ShroomBounce` has exactly one caller in the whole assembly,
`NailSlash.OnTriggerEnter2D` in its down slash branch, and there is no
body-overlap path anywhere. So it is a pogo target with a different velocity,
not a trampoline, and implementing the landing version would have been invented
behaviour.

**It should not be there yet.** The single shroom is on "Fat Grub King" in
Crossroads_38, and that GameObject carries an FSM that reads the PlayerData bool
`fatGrubKing` and deactivates itself on the second frame when it is false. On a
fresh save the original removes the object; there is nothing to slash.

That decided how it shipped at the time, on reasoning that later turned out to
rest on a false premise. See the activation-gate entry below: the Fat Grub King
was never drawn, because it carries a `tk2dSprite` and the scenery cooker reads
only `SpriteRenderer`. The cook-time gate now removes the object and its shroom
with it, which is what the original does.

One port-specific divergence, commented where it lives: `shroom_bounce` clears
`jumping`, which the source does not. This port re-pins the jump hold's velocity
every tick, so an unfinished hold would eat the impulse. The ordinary pogo
already does the same thing for the same reason.

Also measured and deliberately ignored: `BounceShroom.active` is serialized
false on this instance, but that field only gates the shroom's own idle bob,
animation and particles. `NailSlash` checks only that the component exists, so
the bounce still fires.

### A test suite nothing ran

Verifying this turned up a regression of mine that nine builds had shipped.
Adding `ATTACK_RECOVERY_TIME` to `generated_params` for the attack queue work
broke a fixture in `tests/test_cook.py`, and nothing noticed because the build
never ran the host suite. It runs now, immediately after the Python environment
is ready, so a cooker regression fails in seconds rather than surviving ten
minutes of cooking and a clean route sweep. All 431 host tests pass.

That is the third time this session that a suite with no runner had quietly
rotted. The pattern is worth naming: a test that nothing executes is not a test.

## P11: the pogo debt was counting the wrong set (build 155)

The P11 row owed "the 127 of 166 TinkEffect surfaces the static pogo pool does
not cover". Re-measuring it found that none of that sentence survives contact
with the source, and the way it failed is worth keeping.

**`NailSlash` never reads `TinkEffect`.** Disassembling
`NailSlash::OnTriggerEnter2D` and resolving its `GetComponent` MethodSpec tokens
gives `NonBouncer`, `BigBouncer` and `BounceShroom`, and nothing else. The down
slash branch tests `other.gameObject.layer` against 11, 19 and 17, returns early
on an active `NonBouncer`, answers a `BigBouncer` with `BounceHigh` and a
`BounceShroom` with `ShroomBounce`, and otherwise falls through to a plain
`heroCtrl.Bounce()`. No component is required for the ordinary pogo.
`TinkEffect` is a separate behaviour: its own `OnTriggerEnter2D` answers a
collider tagged `Nail Attack` with a camera shake, a flash and a sound on a
0.25 s throttle, and never touches hero velocity. So the old ratio was the
overlap of two unrelated sets, and `pogo_sources` was never trying to cover
the tink population in the first place.

**Nothing is excluded on capacity.** "Static pogo pool" reads like an array that
filled up. There is no array. `world::pogo_targets` filters the streamed bank in
place and `enemies::pogo_contact` consumes that iterator, so a target costs no
`static`, no field of `world::State` or `frame::Game`, and no `.bss`; its only
stack is a fixed 16-point buffer that does not grow with the target count. The
128-per-scene bound in `pogo_sources` is a cooker assertion, and the busiest
scene uses 34 of it. "Static" there means not moving, not statically allocated.

**Measured at build 155:** 282 targets cooked from 281 objects, 161 refused.
The refusals are 128 objects a PlayMakerFSM owns, 18 with a moving
`Rigidbody2D`, and 15 `InfectedBurstLarge` blobs in Crossroads_22 whose only
collider is a circle. All three are modelling limits.

**Two bugs in the measuring tool, both of the kind that reports a true-looking
wrong reason.** `tink_coverage` matched targets by parsing an int out of
`Scene.sid`, which names the file an object was serialized in, so every object
an additive scene contributed was counted as excluded no matter what the cooker
did with it. And it re-derived the refusal policy instead of reading
`pogo_sources`' own output, in a different order, so six objects that refuse
first on having no collider of their own were reported as refusing on their FSM,
and its `excluded: other` bucket of 2 was hiding one such object and one
inactive one. It now reads the refusals the cooker actually emitted, which is
also one policy in one place rather than two that can drift.

What is still owed is the 128 FSM-owned targets. The source bounces off them
regardless of the FSM, so the refusal is the cooker's inability to tell a
cosmetic FSM from one that moves or removes the object, not a source rule. That
is `host/actors.py:717`, the `blockers` set in `pogo_sources`.

## P16 and P17 step 1: what the shipped slice actually contains

Both catalogs are extraction only, and both corrected an assumption in their
brief.

### Prices are not where they are serialized

`ShopItemStats::Awake` throws away the serialized `cost` and reparses it from
the `Prices` language sheet, which ships AES-256-ECB encrypted. **Nine of Sly's
fourteen items have a stale serialized cost.** The Lumafly Lantern serializes
1500 and sells for 1800, the Rancid Egg serializes 500 and sells for 60, the
Simple Key serializes 500 and sells for 950. Anyone who binds
`ShopItemStats.cost` ships wrong prices throughout. The catalog carries both
values and a `cost_is_serialized` flag on every row so the trap cannot be walked
into twice, and the sheet values match the retail game.

The same trap caught the stag toll: the Crossroads bell's FSM serializes
`Toll Cost` 80 and actually charges the sheet's 50.

### Dirtmouth's shop is a door, not a shop

`Sly_shop` in Town is the building front, and the shop is `Room_shop`, which is
not admitted. Same for `Stag_station`, whose interior is another unadmitted
scene. The station that *is* in the slice is Crossroads_47's, and its rules came
out cleanly: 50 Geo to ring the bell, and travel itself is free, which was
proven by the absence of any `TakeGeo` in `Stag Control` rather than assumed.
Every `ShopMenuStock` in the game is in an unadmitted scene, so no purchase is
reachable at all today.

### The economy loop, whole

1,177 Geo per clear of the admitted scenes (487 from 33 rocks, once, and 690
from 216 enemy payouts, farmable) against 230 Geo of things to spend it on:
Cornifer's map for 30, the stag bell for 50, and the City lift toll for 150.
That is the entire loop.

### Items

40 charms with notch costs and their four or five PlayerData fields each, 26
equipment entries, mask shards fusing four to one mask and vessel fragments
three to one vessel, all taken from the prefabs the pickups spawn rather than
from prose. Of that catalog the slice contains 6 charms, 1 notch, 3 mask shards,
1 vessel fragment, 3 relics, the City Crest and a Pale Ore. So the slice is one
shard short of a mask and two fragments short of a vessel, and none of it is
implemented: no pickup, no inventory, no charm, no notch exists in the port.

One find worth keeping: Tutorial_01's chest holds Fury of the Fallen, and a byte
scan of every level file shows `gotCharm_6` appears only there and in the menu
scene. In this install King's Pass is the only pickup for that charm.

### A sharp edge both agents hit

`action_fields` names unnamed array parameters by their index, so array-shaped
actions like `IntSwitch`'s compareTo and sendEvent pairs have to be read
positionally. Both catalogs worked around it locally. It is worth a named helper
before a third reader gets it wrong.

## P19/P20: the unadmitted roster, measured rather than listed

The ledger said 120 of 144 placed Crossroads actors were admitted and named
seven missing families. Both numbers needed correcting.
`host/remaining_actors.py` walks all 45 packed scenes, runs the real recognizer
and records why each placement is refused, per recognizer rather than per
object. There are **216 active HealthManager placements, 120 admitted and 96
refused**, and the refused ones fall into **twelve families, not seven**: Egg
Sac, Blocker, Giant Fly and Mawlek Body were not on the list at all. Of the 96,
24 sit inside a packed region envelope, which is where the ledger's 144 came
from, and 72 are Hatcher pool objects parked at world (100, 100).

The refusal reasons were not what the code says they are. `actor_sources` keeps
a single `movement_error` that every gate overwrites, so a Hatcher, an Egg Sac
and a Blocker all report "no single supported Crawler FSM", naming a recognizer
that was never a candidate for them. That is a design smell worth fixing at the
source: the field should be a per-recognizer list.

### Four placements are refused by the recognizer, not the controller

Verified by editing the parsed trees and re-running the real recognizer, not by
reading. A Zombie Runner in Crossroads_39 is refused only because it carries an
extra `enemy_corpse` FSM and the recognizer demands exactly one. A mirrored
Zombie Leaper in Crossroads_37 has a world basis of a plain x mirror, unit
magnitude and no rotation, and the recognizer refuses any non-identity basis
outright. Two Climbers carry their box collider twice, and `runner.body_box()`
already tolerates exactly that while `climber.recognize` calls `_one` instead.

The Climber pair carries a real risk the others do not: neither scene admits a
Climber today, so admitting them adds a whole art set to two scene banks, and
Crossroads_03 already sits near the 416-texture CLUT budget that defeated the
Knight ability clips earlier this session. That one gets measured before it
ships, and dropped if it does not fit.

### "Hatchers need runtime spawning" was the wrong description

It is not instantiation. The 68 babies are pre-placed pooled objects with their
own HealthManager and FSM, parked at (100, 100) under one cage per scene. A
Hatcher takes a random cage child, moves it, sends SPAWN, and on death the baby
reparents to the cage with its health reset. Nothing is ever created or
destroyed, so no allocator is needed. What it actually needs is cook-time
membership (the cooker selects actors by position, so it must follow a
Hatcher's cage rather than the babies' parked coordinates), a park and live
flag with on-death reset, and a pool budget answer: **Crossroads_22 would need
36 actors against the 32-slot per-scene cap**, so it needs a bounded sub-pool
like the Aspid's 8-slot shot pool, or a truncated cage recorded as a
divergence.

### Two placements the original hides

Zombie Myla carries `DeactivateIfPlayerdataFalse("hasSuperDash")`, so the
original hides her until the Crystal Heart, and a Zombie Runner in ShamanTemple
has `startInactive` set. Admitting either by relaxing a field would put an
enemy in a room the player should not yet see. That is the same class as the
Fat Grub King the BounceShroom work found, and the third instance this session
of the port showing something a PlayerData gate should be hiding. The pattern is
now large enough to name: the activation gap is not cosmetic, it is the port
disagreeing with the original about what exists.

## P18 step 1: who is actually in Dirtmouth, and what they say

27 NPCs across 12 of the 45 admitted scenes, with 154 dialogue keys decrypted
from 14 English sheets. There is no single dialogue sheet: every
`DialogueBox.StartConversation` names its own, so the extractor follows the call
rather than guessing a filename.

Dirtmouth authors ten NPCs and on a fresh save exactly **two** can be talked to.
Elderbug, ungated, who is the whole quest log in one FSM with 21 branches and 33
lines. And the Jiji door, which with no simple key says only "A stone door with
a simple lock." The other eight are all gated on progression the port cannot
reach: Tiso wants the dash, Nymm wants `nymmInTown`, the Gravedigger's FSMs go
inert without the Dream Nail, Zote destroys himself, and Elderbug has a second
Grimm-troupe variant that disables whichever of the pair does not apply.

Elderbug's opening is what a fresh save gets: `ELDERBUG_INTRO_NORMAL` then a
five-page `ELDERBUG_INTRO_MAIN`, and the FSM sets `metElderbug` as it plays.

### The semantic that matters more than the text

Dialogue selection is a **priority list, not a set of independent tests**, and
that was established from `FsmState::ActivateActions` in the shipped
PlayMaker.dll rather than assumed: it returns as soon as `Fsm.IsSwitchingState`,
so once one action transitions, the rest of that state never runs. Elderbug's 21
branches are ordered and each is a said-once flag, so evaluating them as
independent tests would pick the wrong line.

The script executor already behaved that way, but nothing pinned it. It has a
test now that names the reason, and docs/SCRIPT_IR.md cites the evidence. This
is the kind of rule that a later refactor breaks silently and no route notices.

### Honest gaps

Eight keys could not be resolved and are listed rather than guessed. Three are
built at runtime from an FSM variable, one is composed by `BuildString`, and one
is a genuine hole in the shipped English sheet: `Crossroads_04`'s inspect prompt
asks `EN_Prompts` for `MENDER_DOOR`, which that sheet does not contain. That is
a source bug, not a decode failure, and worth knowing before someone spends an
afternoon on the decryptor.

The new-save walker stops at the first action it cannot decide rather than
assuming an undecodable action does nothing, so three NPCs are reported
undecided instead of wrongly decided.

### One consolidation that came out of it

Three extractors had each grown their own copy of the language decryption: one
with the key as a literal, one deriving it from the assembly, and the third
importing the second's private helper. A literal key goes stale silently the day
the install changes. `host/language.py` now owns key derivation and sheet
decryption, all three callers dropped their copy, and the outputs are unchanged.

## Activation gates evaluated at cook time, and a templated-FSM scare measured

The port was showing objects the original deletes. `host/activation.py` now
evaluates the recognized PlayerData gates once, at cook time, against the
PlayerData a new save starts from, and `Scene.active` refuses a gated-off
object. Three gate shapes are recognized by structure rather than by name: the
`DeactivateIfPlayerdataFalse`/`True` components, and the two FSM templates
`deactivate_ifnot_playerdatabool` and `activate_if_pd_bool`. The FSM side is a
walker that follows the FSM from its start state with fresh-save values and
refuses anything it cannot fully decode, so an unfamiliar shape never removes
content.

**1,465 objects are gated off across the 45 scenes, but only 26 were authored
active**, so only 26 actually leave the world. The rest were already inactive.
The 26 are the Grimm troupe and Divine tents in Dirtmouth, the NPCs who are not
in town on a fresh save, Quirrel and Tiso at the lake, Zombie Myla, and the Fat
Grub King.

The verification is worth copying: rather than trusting a full cook, every
region of the 9 affected scenes was A/B cooked through the real cooker with
gates on and off, and the other 36 scenes were proved untouchable because every
gated object in them was already inactive. Town loses 117 draws across 8
regions, Crossroads_45 loses its one actor, Crossroads_38 loses its shroom, and
every other region is byte-identical.

### A correction to this ledger

An earlier entry justified shipping the BounceShroom ungated with "given the
object is on screen, a bounce target on it is closer to the original than an
inert one". **The object was never on screen.** The Fat Grub King carries a
`tk2dSprite` and the scenery cooker reads only `SpriteRenderer`, so its draw
count never moved. The premise was wrong, and so the conclusion it supported is
void: removing the shroom, which is what the gate now does, is simply correct.

### The templated-FSM scare

A `PlayMakerFSM` built from a template does not run its own serialized states.
`InitTemplate` rebuilds from the template and `OverrideVariableValues` applies
only the instance variables the template flags `showInInspector`. 209 of 462
FSMs in eight scenes are templated, so this looked like it could invalidate
every extractor that reads FSM variables, several of which feed the cook: Geo
Rock, Zombie Swipe, chaser and Bouncer Control are all templated and all differ
from their template.

Measured across all 45 scenes: **1,914 variables differ, 1,842 of them are
`showInInspector` so the instance wins and reading the instance is correct, and
the remaining 72 are all the same thing**, `npc_dream_dialogue`'s `Impact Pt`
and `Active Pt`, where only the reference's `m_FileID` differs and the path id
is identical. That is a cross-file reference fixup, not a value any cooker
reads.

So the existing extractors are right, and now there is a number behind that
rather than an assumption. The concern is real for anything that starts reading
a non-inspector variable, and `activation.py`'s `instantiate()` is the model for
doing it properly: 84 of the gate FSMs are templated, and reading their own
copies would have made that feature do nothing at all.

### Limitation, stated plainly

These gates are evaluated once, at cook time, against the PlayerData a new save
starts from. The original re-evaluates them on every scene load, so this is
equivalent only while no gate flag can change. Today none can, because the port
has no acquisition for any of them, which makes the cook-time answer exactly the
answer the original would give every time. The moment a save can set one of
these flags it stops being correct, and the gate needs the runtime script
executor plus a renderer that can drop a draw.

## Route coverage: one of three owed routes back, and why the other two are not

`crossroads-gate` restores the deepest of the three: resumed from the existing
Dirtmouth card, into the well, west along the Crossroads_01 shaft and out
through the side gate into Crossroads_07. It asserts the gate count and the
destination view rather than a position, and both of its jumps start from a wall
the Knight is already stopped against, so a few ticks either way change nothing.
That is the P14 lesson applied rather than restated. Nine routes pass.

**The King's Pass climb is built and verified, but only in the probe.** Spawn to
the Great Door, 22 breakables, no hazard contact, every jump starting from rest.
It does not reproduce on the disc, and the reason is the probe's own boundary:
it models no actors and no damage. On the real disc the Knight loses two masks
on the King's Pass floor, and the second hit's knockback lifts it over the very
step the route anchors on. It is recorded as a probe-only fixture rather than
committed as a route that would pass for the wrong reason. Landing it needs
either actors and damage in the probe or that stretch authored directly against
the emulator, which is tractable at about 30 seconds a replay.

**The Town to King's Pass gate pair is structurally blocked and that is worth
stating once properly.** The Town-side gate sits at y 43 to 55. The only card
fixture resumes at the Dirtmouth bench at y 11.39, Town's ground floor is y 10,
and the next floor up is a 22-unit wall against a 5.25-unit jump. There is no
path. The pair therefore needs either the King's Pass climb (whose Great Door
drops the Knight one unit from the gate, which is why the climb is the real
prerequisite) or a card saved in Town at that height, which nothing can
currently produce because the only bench in the admitted scenes is at y 11.

### A probe that lied, and a fall that misses VBlanks

Two findings worth more than the route.

The probe granted every ability, with a comment of mine claiming a route that
never presses L1, R1 or Triangle was unaffected. That is false: **a wall slide
and a wall jump need no button**, so any route pressing into a wall in mid-air
would slide or kick off in the probe and could never reproduce on the guest. A
greedy route search picked exactly such a jump before it was caught. The Claw is
now withheld by default and `--claw` grants it for deliberate wall authoring.

And the route deliberately stops on the Crossroads_07 entry ledge rather than
walking off it, because the 79-unit fall down that shaft makes the port miss
VBlanks: `HK_INPUT_MISSED_VBLANKS` reaches 2 as the fall crosses three region
bands. That is a real streaming hiccup on a fast multi-region drop, nothing
covers it, and the route stops short of it rather than dodging it silently.

Also recorded: probe tick maps to tape poll only with a memory card inserted,
since a cardless boot costs 400 extra polls, and the probe and the guest
disagree about at least one King's Pass breakable.

## P19/P20: the Egg Sac admitted, 123 of 215

The cheapest unadmitted family is in. It is the simplest actor in the game: no
FSM, no rigidbody, no recoil, no contact damage. It sits, loops one clip, takes
nail hits and dies. `ActorController::Static` is pure data with no state, and
recognition refuses anything that is not exactly that shape.

The interesting part was the corpse. Every existing corpse assumes a launch,
gravity and a landing, and this one has none of the three: it holds a `Death`
clip for 1.4 s then plays `Burst` where it stands. `CorpseSpec` grew a
`hold_ticks` that selects a non-physics form, with zero leaving every existing
corpse on the flung path. Both numbers come from the source rather than being
restated: the hold is the `Spit` state's `Wait` and the burst length is the
clip's own four frames at 18 fps.

Texture cost, measured against the real budget rather than the similarity
staging: the four regions holding the sac each gain exactly 12 textures, the
4 Idle plus 4 Death plus 4 Burst frames with no dedup, and Crossroads_50's
tightest region is 265 against the 416 budget. It fits with room to spare,
unlike the two Climbers.

Three things the contract had wrong, all corrected in `docs/EGG_SAC.md`. The
`initial_direction` should be -1 rather than 1, because the guest negates the
frame box by the facing so -1 is the unmirrored orientation. The claim that no
new cooked field was needed held for `ActorSpec` but not for `CorpseSpec`. And
the audio numbers in its limitations belonged to the actor, not the corpse.

One smell left alone deliberately: `Static { idle_clip }` duplicates
`ActorSpec.walk_clip`, since the cook binds walk, turn and idle to the same
clip. A unit variant reading `walk_clip`, as `Gruzzer` does, would be a field
lighter. It was built as specified rather than changed silently, and it is a
small edit on both sides whenever someone is in there anyway.

## P10 step 5: object references, and a measurement that had been lying

Every cook-time-resolvable reference action now resolves: `GetOwner` 820 of 820,
`GetHero` 177 of 178, `FindChild` 809 of 945, `GetParent` 183 of 376,
`FindGameObject` 68 of 202. The refusals are specific rather than blanket, the
largest being an object that arrives through a variable this cook cannot fix,
and tag lookups for persistent objects the port has no model of.

**The FSM count did not move: still 246 of 1,876.** Not one definition was
blocked solely by a reference action, so unlocking them completed nothing. That
is the honest headline, and it is why the compiler now also reports action
instances, which is the measure that actually moved: **8,380 of 42,166, up from
7,088.** A definition count moves in steps and hides work; the instance count
does not.

Two corrections to code I wrote. `GetOwner` was resolving to the FSM
*component's* path id rather than the owner GameObject's, which is neither what
the world metadata carries nor a node a transform walk can start from. And
`SendEventByName` names its event with an `FsmString` rather than the interned
`FsmEvent` every other action uses, so the compiler raised a `TypeError` that
the survey swallowed as an ordinary refusal: **all 2,286 instances had been
measured as uncompilable and no self-send had ever been emitted.** It never
crashed the survey outright only because every FSM containing one refused on an
earlier action first. 55 self-sends compile now.

### The resolution rule

A reference resolves from the *source* scene, never the cooked world, because an
object the port has not built yet is not an object the game did not have. A
`FindChild` that finds nothing stores `NO_OBJECT`, which is the null the
original stores, while an unreadable sibling refuses instead. Verified by
checking 70 resolutions in Tutorial_01 against the transform tree by name: 70
matched, 0 mismatched.

That exposed a contract problem worth fixing before anything depends on it.
docs/SCRIPT_IR.md said a native call handed `NO_OBJECT` "refuses rather than
acting on some default", which would make a correctly compiled `FindChild` trip
an assertion. PlayMaker treats a null target as a quiet no-op, so that is what
it means now; what a call must never do is fall back to some *other* object.

### What actually blocks the most

The sole-blocker ranking puts `ActivateAllChildren` (67), cross-FSM
`SendEventByName` (31) and `ActivateGameObject` (29) on top, but it hides the
real answer. **`Trigger2dEvent` is the first refusal for 274 FSM instances and
1,026 action instances**, far more than any of them, and with `Collision2dEvent`
it needs the same thing the cross-FSM send needs: a way for an event to reach a
script from outside. The executor had none at all. `Instance::receive` is that
path now, delivering synchronously exactly as a self-send does, and ignoring an
event the current state has no transition for, which is what PlayMaker does
rather than a silent drop.

Also corrected: `tools/playmaker_inventory.py` counted `ActivateGameObject` as a
family the guest already covers natively, which the no-runtime-active-flag
finding contradicts. That overstated step 1's covered share by 707 action
instances.

And `host/script_ir.py` now has tests. The read-only variable fold is the
subtlest code in the file, it substitutes a serialized initial value into an
action, and a wrong answer there is silent rather than a refusal.

## P18 first steps: somebody to talk to

Elderbug stands in Dirtmouth and answers UP with his fresh-save conversation.
He is the port's first NPC, and what makes him interesting is how little of him
needed inventing. His talk range is npc_control's own trigger, so it becomes the
cooked object's bounds. His lines are Conversation Control's entries, flattened
into panel pages in the order its states run them. His facing is not a guess
either: `Hero Is Right` and `Hero Is Left` compare against his own
`localScale.x`, which is +1.25, so the hero standing further right picks Talk
Right. Three clips, Idle and the two talk directions.

The art rides in the view's own room bank, appended during the postpass rather
than inside `cook()`. That is a cache decision, not an aesthetic one: cooking it
inside `cook()` would invalidate all 693 per-region caches for three frames.
Town's tightest view moves 187 to 204 textures against the 416 budget, exactly
one slot per frame.

Two deviations are deliberate and worth naming rather than burying. `metElderbug`
is written when the main entry *opens*, not when it finishes, because that is
where PlayMaker's `OnEnter` ordering puts it relative to the DialogueBox call
sitting beside it. And it is a session flag today, not a real PlayerData record,
which is honest for exactly one NPC and stops being honest at two.

The panel itself is now shared with the tablet reader rather than duplicated,
and the interlock runs both ways: a tablet cannot open behind a conversation and
a conversation cannot open behind a tablet.

### A guard that fired on the wrong thing

`tests/test_route_probe` failed five ways after this landed, and the first
diagnosis offered was wrong. The cause was not the probe's includes. The probe
checks every cooked pack against `.hkpsx/selected-regions.json`, which sounds
like the cook report but is a copy of it frozen at the last *guest build*. So
any cook since then, including this one, makes the guard fire on staleness
rather than on the half-written pack it exists to catch. Measured directly: the
live `data/regions.json` matched every pack on disk, the frozen copy disagreed
about 56 of them.

This mattered more than a failing test because the build now runs the host suite
before the cook, so the suite was aborting the build ahead of the cook that
would have refreshed the copy. The probe reads the live report now.

## The texture budget, measured where it actually binds

Three separate pieces of art work were being sized against the wrong number, so
`tools/texture_headroom.py` now answers the question properly. The 416-slot CLUT
budget is per view, which means a scene's real answer is its worst view, and the
two questions people ask have very different answers.

Art that stands in one place is measured against its own scene. Town has 212
slots left, most Crossroads scenes have between 90 and 280, and Elderbug's three
frames cost 18, six per frame.

Art that has to exist in every view is measured against the tightest view
anywhere, and that is **3 slots**, set by Tutorial_01 view 47 sitting at 413 of
416. Three is small enough to settle a design question rather than inform one: a
pause-screen charm icon, a HUD element, anything resident, either goes through
the Hollow Shade route (frames in linked RAM reaching VRAM through shared
animation slots) or it does not go. Forty charm icons at six slots each would be
240, against a global three.

Two things follow that were not obvious before measuring. Four scenes cannot
hold a three-clip NPC at all: Tutorial_01, Crossroads_03, Crossroads_01 and
Crossroads_13 have 3, 10, 13 and 13. And Tutorial_01, the scene the game opens
in, is the constraint on every resident-art decision in the whole port.

## Three projections, three overshoots, and where the script wall is

The script runtime got three slices this session and **compiled FSM instances
stayed at 255 through all of them**. `Trigger2dEvent` moved action instances
from 8,380 to 8,850 and left 255. The dominance resolver moved answerable object
fields from 1,579 to 2,084, a 32% increase, and left 255. Of the 255, 245
compile to nothing, because `Spawn Offset`, `PlayMaker Unity 2D`,
`RespawnTriggerFSM` and the Shade markers are single empty states whose every
action compiles trivially. Ten have ops. Three can act.

The forward estimates were wrong in the same direction every time, and it is
worth naming the shape rather than the instances. The runtime active flag was
projected at 94 acting instances and measured at 7. The resolver was projected
to recover 333 references and measured 94. Its cross-action reach was projected
at 1,131 object fields and measured 505. Each one counted cases where the data
exists somewhere and reported them as cases where the data resolves into a
usable answer. Those are different questions and the gap between them is large
and consistent.

Where the wall actually is, measured: 6,792 object fields, 60.9% of all that go
through a variable, name a variable nothing in the reference set writes, or that
something outside it disturbs. A spawned object genuinely has no cook-time
identity, so `SpawnObjectFromGlobalPool` and `CreateObject` are correctly
refused for ever. And 20 of the targets that do resolve are `activate=true` on
objects `Scene.active` gated out of the cooked world, which no runtime
visibility flag can ever serve: the port can hide what it built and cannot
reveal what it never built. The classic `closed`/`open` door pair is half
unbuildable for exactly that reason.

The contrast is the thing to take from this. Everything that reached the disc
this session (the charm board, the shop transaction, Elderbug's conversation
chain, the False Knight's phase machine, the Egg Sac) was written natively
against extracted source numbers. The script runtime's one on-disc behaviour is
Town's `Area Resetter`.

One real bug came out of the resolver slice anyway, and it had nothing to do
with dominance: the opening run of actions was read out of `states[0]` rather
than out of the authored `startState`, so an FSM beginning anywhere else would
have been given the wrong state's writes. Every cooked FSM starts at index 0, so
nothing shipped wrong. That was luck.

## The conversation was stuck, and the port was not saying so

Generalising Elderbug turned up a bug in what had already shipped, which is the
kind of find that justifies the exercise. `Conversation Control` does not pick a
line and stop: the state that speaks also writes the PlayerData flag that
changes the next pick, so what a player hears is a chain. Elderbug's runs intro,
then history, then a generic line that writes nothing and therefore repeats
forever.

The port got the first link and no further. His `Convo Choice` derives `Is Steel
Soul Mode` from `permadeathMode` through `IntCompareToBool`, which the walker
could not decode, so every walk after `metElderbug` was set stopped dead. The
port was writing the met flag and then replaying the introduction, forever, with
nothing anywhere reporting a problem. `IntCompareToBool` is not a branch, it
stores a comparison into up to three bool variables, so decoding it was bounded,
and the walk runs on to History 1 now.

The other correction was to an assumption baked into the first NPC. Only
Elderbug's library names its talk clips `Talk Left` and `Talk Right`. Everyone
else has a single `Talk`, or `Talk L` and `Talk R`, or a seated pair. The triple
was Elderbug-shaped and is per NPC now.

Myla is the second admission, measured in the view she actually stands in rather
than in her scene's worst view: 223 of 416, against the 265 that the scene's
tightest view carries with no NPC in it. She has no route that reaches her,
because Crossroads_45 is entered only from Crossroads_14 and no card fixture
starts anywhere near it, so she is cook-verified and nothing more.

## A PlayMaker FSM runs on the PlayStation

Town's `Area Resetter` sits in `Detect` waiting for its `Trigger2dEvent` volume
at the mouth of the well, takes `TOUCH` to `Reset`, and writes `currentArea`.
That is a state machine read out of a Unity scene, compiled to the script IR,
cooked into a linked bank and executed on a 1994 console, with its trigger
entered by the hero body and its write landing in the persistent store. Two
routes walk it.

Getting `Trigger2dEvent` to compile at all needed an argument rather than a
lookup. Only 71 of its 1,026 instances name a `Player` tag, and not one of the
15 FSMs blocked solely on this action names any tag at all, so tags would have
unlocked nothing. A no-tag instance fires for whatever Unity's physics
delivered, and what physics delivers is what the project's Physics2D layer
matrix allows: layer 13 `Hero Detector` admits layer 9 `Player` and no other
named layer. That is the rule the engine applies, not a guess about what a room
contains. 470 compile, 56 by tag and 414 by the matrix, and the refusals stay
refusals: 309 volumes no Player collider can reach, 52 that store the colliding
object, 51 filtering on `Enemies`, 94 naming a child collider of the Knight
rather than its body, and 22 whose own object carries no collider, so Unity
never raises the callback for them at all.

The honest headline is not the 255 instances that compile. **245 of them compile
to nothing**: `Spawn Offset`, `PlayMaker Unity 2D`, `RespawnTriggerFSM` and the
Shade markers are single empty states, so every action in them compiles
trivially. Ten have ops and three can act. Seven of the remaining are `Area
Title Controller`, which is one state with no transitions, so its `DISPLAY` goes
nowhere. That is also what the source does, so it is cooked and reported
separately rather than counted as behaviour.

Cost: 2,565 linked bytes of bank and +6,896 bytes of static footprint, about 6%
of the headroom, for an executor and a module that had never been linked.

Owed, and named in the cooker's own report: a script's PlayerData write lives in
a session store and never reaches the memory card, so a flag a script sets is
lost on a quit where the source would have kept it.

### Watching before pinning

The behaviour the whole slice was built around, `Set Seen Focus Tablet` in
King's Pass, is not reached by any route. Its volume sits at y 27.98 to 37.9;
`kings-death` stays below y 25 and `kings-return` passes underneath at y 11.4.
Pinning it there, as the plan said to, would have asserted a script that cannot
fire. Adding the telemetry first and reading what ten routes actually observed
is what found the one that does.

## Two bugs the disc found that no test could

Neither of these had a symptom anywhere except on hardware.

**The card fixtures.** The save record grew from 56 bytes to 78 when the charm
board and the NPC conversation cursors joined it, so every committed card under
`tools/cards` read as Corrupt and the seven routes that boot from one stopped
resuming. That is correct behaviour, not a bug: a short record must never
half-decode into a save whose new fields would be invented. Regenerating by
replay is circular, though, because `bench-save` is the only route that writes a
card and it resumes from the fixture that needs regenerating.
`tools/migrate_cards.py` carries them forward instead, reading the layout out of
`save.rs` so it cannot drift, refusing any record whose own checksum does not
hold, and zeroing only fields genuinely zero for a save made before charms or
conversations existed.

The first attempt at that failed in the most expensive way available. It
rewrote each record correctly, checksum and all, and left the psx-mc container
header 16 bytes ahead of it saying 56. That header is what makes a read
self-describing, so the guest read back 56 bytes of a 78-byte record and
rejected it for being short. Eight routes failed with nothing to point at: the
records were valid the entire time.

**A loop nobody was running.** With the cards loading again, `kings-return` came
back 3.2 units short of where it had always stopped, with two masks more than it
should have had, while its kills, its hits, the Crawler's health and its
breakables were all identical. The cause was not damage. `charms::vitals` walked
all 40 charms on every `params()` call, and `params` is called several times a
tick by `hurt`, `maintain`, `new_vitals` and the enemy soul gain. That is enough
to push a frame past a VBlank boundary, which changes how many simulation ticks
fall between two input polls, which moves the route. An empty board returns
immediately now and the route is back to exactly 8135671.

This is the second time per-tick cost has shown up as a position rather than as
a frame time, after the wall probe moved the same route by 6.8 units. It is
worth saying plainly: on this port, route positions are a performance test.

And it was only findable because `HK_HEALTH` on that route had been pinned an
hour earlier, from a single observation, in a pass that added assertions to
three routes which were doing real combat and asserting none of it.
`crossroads-gate` kills an enemy, takes two hits and loses a mask; the enemy
runtime could have stopped working entirely and validate would have printed
PASS.

## The world persists (HKS5), and a journey proves it

Until now a reload reset the world. The False Knight survived only because two
top slots of the script reserve were borrowed for its arena, which a recook
that moved the script field list would silently drop; broken walls, mined Geo
rocks, the Lifeblood cocoon and revealed secrets were forgotten outright.

**The store.** `game/src/persist.rs` mirrors the source's two stores.
PlayerData is a `u32` of bools the port implements (`falseKnightDefeated`,
`falseKnightFirstPlop`, `hasDash`, `hasWalljump`, `hasDoubleJump`,
`hasSuperDash`, `hasShadowDash`, `hasDreamNail`; append-only bit order) and four
level bytes (`fireballLevel`, `quakeLevel`, `screamLevel`, reserved). SceneData
is a sorted list of `u32` items, kind (4 bits) : scene (10) : cooked state index
(10) : value (8), holding only what differs from the authored world: persistent
breakables (the cooker's full `PersistentBoolItem`, not `dontSave` or
`semiPersistent`), `GeoRock` `hitsLeft`, one-way secret masks, the cocoon and a
`Battle Scene`'s `Activated`. Capacity 128 items against 54 the admitted world
can produce; a full store refuses and counts (`HK_WORLD_OVERFLOW`), never evicts.
Breakables and rocks already had session stores, so they are copied in at the
bench and back out on load; masks, cocoon and arena write through at the event.
The 64-bit ids of `hk_sim::persistent` were not used: the guest has no table for
them and they cost three times the bytes per item.

**The record.** HKS5 keeps HKS4's 152 bytes of fields at the same offsets, then
the PlayerData (8), the item count (2), the items and the checksum: 166 bytes
for an untouched world, 4 more per item, 678 at most, read and written through
one BSS buffer rather than stack. HKS4 still loads, as its own fields plus the
authored world (which is what it always restored), and its two borrowed False
Knight slots move into the store on load; HKS1 to HKS3 are still refused. The
card fixtures stay on HKS4 on purpose, so every card route proves an old save
loads. `tools/migrate_cards.py`, `seed_card.py` and `validate_power_cut.py` read
a record's length from its psx-mc container now, because HKS5 is variable.
The power-cut check passes with an HKS4 copy beside an HKS5 one.

Cost: plain build 141,496 bytes free before the stack against 150,912 (9,416),
PGO 80,020 against 101,724 (21,704; the PGO image's code grew about twice as
much as the plain one, the largest single growth being `main` by about 4 KB of
re-inlined callees, and that is not investigated yet).
Gameplay fps on the frozen frontend, PGO: kings 24.492 to 24.309, crossroads
21.903 to 21.751 (about 0.7%); plain: 23.731 to 23.783 and 21.196 to 21.139.

**A gate type that never worked.** Leaving through a top gate enters the
destination through its bottom gate, and the port spawned the Knight on the
gate and let him fall straight back through the hole, into the scene he came
from, forever. `HeroController.<EnterScene>d__487` with GatePosition 3 places
him 3.0 units above the gate (IL 0x04b5), holds him through the 0.165 s and
0.2 s waits, drives him at SPEED_TO_ENTER_SCENE_HOR 6.0 and _UP 9.4 for
TIME_TO_ENTER_SCENE_BOT 0.1 s, then releases him to gravity with that
horizontal speed and no input until he lands (`great_door::begin_bottom_entry`).
The gate's alwaysEnterLeft/Right flags are not cooked; he keeps his facing.

**The journey.** Four route segments, `JOURNEY` in the driver, run in order on
one card, the first on none: `journey-kings` (new game, King's Pass, the Great
Door, a Dirtmouth bench save of six persistent walls), `journey-crossroads`
(nine scene gates to the Crossroads_47 stag bench), `journey-false-knight`
(Crossroads_03, 21 and 10 to the arena, the fight, back to the bench, a save
with the arena's `Activated` and the two boss bools) and `journey-reload` (the
same way back in: nothing triggers, nothing drops, the gates quick-open). All
26 routes pass on the plain and PGO builds with zero faults.

What it is and is not, plainly. **Invincibility carries all the travel.** The
King's Pass Crawlers' knockback throws an open-loop climb off its route (see
"Route coverage" above) and the Crossroads rooms are full of enemies, so the
segments turn the cheat on from the pause menu. It is turned off before the
arena, and `HK_FK_KILL_CHEATS` is the cheat bits on the tick the boss died,
pinned at 0: the kill is fair, the travel is not. The fight is a
`tools/boss_sim.py` search from this arrival (the right-hand approach, after a
scene load has re-seeded the barrel summoner from the scene id), not the
committed `boss-fight` tape. The route is long because the short ways are shut
on a fresh save in the source itself: Crossroads_33's `sliding_wall` needs
`crossroadsInfected`/`shamanPillar`, and Crossroads_03's two `Toll Gate`s wall
its top entry off. Every jump was searched in a native probe against the cooked
terrain and starts from a wall stop or a rest, so a few ticks either way change
nothing. The travel swings the nail only where a wall must break, because every
landed hit on an enemy recoils the Knight 0.69 units and moves the route.

For the next console burn: psx-mc's timing is still unconfirmed on silicon, and
HKS5 changes what it does. A continue now reads the chosen profile's card file
twice (the survey, then `save::load_world` after the menu), and a save with
items spans more 128-byte frames than HKS4's two, so the write takes longer and
the power-cut window grows with the list. Worth watching on hardware: a
continue from an HKS5 card, a save with a dozen items, and a pull during that save.

## Enemy behaviour against the original, measured (2026-10-09)

`tools/behaviour-parity` compiles the guest's `game/src/enemies.rs` natively over the cooked rooms, scene
banks and actor catalogue and compares it with per-frame traces of the original that `tools/hkref scenes`
records, in three ways. **Idle**: every enemy of 60 scenes left alone for 15 seconds, judged on the
envelope and speed of its motion. **Strike**: each distinct enemy struck through the game's own
`HealthManager.Hit` with the no-charm nail every half second, judged on hit points, hits to kill and recoil.
**Approach**: the Knight walks up to each enemy from 14 units, judged on whether it reacts and how far away.
A catalogue check (hit points, contact damage, recoil, flags, body box against the source records), a scan of
every placement alone for 40 seconds, a harassment run (Knight pacing and swinging past each placement for 50
seconds) and a check that the guest's column index over each room's edges answers every wall, ledge and sight
query as visiting all of them does (7.6 million queries over 943 rooms) run without an original.

Where it agrees: the idle family means are within 4 percent for Crawler (3.78 u/s against 3.79), Climber,
Runner, Zombie Shield, Mosquito, Acid Flyer and Moss Walker; all 64 strike series agree on hit points and on how
many hits kill; recoil displacement after a strike agrees (within a quarter, or half a unit) on 62 of 64; no placement
panics, falls out of its world or jumps across it under the scan and the harassment run.

Two causes found and fixed:

- A Climber's nearest-hit test compared two ray parameters by cross-multiplication, which reaches about 6e20 for
  a two-unit ray against a six-unit edge and wraps `i64`. On the thin blocks Crossroads_07 puts Tiktiks on, the
  wrap ranked the underside nearer than the top face, so four of its seven Climbers attached to the wrong side of
  the block and walked it backwards over a path 0.34 units too high. Hits are ranked by the Q16 ray fraction now.
- A dormant Husk Guard woke inside `Alert Range New` (17 units either side). `Dormant` answers only ATTACK ALERT, and
  `Wake` rescales `Attack Range` from its authored 16.29 wide to 10, so the original sleeps until the hero is
  within 8.1 units. On the Crossroads_48 approach it woke at 7.9 units where the port woke at 14; with the
  authored box both wake on the same frame. The cook proves the authored box too.

Known and left: `FSMActivator` is recorded and not run. Hatchers, Aspids, Gruzzers, Moss Walkers, Acid Flyers and the
Fat Flies start with their FSMs disabled until an `ActiveRegion` trigger overlaps them, so the original's Hatchers
sit still until the Knight is within roughly 20 units and the port's drift from the moment the room loads. The
guest also advances only the enemies within a view of the Knight's, where the original runs them all. Enemy
families the original has in these scenes with no controller here: Moss Charger, Mossman Runner, Mossman
Shaker and Fat Fly. One Zombie Shield on a ledge in Crossroads_15 turns at the edge
where the original stops and attacks downward.

## Required package completion record

For every validated package, add source contract/version, covered instances and
variants, implementation paths, remaining exceptions, commands actually run,
original-game evidence, guest/disc hashes where applicable, inspected output,
memory/performance effects, persistence/reset evidence and the next package.
