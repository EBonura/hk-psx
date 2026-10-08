# Egg Sac source contract

Nothing implements this yet. `host/remaining_actors.py` extracted the contract
below from `level81:4050`, the single placed Egg Sac in the packed catalog
(Crossroads_50, world position 117.71, 43.31, inside four packed region
envelopes). There is no second placement anywhere in the 45 packed scenes, so
one recognizer and one controller cover the whole family.

## Why actor_sources refuses it

`walker_control` raises `no single supported Crawler FSM`, and no other gate in
`actor_sources` selects a recognizer: the object carries no `Climber`, no
`Walker`, no `LineOfSightDetector` and no `PersonalObjectPool`, and its name
matches none of the `Buzzer`/`Fly`/`Roller`/`Spitter` prefixes. That refusal is
an accident of the dispatch order rather than a statement about the actor: the
Egg Sac has no FSM at all, so there is no movement to reject.

## What it is

A stationary destructible on layer 11 with an identity world basis. One enabled
`BoxCollider2D`, size 1.7711232900619507 by 1.8317594528198242, offset
-0.04640769958496094, -0.37363290786743164, edge radius 0, not a trigger. No
`Rigidbody2D`, no `Recoil`, no `DamageHero` and no `PlayMakerFSM`. It therefore
neither moves, nor recoils, nor touches the hero; the only interaction is the
nail.

`HealthManager`: hp 20, enemyType 0, no geo drops of any size, `invincible`,
`invincibleFromDirection`, `hasSpecialDeath`, `hasAlternateHitAnimation`,
`damageOverride`, `ignoreKillAll` and `megaFlingGeo` all zero. That set passes
every check `generated_actor_specs` already makes, so the existing
`ActorSpec`/`EnemyParams` path needs no new field for it.

`SetZ` puts it at z 0.006 with `dontRandomize` 0 and
`delayBeforeRandomizing` 0.5. `EnemyDreamnailReaction` pays the ordinary 33 SOUL
(`noSoul` 0, `startSuppressed` 0, convoTitle MINDLESS, convoAmount 1).
`PersistentBoolItem` is non-semi-persistent, so a killed sac stays dead across a
scene reload in the original; the port has no save state for that yet.

## Animation

Library `sharedassets81.assets:84`, animator enabled, `playAutomatically` true,
`isRealtime` false, default clip index 0.

| index | clip  | frames | fps | wrapMode | loopStart |
|-------|-------|--------|-----|----------|-----------|
| 0     | Idle  | 4      | 12  | 0 (loop) | 0         |
| 1     | Death | 4      | 12  | 1 (loop section) | 1 |
| 2     | Burst | 4      | 18  | 2 (once) | 0         |

Because `playAutomatically` is set and the default clip is Idle, the live object
loops Idle forever with no controller driving it. Nothing on the object plays
`Death` or `Burst`: the corpse prefab uses the same library object
(`sharedassets81.assets:84`) and plays them there, so the art is already shared
and the actor and its corpse cook from one clip set.

## Controller

There is no behaviour to port. The controller is "play Idle, do not move". The
cheapest shape is a new `ActorController` variant with no state at all (an
`ActorController::Static { idle_clip }`), reusing the existing walk-clip binding
for the idle loop and leaving `WalkParams` at zero speed with
`initial_direction` -1 and `random_start_direction` false. The body box and
health flow through the fields `generated_actor_specs` already emits.

`initial_direction` is -1, not 1: the guest draw negates the frame box by the
actor facing, so -1 is the unmirrored orientation (the same convention the
Crawler and the Buzzer use) and the world basis here is the identity.

## Death

`EnemyDeathEffects` on the sac: `corpsePrefab` `sharedassets81.assets:42`
(`Corpse Egg Sac`), `corpseFlingSpeed` 0, `corpseSpawnPoint` zero,
`rotateCorpse` 1, `effectOrigin` 0, -0.2, `enemyDeathType` 0,
`playerDataName` "EggSac", `doKillFreeze` 1.

`Corpse Egg Sac` is not the physics corpse `host/effects.py` recognizes: it has
no `Rigidbody2D`, no `ObjectBounce` and no `Corpse` component. It is a
`Transform`, renderer pair, `tk2dSprite`, `tk2dSpriteAnimator`, `SetZ`,
`AudioSource`, `PreInstantiateGameObject` and one `Control` FSM at scale 1, with
the same three clips. That FSM runs:

- `Init`: take the pre-instantiated child, place it on the corpse, set the
  child's `Shiny Control` FSM `Trinket Num` to 11 and `Fling On Start` to true,
  parent it to the corpse and deactivate it. It also reads the corpse's z
  rotation into `Angle Min`, adds 80 and stores `Angle Min + 20` as `Angle Max`.
- `Spit`: play `Death` and wait 1.4 seconds (scaled time), emitting a particle.
- `Burst`: send `EnemyKillShake`, play `Burst` to completion, activate the shiny
  and unparent it, play a one shot and stop the loop audio.
- `End`: deactivate the corpse.

So the drop is a Shiny, and the item it gives resolves through the ordinary
`Shiny Control` routing that `host/items.py` already walks from `Trinket Num`.
The corpse itself is a fixed 1.4 second `Death` hold followed by a four frame
`Burst`, with no physics, which is a `CorpseSpec`-shaped thing the existing
corpse path cannot express today (it assumes fling speed, gravity and a landing).

## Limitations a first implementation must state

- The corpse is a timed two clip sequence, not a flung body; either `CorpseSpec`
  gains a zero gravity "hold then play" form or the guest plays the two clips in
  place and removes the actor.
- The Shiny the corpse releases is not presented unless the item pass is wired
  to the corpse; `Angle Min`/`Angle Max` (owner z rotation plus 80, plus a
  further 20) are the fling arc the shiny would use.
- `PersistentBoolItem` is recorded, not honoured: a killed Egg Sac reappears on
  scene reload.
- The actor's `AudioSource` is `playOnAwake` with `Loop` true at volume 0.75 and
  pitch 1.53 (the corpse carries its own at volume 1 and pitch 1); the idle loop
  is not presented, and `AudioStop` in the corpse's `Burst` has nothing to stop
  in the port.
- `EnemyDreamnailReaction` soul is already generic; nothing Egg Sac specific is
  needed for it.
- The particle emitters (`Particle 1`, `Pt Death`) and the `Spit Point` child
  marker are not presented.

## What was built

`actors.egg_sac_control` recognizes the placement behind a name and "no FSM"
gate, matching the serialized component multiset, world basis, collider, clip
table and HealthManager flags exactly, because the family has one placement.
`effects._egg_sac_corpse_source` validates `Corpse Egg Sac` the same way and
reads the hold out of the `Spit` state's `Wait` (1.4 s, 84 ticks) and the burst
length out of the clip itself (four frames at 18 fps, 14 ticks) rather than
restating them. The Shiny's `Trinket Num` is read from the `Init` state and
carried as `shiny_trinket_num`; nothing spawns it yet.

`CorpseSpec` gained one field, `hold_ticks`. Non-zero selects the non-physics
form: `Corpse::spawn` never launches, `Corpse::tick` never touches terrain, the
corpse holds `air_clip` for `hold_ticks` and then plays `land_clip` until
`remove_after_land` removes it. Zero leaves every existing corpse on the flung
path, and the existing corpse tests cover that.

The claim that no new field was needed held for `ActorSpec` and `EnemyParams`,
not for `CorpseSpec`: the flung form assumes a launch, gravity and a landing,
and this corpse has none of the three.

One change is still outstanding in `host/cook.py`, which this pass could not
write. `append_actor_art` dispatches its clip bindings on the controller kind
and raises on an unknown one, so the region cook needs:

```python
        elif control['kind']=='EggSac':
            bindings={'walk':'Idle','turn':'Idle','idle':'Idle'}
```

next to the `Aspid` branch. Without it every Crossroads_50 region cook aborts.
