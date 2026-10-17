# Hornet (Greenpath) and the Mothwing Cloak

Status, 2026-10-17. The Mothwing Cloak cannot be earned yet: Hornet 1 has to be beaten, and her fight runs on the host and in a
guest module that no build includes, because her art does not fit the room format (measured below). Everything else on the way is
in: the road to her arena, the native components, the original's FSMs on the boss tier of the kernel, the corpse that sends
`HORNET LEAVE`, and the Cloak pickup that waits for it.

## What the original does

- `Fungus1_04` (level132) is the arena room. Its `BossLoader` (`SceneAdditiveLoadConditional`) additively loads
  `Fungus1_04_boss` (level133) while `hornet1Defeated` is false. Hornet herself is in that second scene, so the port reads the two
  as one room exactly as it does for `Crossroads_10` and `Crossroads_10_boss`.
- The Cloak is `Cloak Corpse/Shiny Item` (level132:5138, `Shiny Control`, variable `Dash Cloak`, `PD Bool Name hasDash`). Its
  `Dash` state writes `hasDash`, `canDash` and `corn_greenpathLeft`. `Cloak Corpse Control` (level132:5165) keeps the shiny inactive
  until it receives `HORNET LEAVE` (then `Fall Pause` 0.5 s, `Fall` 0.4 s, `Activate Item`), or until `Check PD` finds
  `hornet1Defeated` on a later visit.
- `Hornet Infected Knight Encounter` (level133, `Encounter` FSM) starts the fight: the Knight entering its trigger starts a
  conversation (`DialogueBox.StartConversation`, with `HeroController.RelinquishControl`), closes the battle gates (`BG CLOSE`), then
  sends `WAKE` and `BOSS HORNET`. `Hornet Boss 1` has 225 HP, a `Recoil` (speed 15, 0.15 s, no recoil up), and
  `ConstrainPosition` x 15.07 to 37.96. Her `Control` FSM has no death state: `Stun Control` counts hits and sends `STUN`; the kill
  is native.
- **`HORNET LEAVE` has one sender**: the corpse. `Hornet Boss 1`'s `EnemyDeathEffectsUninfected.corpsePrefab` is `Corpse Hornet 1`
  (sharedassets105.assets path 30), whose `Control` FSM writes `hornet1Defeated` in its first state, sends `HORNET KILLED`, flings
  the corpse, jumps, throws the needle, and ends by sending `HORNET LEAVE` to the Cloak Corpse. No FSM of either scene sends it.

## What the port has

- The road (Fungus1_22, 21, 04) is admitted: scene rows 60 to 62 in `host/quality.py` and `host/hk-cook/src/quality.rs`, with the
  measured view layouts of their scene packs (32, 17 and 21 views); the catalogue marks them playable. The arena gate closed set is
  a u32 (Fungus1_04 has two gates). Fungus1_04's `Battle Gate (1)` is mirrored and scaled, so it is a collider with no art
  (`ART_MASK`). Fungus1_21's `Battle Gate Bone` is a different FSM variant on a non-terrain layer and is recognised and dropped.
- The Cloak Corpse shiny is a pickup (`host/pickups.py`): its grant is read from the shiny's own routing, it stays hidden until
  `hornet1Defeated` (`persist::HORNET_1_DEFEATED`, PlayerData bit 26) and, in the same visit as the fight, until the corpse has sent
  `HORNET LEAVE` and the 0.9 s fall is over (`enemies::cloak_waiting`); `Grant::Ability` sets the saved ability bit (`HAS_DASH`,
  bit 2) that `frame.rs` already reads for the dash.
- The boss tier of the HKFSM03 kernel (docs/FSM-V2-DESIGN.md 2.4): 70 handlers on a table of scene objects, the fight's 18 FSMs and
  the corpse's FSM cooked from the Steam files in the build (`cook3::cook_hornet3`), and an oracle replay against the original
  (3,637 observations, three known differences ratcheted in `shared/hk-sim/tests/fsm3_hornet.rs`).
- The fight adapter (`shared/hk-sim/src/hornet_fight.rs`, tests in `tests/hornet_fight.rs`): the body against the terrain, the
  trigger volumes, `CheckCollisionSide`, the sprite clocks and frame triggers, the HealthManager (225 points, `TOOK DAMAGE`,
  `SetInvincible`), the Recoil, the DamageHero boxes, and the death: the corpse tree runs, writes `hornet1Defeated` and sends
  `HORNET LEAVE`. On the host she visits 21 of her Control states in ninety seconds, covers the whole arena, hurts a Knight who
  stands still, and dies to a Knight who strikes her.
- The guest side (`game/src/enemies/hornet.rs`, the `hornet` feature): the fight ticks once a frame in the Fungus1_04 room module
  (`hornet`, 63 KB of the 64 KiB a module may be) over a 4 KB zeroed state block (`art_hornet`), with her gates, shakes, boss
  music, the Knight's lock during the encounter, and the Cloak wait. The build includes none of it unless the Hornet postpass
  (`hk_cook::hornet_scene`, opt-in with `HK_HORNET_ART=1`) has appended her art.

## Her art does not fit

`hk-cook hornet-measure` cooks her sprites the way the other bosses' are cooked and prints what the standard boss bank would take
(`fk_bank::plan`, with room for everything):

| | |
| --- | ---: |
| sprite libraries | 2 (her own, with 61 clips of which 35 are named by her trees; the Hornet NPC sheet, 22 of which 13 are) |
| unique sprites | 341 + 114 (the corpse's 5 NPC clips share the second set) |
| pixels | 1.34 M |
| parts (trimmed bands, one texture entry each) | 835 |
| texels | 496 KB, 16 scene pages |

What the format allows: 640 texture entries per room (`hk_format::MAX_TEXTURES`; the guest's per-texture tables in `render.rs` are sized by it, a
primitive template, two cores, a cover record and bit sets: some sixty bytes of resident BSS each), 416 palette slots per view (`TEXTURE_BUDGET`), and 20 pages per region. Fungus1_04's views
already hold 114 to 202 textures and up to 9 pages. So her art needs about three times the texture entries the room has left and
twice its palettes, before the sprite pixels (11 free pages hold 352 KB of the 496). The False Knight, the largest boss bank, is
99 sprites and 444 parts.

Options, none of them mine to take (each costs fidelity or an engine change):

1. Raise `MAX_TEXTURES` and the palette banks: roughly 26 KB of resident BSS for 1,100 textures, and the free RAM below the pool
   is 916 bytes.
2. A boss art bank loaded as its own region for the fight (the room already streams; a second table for one actor).
3. Stream her clips from disc a clip at a time into the animation slots (the CD is held by the boss music; the cheapest clip
   change is a seek).
4. Cut frames (stride 2 on the 12 to 20 fps clips) and share one palette per clip: about 180 sprites and 100 palettes, visibly
   coarser animation than the original.

## Journeys

Both replay on the disc with no cheat, no death and zero faults (`HK_VALIDATE_ONLY=hornet-road,hornet-road-defeated cargo
hk-build validate`).

- `hornet-road`: seeded at (24, 12) in Fungus1_22, climbs the 120 unit shaft, takes the top gate into Fungus1_21, its left gate into
  Fungus1_04 and walks to the Cloak Corpse (region 1005, two gate loads). `hornet1Defeated` is clear, so pressing UP at the shiny
  does nothing: `HK_WORLD_PLAYER` 0, `HK_ABILITY_DRAWN` 0.
- `hornet-road-defeated`: the same tape on a card whose only difference is `hornet1Defeated`. The Knight takes the Cloak,
  `HK_WORLD_PLAYER` is 0x4000004 (bits 26 and 2) and the dash runs (`HK_ABILITY_DRAWN` 5 or more).

## What blocks a walk from Greenpath

- Fungus1_22's lower hall (the arrival from Fungus1_30's `top1`) is sealed by a `Metal Gate` (level150:6741, x 21.3 to 26.3, y
  10.5). Its `Gate Switch` (level150:10065) is at y 13.4, above it: the original opens it from the shaft side, so the shaft is a one
  way shortcut. The planner confirms the hall has no way out; the journeys start from a seeded card.
- The other way into Fungus1_21 is Fungus1_32, reached from Fungus1_31's top. Fungus1_31 (33 views) and Fungus1_03 (13 views) are not
  admitted.
