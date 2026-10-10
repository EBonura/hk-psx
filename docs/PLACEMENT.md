# Placement parity against the original

What the original game places in each scene, against what the port spawns. The census is
`tools/placement-census` (a Rust crate with its own workspace, like `tools/parity-census`).

## Running it

```sh
cargo run --release --manifest-path tools/placement-census/Cargo.toml -- \
    --root . --survey <hkref survey run>... --out census.json --md census.md [--strict]
```

The survey is `hkref survey` (tools/hkref): the original run to every scene, 60 frames of settling, then
every `HealthManager` with its world position, activity and FSM states. The three runs of 2026-10-08
(`work/hk-ref-2026-10-08/runs/survey-{crossroads,greenpath,rest}`) cover all 501 scenes, so a census of the
60 cooked ones needs no new run of the original. The rest of the inputs are the build's own:
`data/regions.json`, the regions' `room.hk`, the packed metadata banks in `.hkpsx/world-metadata-packed`
(what the guest reads), the whole-world import in `.hkpsx/world-import`, and `data/actor_persistence.rs`.

## What it checks

Every enemy the original has active in a scene is `ok`, `snap`, `unadmitted` or `absent`:

- `ok`: the cook places it, a controller drives it and, for the gravity-bound families (Walker, Runner,
  Zombie Shield, Husk Guard), the guest's own spawn solver (`hk_sim::resolve_actor_spawn` then
  `Player::step`, run over the region's terrain) rests it where the original rests it, within
  `--snap-tol` (0.02 world units; every placement agrees to within a few thousandths).
- `snap`: placed and driven, but it floats, sinks, lands on another floor or never lands.
- `unadmitted`: the cook lists it and refuses it a controller. The refusal is the cook's own reason.
- `absent`: the cook does not list it.

Cooked actors the original does not have active are `extra`: refused by the cook (never spawned), a boss
placed dormant for its arena, or a difference in play (state-gated in the original, or unknown).

Each placement the port spawns is also held to contract checks, and each source scene to three more:

| check | what |
|---|---|
| bank | the packed bank carries an admitted object at the cooked position |
| hp | the cooked hit points are the original's (read after its own start-up scaling) |
| facing | for families that read the transform mirror, the bank's direction is the source scale's sign |
| scale | the cooked sprite scale is the source world scale times the tk2dSprite's own |
| persistence | an enemy whose `PersistentBoolItem` saves is in `data/actor_persistence.rs` |
| pool | a scene's admitted placements fit the guest's 32 actor slots |
| hazard | every `DamageHero` with an active collider is a cooked hazard |
| pickup | every chest, shiny, heart piece and vessel fragment is cooked, with each chest's own shiny |

`--strict` exits non-zero on any difference, so it can gate a build once the open list below is empty.

## Persistent enemy deaths

`HealthManager` hooks its object's `PersistentBoolItem`: a death writes the item
(`EnemyDeathEffects.RecieveDeathEvent` calls its `SaveState`), and a scene that loads with the item set
marks the enemy dead and deactivates it. Its identity is the owner's name and the scene, so placements
that share a name share a state. `semiPersistent` items (every Crossroads husk) are reset by a bench rest
and by the Knight's death (`GameManager.PlayerDead` calls `ResetSemiPersistentItems`). The soul totems are semi-persistent too, so one `persist::reset_semi_persistent` serves both resets.

`host/hk-cook/src/actor_persistence.rs` cooks which placements those are into `data/actor_persistence.rs`.
`game/src/actor_persistence.rs` keeps the deaths in `persist` (kind `Enemy`, local ids from 16 up, below
them the Blockers' own) so they ride the bench save. `enemies.rs` seats a placement dead when its state
says so, and a bench rest or a death clears the semi-persistent ones. Bosses and the Blocker keep the
deaths they already had (the arena's `Activated`, the terrain block).

Before this a husk killed in Crossroads_01 was back the next time the scene loaded.

## Open

See the placement census report for the current list. The families that need a controller, the Hatcher
cage that does not fit the 32 slots, and the story-state gates (`Activate Infected`) are behaviour and
capacity work, not placement.
