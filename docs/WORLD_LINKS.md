# Switches, gates and PlayerData observers

One engine system, driven by the cooked scene data, runs every nail switch and the gates it opens. A new room needs no authoring: the cook reads the source scene and either binds an object or says why not.

## What the original does

A `Switch Control` FSM sits on an object with a nail-hit trigger. A hit plays `Hit` (a wait), enters `Open` and sends `ACTIVATE` to the GameObject its `Target` variable points at. A switch that was hit before (its `PersistentBoolItem`, or the PlayerData bool its `Player Data` variable names when `SetPlayerData` is set) starts in `Activated` and sends `ACTIVATE AGAIN`, so the gate is open on load. The target carries a gate FSM (`Pillar Control`, `Gate`, `Gate Control`, `Mantis Gate` or `Toll Gate`) whose `ACTIVATE` path ends in `SetCollider` false or `DestroySelf`, after an animation of 0 to 0.75 s.

Separately, any collider object may carry `DeactivateIfPlayerdataTrue(bool)`, which removes it on load once the bool is set. Crossroads_33's `sliding_wall` is open for the Knight exactly when Crossroads_06's switch has set `shamanPillar`, in another scene.

## What the port does

`host/world_links.py` is a postpass over the finished region report (below the divider in `cook_inputs.txt`, so it recooks no region). For every cooked scene it:

1. Finds each `Switch Control` whose `Idle` waits for a `Nail Attack` trigger and whose `Open` sends `ACTIVATE` to a GameObject variable of its own scene.
2. Walks each target's FSM from `ACTIVATE` and binds it only if that path reaches `SetCollider` false (on the object itself) or `DestroySelf`. The seconds of `Wait` along the path are recorded as `delay_ticks`.
3. Gives the switch a scene state bit, `LEVER_TOP` (119) downwards past any index a breakable or secret uses. Secrets own 127 down to 120.
4. Binds every `DeactivateIfPlayerdataTrue` solid collider object whose bool is set by exactly one cooked switch to that switch's bit, whatever scene it is in (`state_scene`).
5. Writes `report['links']`, `report['link_observers']` and `report['link_policy']` (the counts, and what was refused and why).

`postpack_remote_art` (host/world.py) then, per view, adds the switch to the view's `levers` when its hit box is within the view's activation bounds plus 2 units, and a remote-art binding for every gate or observer whose collider edges or renderer draws that view holds.

`tools/world_metadata.py` encodes them with machinery that was already there:

* a `lever` is a bank `KIND_BREAKABLE` with flag 16 and flag 1 when its `PersistentBoolItem` saves. It carries the hit polygons and nothing else;
* a gate is a `KIND_REMOTE_ART` entry (off draws, edge indices) on the lever's state. One carrier per view and per state base, because an entry's state is a u16 added to the carrier's base: a binding to another scene's switch is a carrier whose base is that scene's range.

At run time (`game/src/world.rs`) the nail sets the bit through `State::strike` (flag 16: no debris, no particles, one `EnemyKillShake` as the source's `Hit` sends). Everything after that is the existing object-state path: `fill_edges` blanks the bound edges while the bit is set, `State::apply` hides the bound draws, `for_each_persistent_broken` and `restore_broken` save and restore the bit as a `Kind::Breakable` item (`scene * 128 + index`), so the gate survives a bench save, a power cycle and Continue. The closed gate's collision is the gate object's own collider, tagged by `edge_sources` in the room pack, never anonymous terrain.

## Vine platforms (the same system, the other way round)

A `VinePlatform` (Greenpath, 7 in Fungus1_06 alone) hangs from a vine. A nail hit on its `Vine` child (`VinePlatformCut`) drops it onto an `Activated Plat`, an inactive object that becomes solid for good (the platform's own `PersistentBoolItem`). The cook (`world_links.vine_records`) makes the vine's trigger a lever, binds the hanging platform's collider and sprites as an ordinary off binding, and binds the `Activated Plat` collider as an **inverted** binding: a carrier with flag 1 whose edges exist only while the bit is set (`remote_edges_blank` in game/src/world.rs). `world_links.bit_enabled` tells `host/cook.py` which inactive objects to bake: it bakes their colliders as terrain edges and forces their renderers to be draws, so the same bit shows the new platform and hides the old one.

`tools/inactive_census.py` lists every class of inactive solid collider in the source that the cook skips, with the vine platforms' `Activated Plat` marked. The classes with a persistent bit or an FSM that enables them are the next candidates (`Break Wall` in Crossroads_27, `Quaked Floor`, `Toll Gate` objects that start inactive); the rest are boss-arena props, corpses and colosseum platforms.

## Arena gates

`BG Control` gates are selected by FSM, not layer. Fungus1_21's `Battle Gate Bone` is on the Soft Terrain layer (25) and now bakes as a gate; because its `Battle Control` is not run by the port nothing would ever open it, so a Soft Terrain gate loads open (`battle_gates.survey`). `tools/arena_gate_census.py` prints every gate in the source and the verdict of the cook: 82 gates, 58 accepted (the family and the Bone variant, which differ only in tk2d clips), 24 refused (Deepnest 9, Archives 6, Ruins 4, Outskirts 2, Hive 2: FSM variants starting in `Init`, each needs its own recognizer; Abyss_19: a gate whose collider is not solid).

## Scenery packet budget (what the second landing hit)

Art for a linked object (the gate, the switch, a vine platform) is new draws in its view, and a view has a fixed packet reservation: `CAP` 1,032 less `ACTOR_RESERVE` 264 leaves 768 for scenery. The tk2d sprite objects were not drawn anywhere before (nothing is baked twice: Fungus1_22's view 947 had no draw of the Metal Gate or the Gate Switch), and the two draws took that view from 767 to 769.

The reservation used to count every cell of every draw's grid. The renderer (`game/src/scenery_geometry.rs`) drops each cell wholly off the 320x240 screen, so `host/scenery_geometry.py` now also bounds the cells a screen window can touch for an axis-aligned quad (`Packets.on_screen`; the 4,000 px background fogs fall from 64 packets to 6, a slanted layer keeps its whole grid). `mandatory_packets` sums the on-screen bound; `grid=True` gives the old figure. View 947 totals 653. A view that only fits by that bound keeps the limit in the budget table (`SCENERY_PACKET_BUDGETS`), so the renderer's optional-scissor allowance is 0 there and every other view's table value, and rendering, is unchanged. `tests/test_scenery_geometry.py` checks the bound against a port of the renderer's cell loop at random camera offsets.

The cook totals each view as it cooks (`regions.py`, workers included), prints `PACKETS region ...` for views within 64 packets of the limit and fails on the first one over; `generate()` prints the twelve tightest views in every build log.

## Toll Machines

A `Toll Machine` FSM (Fungus1_31 has two; Crossroads_49b's `Toll Machine Lift` has no gate in its scene and is left out) is an UP-to-inspect Yes/No box that takes Geo and then broadcasts `TOLL GATE OPEN` to every FSM of the scene (`Open Gates`, event target 4). Every `Toll Gate` answers by playing its open clip and destroying itself, and answers `TOLL GATE OPENED` (sent on load by a machine whose own persistent bool is set) by destroying itself at once. `world_links.toll_record` reads one record per scene: the machines (range box = the trigger collider UP is answered in, `Prompt Marker`, price = the `Prices` row named by the instance's `Price String Name`, question = the `Prompts` row named by `Prompt Convo`), the gates (the same `gate_target` the switches use, with `TOLL GATE OPEN` / `TOLL GATE OPENED`), and one persistent scene bit for the group, because every machine opens every gate. The bit is a lever object with flag 32 (the nail leaves it alone, `State::hit_object`); the gates are remote-art bindings on it, exactly the Metal Gate's. Nothing in the record is keyed on a scene or a name.

`host/tolls.py` writes `data/toll.rs` (the machines' scene, range, marker, price, the bit's state id, the question and the wait between Yes and the bit: the source's `Pause Before Box Drop`, 0.5 s; its `Box Disappear Anim` wait is disabled in the placed instances and the clip is not modelled). The guest half is `game/src/toll.rs`, the `toll` room module (host/code_modules.py: the scenes named by the `Machine { scene: N,` rows carry it; `modules::loaded(TOLL)` guards `frame::interact` and `dialogue::prepare`, both approved in host/module_edges.json): UP inside a range opens the box with the price beside the question, `Yes` is not selectable while the wallet is short, `Yes` takes the Geo (`geo::take`) and queues the bit through `State::toll_paid` (the lever wait slot, so leaving the scene flushes it), `No`/O closes. Once the bit is set, or being set, no machine of the scene answers. Telemetry: `HK_TOLL_OPENED`, `HK_TOLL_PAID`, `HK_TOLL_STEP` (0 closed, 1 box with Yes chosen, 2 with No chosen).

Not reproduced: `Take Control` (the hero is held by the panel, not walked to the machine), the hero's `Box_up` / `Box_disappear` clips, the machine's and the gates' art (the gates are invisible walls before and after, as every `tk2dSprite` object without a `tk2d_art` entry is; the draws would recook the scene's views), the coin sound.

## Adding a room

Nothing to author. Add the scene to the cooked catalogue, recook, and read `report['link_policy']` in `data/regions.json`:

* `switches` and `gates` are the pairs now live; `observers` the PlayerData-removed colliders bound to a switch;
* `refused_targets_by_family` counts the switch targets this system does not open: slide floors and doors (`Control`), `Lift Control`, `Stag Bell`, `Stop Stompers`, untyped `FSM` objects. Each is a different mechanism (a moving floor, a lift) and needs its own object kind;
* `refused_switches` names switches whose `Idle` is not the `Nail Attack` trigger (the `ReceivedDamage` levers of the Mines and Town);
* `observers_without_a_cooked_setter` counts `DeactivateIfPlayerdataTrue` colliders whose bool no cooked switch sets (`crossroadsInfected`, set by a cutscene). They keep the fresh-save answer of host/activation.py.

`tools/world_links_census.py` runs the same recognizers over every scene of the source, cooked or not, and prints what is left for later areas.

## What is left (tools/world_links_census.py over all 501 scenes, 2026-10-31)

62 `Switch Control` switches. The system runs 27 collider gates behind nail switches (Toll Gate 23, Gate 2, Pillar Control 1, Gate Control 1) and leaves out:

* 26 switches whose `Idle` is `ReceivedDamage` + an `IntSwitch` on the attack type (the Mines, Mantis Village and City levers, the Mantis Gates among them): they take nail and spell damage events through `damages_enemy`, not a `Nail Attack` trigger. Needs the `IntSwitch` compare values decoded from the raw action arrays (`action_fields` prints the field positions, not the values) and the spell hit path.
* 3 switches whose FSM has other states.
* 13 targets that are not collider gates: slide floors and doors (`Control`, 6), `Stop Stompers` (2), `Lift Control` (1), `Stag Bell` (1), `Cut Vine Plat` (1, a switch that targets a switch), one `Mantis Gate` with no solid collider, one `Gate` whose ACTIVATE path does not remove its collider.
* Lifts: 41 `Call Lever` and 16 `Control Lever` (elevators), and 2 `BridgeLever` (City bridges, a C# component): a different object kind each.

`DeactivateIfPlayerdataTrue` colliders: 6 objects on 4 bools; one is bound (`shamanPillar`). The others read bools no switch sets (`crossroadsInfected`, `hegemolDefeated`, ...) and keep the fresh-save answer.

## Telemetry and proof

`HK_BREAK_COUNT` counts a switch hit as a break (the camera's `EnemyKillShake`); `HK_WORLD_ITEMS` and `HK_WORLD_RESTORED` count the saved bits. Walks (docs/ROUTES.md, "Switches and gates"), all on the shipping disc, all the same ticks twice:

* `walk-crossroads-shortcut` (1,444 ticks): seeded at Crossroads_06's right-hand entry, along the tunnel (planned), left to the pillar and stopped by it, back to the Gate Switch, one swing, out through the left exit into Crossroads_33 and away from the spot the sliding wall used to wall in.
* `walk-shortcut-reload` (735 ticks): `Continue` from a card that holds the switch's save item (`tools/cards/shortcut-06-opened.mcd`, made with `seed_card.py --item 1:8:119:1`), then the same exit with nothing hit. A bench save after a hit is not walked: the only bench in reach is the Ancestral Mound's, and the climb from the switch back to the Mound's door is a chain of marginal hops that the planner's plans did not survive on the disc when joined to the walk of the tunnel (they pass alone from a seeded card). The write side is covered by the `a_switch_sets_its_bit...` test in `tests/world_runtime.rs`: the switch's id is among `for_each_persistent_broken`'s items, the same call every other persistent breakable is saved through (`walk-crest-save-reload` pins twelve of them).
* `walk-fungus1-22-gate` (959 ticks): seeded on the upper floor east of the Metal Gate, stands on the gate, jumps back, hits the Gate Switch and drops through the opening.

## Known simplifications

The gate opens when the bit is set, not `hit wait + delay_ticks` later, and neither the switch's `Switch Activated` sprite nor the pillar's shake and sink are drawn (tk2d sprite objects of this kind have no draws in the cooked rooms). Both are the second step.

## Gate arrivals (2026-11-02)

Where a gate puts the Knight is computed by `host/world.py resolve_gates`, from `HeroController.<EnterScene>d__487` (read with `host/inspect_il.py`): a left or right entry stands him at `FindGroundPointY` below the gate (x moved one unit in), and a door entry (GatePosition.door, a destination gate named `door*`) at `FindGroundPoint(enterGate.transform.position)`, the gate's own x, no `entryOffset`. Both are a ray of up to 10 units straight down onto layer 8 (terrain) plus the hero collider's half height less its offset plus 0.01 (1.4006). Top and bottom entries keep the authored point (the Knight falls, or climbs, from there). The door case was missing: the Ancestral Mound's exit arrived at Crossroads_06's door1 at the gate's own height, 0.73 inside the top platform, and the Knight settled in a sealed pocket under it (found on 2026-11-02 chaining the Mound to the shortcut; `walk-mound-exit` holds it). The cook now raises when a left, right or door entry has no ground within 10 units, and `tests/test_gate_arrivals.py` resolves every gate of `data/regions.json` and checks each standing arrival against its scene's terrain (it needs the retail files, like `test_doors.py`). Census on 2026-11-02: of 135 resolved gates (54 right, 55 left, 12 bottom, 13 top, 1 door), one standing entry was wrong (the Mound exit); the vertical ones are not standing arrivals by design.
