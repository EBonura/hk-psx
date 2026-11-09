# Hornet (Greenpath) and the Mothwing Cloak

Status, 2026-10-25. Her fight, her art, her greeting and the Cloak are in the default build. The fight runs in a guest module on
the original's FSMs; her art is a boss bank carried by Fungus1_04's scene (docs/BOSS-ART.md); the Cloak pickup waits for the
corpse's `HORNET LEAVE`. `HK_HORNET_ART=off` leaves her out of a build.

## What the original does

- `Fungus1_04` (level132) is the arena room. Its `BossLoader` (`SceneAdditiveLoadConditional`) additively loads
  `Fungus1_04_boss` (level133) while `hornet1Defeated` is false. Hornet herself is in that second scene, so the port reads the two
  as one room exactly as it does for `Crossroads_10` and `Crossroads_10_boss`.
- The Cloak is `Cloak Corpse/Shiny Item` (level132:5138, `Shiny Control`, variable `Dash Cloak`, `PD Bool Name hasDash`). Its
  `Dash` state writes `hasDash`, `canDash` and `corn_greenpathLeft`. `Cloak Corpse Control` (level132:5165) keeps the shiny inactive
  until it receives `HORNET LEAVE` (then `Fall Pause` 0.5 s, `Fall` 0.4 s, `Activate Item`), or until `Check PD` finds
  `hornet1Defeated` on a later visit.
- `Hornet Infected Knight Encounter` (level133, `Encounter` FSM) starts the fight: the Knight entering its trigger starts a
  conversation (`DialogueBox.StartConversation` with `HORNET_GREENPATH`, with `HeroController.RelinquishControl`), closes the battle
  gates (`BG CLOSE`), then sends `WAKE` and `BOSS HORNET`. `Hornet Boss 1` has 225 HP, a `Recoil` (speed 15, 0.15 s, no recoil up),
  and `ConstrainPosition` x 15.07 to 37.96. Her `Control` FSM has no death state: `Stun Control` counts hits and sends `STUN`; the
  kill is native.
- **`HORNET LEAVE` has one sender**: the corpse. `Hornet Boss 1`'s `EnemyDeathEffectsUninfected.corpsePrefab` is `Corpse Hornet 1`
  (sharedassets105.assets path 30), whose `Control` FSM writes `hornet1Defeated` in its first state, sends `HORNET KILLED`, flings
  the corpse, jumps, throws the needle, and ends by sending `HORNET LEAVE` to the Cloak Corpse. No FSM of either scene sends it.

## What the port has

- The road (Fungus1_22, 21, 04) is admitted: scene rows 60 to 62 in `host/quality.py` and `host/hk-cook/src/quality.rs`. The arena
  gate closed set is a u32 (Fungus1_04 has two gates). Fungus1_04's `Battle Gate (1)` is mirrored and scaled, so it is a collider
  with no art (`ART_MASK`). Fungus1_21's `Battle Gate Bone` is a different FSM variant on a non-terrain gate and is dropped.
- The boss tier of the HKFSM03 kernel (docs/FSM-V2-DESIGN.md 2.4): 70 handlers on a table of scene objects, the fight's 18 FSMs and
  the corpse's FSM cooked from the Steam files in the build (`cook3::cook_hornet3`), and an oracle replay against the original
  (3,637 observations, three known differences ratcheted in `shared/hk-sim/tests/fsm3_hornet.rs`).
- The fight adapter (`shared/hk-sim/src/hornet_fight.rs`): the body against the terrain, the trigger volumes, the sprite clocks and
  frame triggers, the HealthManager (225 points, `TOOK DAMAGE`, `SetInvincible`), the Recoil, the DamageHero boxes, and the death:
  the corpse tree runs, writes `hornet1Defeated` and sends `HORNET LEAVE`.
- **Her greeting** (`HORNET_GREENPATH`, four pages of the Hornet sheet, cooked by `host/npc_dialogue.py cook_hornet` into
  `data/hornet_convo.rs`): the Encounter's `StartConversation` opens it on the NPC panel; Cross turns the page and closes after the
  last, Circle closes; closing sends `CONVO_FINISH` and the fight starts. The Knight is locked meanwhile (`HK_HERO_LOCK`).
- **The Cloak**: the corpse's `HORNET LEAVE` and the 0.9 s fall wake the shiny (`enemies::cloak_waiting`); UP takes it
  (`Grant::Ability(HAS_DASH)`, PlayerData bit 2, saved), and the room announces it with an item-get panel (`Mothwing Cloak`, the
  `GET_DASH_2` line and `Press R2 to dash forwards.`, cooked from the UI and Prompts sheets) until a button closes it. A room entered
  with the Cloak already taken shows nothing. Not reproduced: the Knight's kneel animation while the panel is up (the panel does not lock him).

## Where it lives (RAM)

Everything of hers is carried with her room; the resident image holds three small calls (`game/src/boss_hooks.rs`), the draw
pass's packet loop and the telemetry words. The resident set is two words (`modules::Mods`, 64 ids; it was a u32 and the build refused a thirty-third module or package
when Hornet's second module aliased the ability effects' package onto module 0, so the empty `arena` module was dropped). A
room's own set stays one word, so code and room art take the first 32 ids and the carried packages come after them.

| piece | where | bytes |
| --- | --- | ---: |
| `hornet`: the fight's tick, the boss tier's handlers, the fight adapter, the FSM action families it needs | code module, under the pool's first window (53,504) | 49,200 |
| `hornet_art`: the boss bank's part decode and queue, the draw pass over her objects, the greeting and item panels | code module | 14,456 |
| `art_hornet`: the state block (the fight, each object's world transform, the bank reader's index and queue), then her tree and the corpse's tree | data package | 15,976 |
| the boss bank: 70 chunks of 2 KiB, her 335 parts and 16 palettes | streamed textures of the scene | 143,360 of the scene's stream |

Her room needs 79,652 pool bytes and carries 18,432 more (the Knight's Death clip, the respawn clips and the ability effects): 98,084
of 98,304, 220 to spare. The pool did not move (base 0x801DAF00, 102,400 bytes). The boss-art branch (2026-10-18) moved it to
0x801D8000 so one window could hold a 65 KB module, which on today's main leaves 448 bytes free below it; this layout leaves 23,264 bytes free below the
pool (docs/BUDGET.md; main without her has 24,856). Modules do not call each other (the publisher refuses a cross-module
edge and lists it), so the fight's tick exports every visible object's composed world transform into the state block for the
drawing module to read, and her terrain is read through the actors' shared edge copy (`State::edge` would have kept 1,768 bytes of
resident code alive for her alone).

**The Shade in her arena** (a deviation). A room with her module, her trees, her state and the carried death art has no room for the
Shade's 30 KB art as well. While `hornet1Defeated` is clear no Shade is born in her room and none is carried into it
(`game/src/shade.rs waits_for_hornet`; the publisher counts her room's carried packages without it). A Shade recorded when the Knight
dies to her therefore waits, with its Geo, and appears on the first visit after she is beaten.

## Her art

The tree cooker's way (`hk-cook hornet-measure`: every clip any of her objects could play) asked for 835 parts, 496 KB and 16
pages against a view's 640 texture entries, 416 palettes and 256 KiB of stream. `hk-cook boss-measure` counted what her fights
play: 229 sprites, which the boss bank (docs/BOSS-ART.md) holds as 335 parts in 142 KB, 16 shared palettes and 70 chunks of 2 KiB
carried as streamed textures of Fungus1_04. It costs a view 70 texture entries, 16 palettes and no VRAM page, and the scene 143 KB
of its 256 KiB stream. A frame that needs many new parts takes the pad poll between uploads (`input::poll_only`), which is what kept
the first frames of her fight from missing a poll.

## Journeys

All three replay on the disc with zero faults, open loop on the pinned frontend and closed loop on the gate (`hornet-road.cuts`; the
fight reuses it, its first 8,365 polls are the road's).

- `hornet-road`: seeded at (24, 12) in Fungus1_22, climbs the 120 unit shaft, takes the top gate into Fungus1_21, its left gate into
  Fungus1_04 and walks into her trigger (region 1005, two gate loads). Her greeting opens and holds the Knight: 5 masks,
  `HK_HORNET_ENCOUNTERS` 1, no part dropped, the bank's 70 chunks found.
- `hornet-road-defeated`: the same walk on a card whose only difference is `hornet1Defeated`. The Knight takes the Cloak,
  `HK_WORLD_PLAYER` is 0x4000004 (bits 26 and 2) and the dash runs (`HK_ABILITY_DRAWN` 5 or more).
- `hornet-fight` (RETIRED from `validate`, 2026-10-31: its pins are deleted; the tape, route and cuts stay for a host simulation): the road, then her greeting turned through with four Crosses, the pause menu's Invincible and Pure Nail cheats (`HK_CHEATS` 3), a Square mash that killed her, the walk to the shiny, UP three times (the Cloak, bit 2) and the dash. It used two cheats, which the release route does not allow: a kill without them needs a Knight that dodges her, which an open-loop tape cannot do.
- `walk-hornet-fight` (docs/ROUTES.md) is the fair fight: closed loop on her raw state, nail only, no cheat, SOUL 0, then the seal, the open gates, the Cloak and the dash, all checked on game state. The road to her arena (`hornet-road`, `hornet-road-defeated`) stays as it was: she hurts a Knight who stands still (`HK_HORNET_HURT`).

## What blocks a walk from Greenpath

- Fungus1_22's lower hall (the arrival from Fungus1_30's `top1`) is sealed by a `Metal Gate` (level150:6741, x 21.3 to 26.3, y
  10.5). Its `Gate Switch` (level150:10065) is at y 13.4, above it: the original opens it from the shaft side, so the shaft is a one
  way shortcut. The journeys start from a seeded card.
- The other way into Fungus1_21 is Fungus1_32, reached from Fungus1_31's top. Fungus1_31 (33 views) and Fungus1_32 (16 views) are
  admitted (2026-11-02), and Fungus1_31 is the only way on from Greenpath west: its two Toll Gates (thin solid slabs at x 20.2 to
  20.7, y 49.9 to 53.9 and x 24.3 to 24.75, y 57 to 61) open only on the Toll Machines' broadcast after a 50 Geo payment
  (`Price String Name` STAGWAY_CROSSROADS, Prices sheet; machines at (10.5, 52.0) and (22.3, 59.2)). hkref confirms that in the
  original the Knight cannot pass the first gate unpaid (he lands against its face at x 19.96, the jump is capped by a ceiling 1.1
  above the slab), and hk-walkplan reaches the top exit only with both gates open. The port's Toll Machine (docs/WORLD_LINKS.md; `walk-toll-31-pay`, `-refuse`, `-reload`) takes the 50 Geo at the machine at (10.5, 52.0) or (22.3, 59.2) and removes both gates; a fair walk has to bring the Geo (all 53 source rocks are cooked since the per-scene Geo bank). The machines and both gates are drawn (Fungus1_31 tk2d idle sprites); the gates vanish with the toll bit. Fungus1_03
  (13 views) is not admitted.
- Fungus1_04's other exit, to Fungus1_25, is not admitted either, so nothing the Cloak opens in Greenpath can be walked to yet.

## Greenpath sightings (not ported; read of the source, 2026-11-06)

`hornetGreenpath` is the counter. Three objects write it, one script raises it: `Hornet Encounter GP1` (Fungus1_02, at 6.42, 61.49),
`GP2` (Fungus1_10, at 17.12, 13.46, scale x -1) and `GP3` (Fungus1_30, at 88.69, 23.68, scale x -1, layer 18, with a solid 1.08 x 1.94
box); Fungus1_31's `Hornet Encounter Control` sets it to 3 when lower (IntCompare, listed in `fsm3_scripts.rs` `INERT_ON_DISC`). Each object is
a tk2dSprite + tk2dSpriteAnimator + Rigidbody2D + AudioSource + the `Hornet Encounter` FSM, with an `Evade Range` child (a trigger box on layer
13, `FSM` Init/Detect/Enter/Exit that sets the parent's `Evade Range` bool); GP1 also has a `Turn Range` child and a `Grass` particle child.

| | Init destroys it when the counter is | Triggers | Sequence (values from the FSM) | Counter |
| --- | --- | --- | --- | --- |
| GP1 | above 0 (Remove Cam: also destroys `CameraLockArea H`) | Turn Range box 12.46 x 18.28 at offset (5.73, -0.27); Evade Range box 6.53 x 18.28 at (2.77, -0.27) | Idle; Turn Range: scale x -1, clip Turn, then clip Idle for up to 5 s; Evade Range: clip Evade Antic, then clip Evade at x velocity -25 for 3 s with a particle emit and a sound; after 5 s without Evade: turn back (scale +1, clip Turn), clip Run at -9 for 5 s with a sound | 1 on Turn |
| GP2 | above 1 | Evade Range box 11.81 x 11.38 at (5.40, 3.18) | Idle; Evade: clip Run at x -10 for 5 s, scale x +1, a sound | 2 on Evade |
| GP3 | above 2 | Evade Range box 12.68 x 13.33 at (2.80, 2.21) | Idle; Evade: counter 3, clip Jump Full at (7, 28) with a sound, wait for a collision (bottom hit), sound, clip Soft Land, zero velocity, clip Jump at y velocity 28 for 2 s with a sound | 3 on Evade |

All three are destroyed when the sequence ends, so a sighting is once per counter value. The arena (Fungus1_04) does not read the counter, so
nothing on the route to the fight changes; the sightings only add Hornet to three rooms.

What a port needs, smallest first: (1) `IntCompare` (and `GetPlayerDataInt`, `SetPlayerDataInt` on `hornetGreenpath`) in the light families, so
the Fungus1_31 script goes live and scene 63 leaves `INERT_ON_DISC`; the counter must be in the save record. (2) A trigger-driven actor with
no health: the Pigeon (`shared/hk-sim/src/pigeon.rs`) is the nearest precedent (a critter whose only contact is a trigger), or one more
`enemy_families.json` row on the HKFSM03 kernel with ops for SetVelocity2d, SetScale, NextFrameEvent, CheckCollisionSide (GP3 only), DestroySelf
and AudioPlaySimple. (3) Art: eight clips of the Hornet NPC collection (Idle, Turn, Run, Evade Antic, Evade, Jump Full, Soft Land, Jump) as an
actor or boss bank in three scenes; Fungus1_02's tightest view has 43 free CLUT slots (quality.py), 10 has 127 and 30 has 116, so the bank
must share the boss bank's 16 palettes and stay streamed. (4) The sounds, from the scene's sound bank. (5) A walk check per sighting.

It is a feature of its own (module code, a cook change that re-keys three scenes, a bank), not a data fix, and it adds RAM to rooms that
are already on the route; it belongs after hk-resident lands.
