# Mender Bug source contract

Nothing implements this yet. `host/remaining_actors.py` extracted the contract
below from `level37:5202`, the only Mender Bug in the packed catalog
(Crossroads_01, world position 48.8383903503418, 11.011002540588379). Read the
spawn gate first: in the original this object is present on about one room entry
in fifty, and only after an unrelated PlayerData flag is set.

## Why actor_sources refuses it

`walker_control` raises `Crawler outside the enemy layer: layer 19` on its first
check, because the GameObject sits on layer 19 rather than layer 11. That check
used to share a message with the bouncer one below it; the two are reported
apart now, because the layer is not what keeps this family out. Widening the
layer set to the three `NailSlash` gives its normal bounce on (11, 17, 19)
changes nothing here: the second check then refuses the Mender Bug on
`no single supported Crawler FSM`, since its FSM is `Mender Bug Ctrl`. No other
gate in `actor_sources` selects a recognizer: no `Climber`, no `Walker`, no
`LineOfSightDetector`, no `PersonalObjectPool`, and the name matches none of the
`Buzzer`/`Fly`/`Roller`/`Spitter` prefixes.

## What it is

An uninfected flavour creature, not a combatant. hp 1, enemyType 1, no geo drops,
no `Rigidbody2D`, no `Recoil`, no `DamageHero`: it cannot hurt the hero and it
dies to a single nail hit. `EnemyHitEffectsUninfected` and
`EnemyDeathEffectsUninfected` stand in for the infected pair.

The transform is not unit scale. Its world basis is
`[[0.8374961614608765, 0], [0, 0.8898497819900513]]`, which every existing
recognizer would refuse outright, so the admission path needs a non-unit scale
form before this family can enter at all. The one enabled `BoxCollider2D` is
1.1518195867538452 by 1.9342988729476929 at offset 0, -0.14642231166362762, edge
radius 0, and that is a local box: the world body is the box through the scale
above.

`EnemyDeathEffectsUninfected`: `corpsePrefab` `sharedassets37.assets:68`
(`Corpse Mender`, a conventional `Corpse` + `ObjectBounce` + `Rigidbody2D`
prefab), `corpseFlingSpeed` 0, `corpseSpawnPoint` zero, `rotateCorpse` 0,
`effectOrigin` zero, `enemyDeathType` 0, `playerDataName` "MenderBug",
`doKillFreeze` 1.

## Animation

Library `sharedassets37.assets:158`, `playAutomatically` true, `isRealtime`
false, default clip index 0.

| index | clip       | frames | fps | wrapMode |
|-------|------------|--------|-----|----------|
| 0     | Idle       | 3      | 12  | 0 (loop) |
| 1     | Fly        | 4      | 12  | 0 (loop) |
| 2     | Death Air  | 4      | 15  | 2 (once) |
| 3     | Death Land | 2      | 12  | 2 (once) |
| 4     | Startle    | 6      | 12  | 2 (once) |

## The FSM

One `PlayMakerFSM`, `Mender Bug Ctrl`, start state `Init`. Its only global
transition is `ZERO HP` to `Killed`.

- `Init`: `GetOwner`, then `NextFrameEvent FINISHED` to `Dead?`.
- `Dead?`: read PlayerData int `menderState`. Equal to or greater than 2 goes to
  `Destroy`; less than 2 goes to `Sign Broken?`.
- `Sign Broken?`: PlayerData bool `menderSignBroken`. False goes to `Destroy`;
  true falls through to `Chance`.
- `Chance`: set `menderSignBroken` false, draw `RandomInt` over 1 to 50
  inclusive, and compare against 50. Only 50 itself (and the unreachable
  greater-than branch) continues to `Idle`; anything less goes to `Destroy`.
  This is the one in fifty appearance rate.
- `Destroy`: the `DestroySelf` action is disabled; the enabled action is
  `ActivateGameObject(self, activate false)`.
- `Idle`: loop the object's own `AudioSource` at volume 1 and the child
  `Hammer`'s at volume 0.15, then wait for `HERO ENTER`.
- `Direction`: `CheckTargetDirection` against the hero. Hero to the right sends
  `RIGHT`, which goes to `Fly Left`; hero to the left sends `LEFT`, which goes
  to `Fly Right`. The bug always flees away from the hero.
- `Fly Left`: `Flight Vector` = (-15, 20, -30).
- `Fly Right`: `Flight Vector` = (15, 20, -30), then read the transform x scale,
  multiply by -1 and write it back, which mirrors the sprite.
- `Startle` (from either): stop the loop audio, destroy the `Hammer` child, play
  a one shot (`sharedassets37.assets` file id 4 path id 22 through audio player
  file id 5 path id 4126, pitch 1 to 1, volume 1), and play `Startle` to
  completion, which sends `FINISHED` to `Fly`.
- `Fly`: play the `Fly` clip, swap the `AudioSource` clip and play it, disable
  the collider (`SetCollider(active false)`), and run `iTweenMoveBy` with
  `vector` = `Flight Vector`, `time` 1.0, `delay` 0, `orientToPath` false,
  `realTime` false, `stopOnExit` true, `loopDontFinish` true, `finishEvent`
  `DESTROY`. The serialized `easeType`, `loopType`, `space` and `axis` are stored
  as enum ordinals 12, 0, 0 and 0; the enum names are not in the serialized data,
  so only the `space` ordinal is safe to read as `UnityEngine.Space.World`.
- `Killed`: set PlayerData int `menderState` to 2.

## The hero trigger

The `HERO ENTER` event comes from the child `Hero Detect`, which carries a
trigger `BoxCollider2D` of 22.229999542236328 by 7.468674659729004 at offset
0, 0.5658745169639587, with local scale 1.194035291671753, 1.123785138130188
and local position 0.02, 1.63, -8.494688034057617. The child scale cancels the
parent scale exactly, so the world trigger is
`[37.7401, 9.2930, 59.9701, 16.7617]`, 22.23 by 7.469 units centred on
48.855, 13.027. Its `hero_detect_region` FSM has `Enter Event` "HERO ENTER",
`Exit Event` and `Stay Event` empty, `Send to Parent` true and `Deparent` false,
so the enter event is forwarded to the parent once and nothing else is sent.

## Controller

The behaviour is three states: hold `Idle` until the hero box overlaps the
trigger, play the six frame `Startle` once (half a second at 12 fps), then move
by the flight vector over one second on an unspecified ease curve while looping
`Fly`, with the body collider disabled for the whole flight, and remove the
actor. Nothing in that path reads terrain, gravity or recoil.

## Limitations a first implementation must state

- The spawn gate is PlayerData. `menderState`, `menderSignBroken` and the one in
  fifty draw have no equivalent in the port, so an admitted Mender Bug would be
  present every time, which the original never does. Until PlayerData exists the
  honest choice is to keep it out, or to admit it with the gate recorded as an
  explicit divergence.
- `iTween`'s ease curve is an enum ordinal in the source, not a curve; ordinal 12
  is recorded but not resolved to a named easing, so a straight-line or
  linear-eased move is a substitution, not the source behaviour.
- The z component of the flight vector is -30, which in the original pushes the
  bug behind the scene during its escape. The guest draws actors on one plane, so
  that depth travel is not presented.
- The loop audio, the `Hammer` child audio at volume 0.15, the startle one shot
  and the audio clip swap on `Fly` are not presented.
- `Death Air` and `Death Land` belong to the `Corpse Mender` prefab, which is a
  conventional flung corpse; its parameters were not extracted here because the
  one hp actor is expected to be killed rarely and the corpse path is shared.
- The non-unit world scale (0.8375, 0.8898) has to be carried into the body box
  and the drawn sprite; every current recognizer refuses a non-identity basis.
