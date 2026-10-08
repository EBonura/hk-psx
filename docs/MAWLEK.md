# Brooding Mawlek

Crossroads_09's boss, running from its own serialized state machines. The
split follows the False Knight's (docs/FALSE_KNIGHT.md):

- `host/mawlek.py` admits the one placed `Mawlek Body` during the region cook
  and hands the guest `Alert Range New`, the box that wakes it.
- `host/mawlek_art.py` (postpass) holds `CONTRACT`, every number the fight
  runs, and checks each one against the installed source at cook time: the
  five FSMs (`Mawlek Control`, both `Mawlek Arm Control`s, `Mawlek Head`,
  `nail_clash_tink`, the corpse's `corpse`) and `Battle Control`, pinned by
  structural digest as well, plus the Walker, the HealthManager and the
  `Shot Mawlek NoDrip` and `Corpse Egg Guardian` prefabs. It also cooks the art.
- `shared/hk-sim/src/mawlek.rs` is the controller. tests/test_mawlek.py holds
  `CONTRACT` against its constants without needing the source.
- `game/src/enemies.rs` `MawlekRuntime` binds it to the guest body.

## The fight

What the source does, and so what runs:

- **Lurk and wake.** The body sits 3.16 units behind the play plane with the
  Dummy playing `Dummy Lurk`. The hero entering `Alert Range New` sends START
  to `Battle Scene` (the gates close), then `Wake Jump` launches it straight up
  at 53 with gravity 0 for 0.1 s, `Wake In Air` sets gravity 3 and tweens it to
  the play plane over 0.5 s (drawn shrunk by the camera distance while it is
  behind), `Wake Land` shakes and raises the MAWLEK boss title, it roars for
  2 s, and `Music` applies `EnemyBattle`. `Wake Roar` sends ROAR ENTER to the
  hero's `Roar Lock` and `Roar End` sends ROAR EXIT, so the Knight takes no
  input but Start while it roars.
- **Walk.** `Idle` waits 2 to 3 s while the Walker walks it at 3 units a
  second for 1 to 3 s and pauses 1 to 3 s, turning at walls and ledges but
  never mirroring (`preventScaleChange`).
- **Super attacks.** After `Idle` it stops and waits for both arms and the Head
  to finish what they are doing, hides the body and plays the whole-body art
  on the Dummy. 50/50, with a switch forced after more than three of one kind
  in a row:
  - *Spit*: `Dummy Shoot Antic`, then 25 shots at 32 to 35 units a second
    between 92 and 105 degrees (75 to 88 when the hero is right), then 1.75 s
    of cooldown.
  - *Leap*: jump at the hero with x speed 1.25 times the gap and y 68, land,
    wait 0.5 s, jump back towards where it woke, then 0.25 s of cooldown.
  - A quarter of attacks (`Repeat Check`) go again at once; a repeated one
    never does.
- **Arms.** Each arm swipes whenever the hero is inside its own `Attack
  Range`: 49 ticks of antic, a 4-tick swipe with its DamageHero collider live,
  cooldown and a 0.15 s pause. The nail meeting a live swipe parries it once
  (`nail_clash_tink`): recoil, the clash sound and a shake.
- **Head.** Spits one shot at 27 units a second every 0.3 to 0.6 s plus its
  antic while awake; its box is a second nail target on the body's 300 hp.
- **Death.** The corpse spawns flung at 10 units a second away from the
  killing blow, BATTLE END ends the music over 2 s, `Init` 1.5 s, `Steam` 3 s,
  `Ready` 1 s, `Sting` plays Boss Defeat, `Blow` removes it. `Battle Control`
  writes `Activated` at BATTLE END and opens the gates 10.5 s later; a later
  visit finds the body destroyed and the gates open.
- **Heart Piece.** `Battle Control`'s `PrePause` finds the scene's Heart Piece
  by its tag and deactivates it; `End Wait`, 5.5 s after BATTLE END (the tick
  `Blow` lands, since the corpse's 1.5 + 3 + 1 s is the same 5.5 s), activates
  it, and a later visit's `Activate` shows it at once. It is an ordinary touch
  pickup (host/pickups.py `after_arena`), a mask shard saved as persist kind
  10 like every other.

## Art

All 91 sprites the body, Dummy, arms, Head, shot, shot impact, the turned
`Spit Effect` and the corpse's `Dummy Roar` use are cooked at the size the
actor path projects to, through host/false_knight_art.py's decomposition.
Every one fits Crossroads_09's spare texture pages (12 of 19 used after), so
nothing streams and the scene arena does not grow. Each part's box is
relative to the child it hangs off; `Mawlek Arm L` is the right arm's art
mirrored.

## Audio

- `EnemyBattle` (the OST's Decisive Battle, per the HK wiki) is CD-DA track 4
  and Boss Defeat track 5, played once (host/area_music.py). Boss Defeat is
  25 s of stereo music, which no scene bank has room for.
- Fifteen one-shots in Crossroads_09's scene bank (host/hk-cook/src/scene_sfx.rs): the
  fight's twelve plus `boss_final_hit` and the two gate clips, shared rows
  with Crossroads_10. All admitted, 8,464 of 56,432 bytes left.

## Evidence and cost

- `mawlek-fight` (tools/tapes/mawlek-fight.route) boots a seeded card on the
  Crossroads_09 floor left of the arena and turns on invincibility and the max
  nail, so it proves the fight, not a fair kill or the way in. The way in is
  from Crossroads_36, past an Elder Baldur only the spell cheat can kill;
  Crossroads_33's side is `full_wall_left` until the arena is won. After the
  kill the Knight jumps up and right to the Heart Piece and takes the shard
  (`HK_SHOP_SHARDS` 1, `HK_WORLD_ITEMS` 2).
- Code: about 20 KB of the guest's code is the Mawlek (controller, runtime,
  draw, strike), and the shot pool grew from 8 to 32 slots so a whole spray is
  on screen at once. docs/BUDGET.md carries the bytes before the stack.

## Not done

- The blood, dust, rocks, jump trail and roar waves, and the corpse's flung
  chunks and orange globs.
- The Walker's Sweeps are two terrain segments rather than three-ray Sweeps,
  the swipe colliders are their polygons' bounding boxes, and the corpse stops
  sliding when it lands instead of running Box2D friction down.
- The boss camera lock: the authored `CameraLockArea`s stay as cooked; the
  `Boss Camera Lock` and `CamLock NB` swap is not reproduced.
