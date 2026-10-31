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

## Telemetry and proof

`HK_BREAK_COUNT` counts a switch hit as a break; `HK_WORLD_ITEMS` and `HK_WORLD_RESTORED` count its saved bit. Walks: `walk-crossroads-shortcut` (open the Crossroads_06 pillar from its east side, leave through it into Crossroads_33), the same with a save, power cycle and Continue in between, and `walk-fungus1-22-gate` (docs/ROUTES.md).

## Known simplifications

The gate opens when the bit is set, not `hit wait + delay_ticks` later, and neither the switch's `Switch Activated` sprite nor the pillar's shake and sink are drawn (tk2d sprite objects of this kind have no draws in the cooked rooms). Both are the second step.
