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
pass's packet loop and the telemetry words. The resident mask is a u32 and the build now refuses a thirty-third module or package
(Hornet's second module made it 33 for a build, which aliased the ability effects' package onto module 0; the empty `arena`
module was dropped).

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
- `hornet-fight`: the road, then her greeting turned through with four Crosses (the Knight is locked from poll 8,361 to 8,680), the
  pause menu's Invincible and Pure Nail cheats (`HK_CHEATS` 3), a Square mash with left and right sweeps that takes her through
  phase 1 (first hit at poll 9,177) to her death (9,789, phase 2: the corpse writes `hornet1Defeated`) and her leaving (9,989,
  phase 3), the walk to the shiny, UP three times (the Cloak, bit 2, at 10,116), the item-get panel closed and the dash both ways.
  **It uses two cheats**: a kill without them needs a Knight that dodges her, which an open-loop tape cannot do, and no fair-kill
  search exists for her as `tools/boss_sim.py` has for the False Knight. Her fight, her hurting the Knight and her stagger are shown
  by `hornet-road` as it stands (she hurts a Knight who stands still: `HK_HORNET_HURT`) and by the host tests.
  The tape is deterministic from the load into her room, and it depends on the polls the guest takes: redo it (the route string is
  `tools/tapes/hornet-fight.route`, `hkref tape gen` makes the tape) when code that moves polls changes.

## What blocks a walk from Greenpath

- Fungus1_22's lower hall (the arrival from Fungus1_30's `top1`) is sealed by a `Metal Gate` (level150:6741, x 21.3 to 26.3, y
  10.5). Its `Gate Switch` (level150:10065) is at y 13.4, above it: the original opens it from the shaft side, so the shaft is a one
  way shortcut. The journeys start from a seeded card.
- The other way into Fungus1_21 is Fungus1_32, reached from Fungus1_31's top. Fungus1_31 (33 views) and Fungus1_03 (13 views) are not
  admitted.
- Fungus1_04's other exit, to Fungus1_25, is not admitted either, so nothing the Cloak opens in Greenpath can be walked to yet.
