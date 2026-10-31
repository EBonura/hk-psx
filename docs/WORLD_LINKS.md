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
