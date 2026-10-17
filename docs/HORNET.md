# Hornet (Greenpath) and the Mothwing Cloak

Status, 2026-10-15. The first progression blocker: the Mothwing Cloak cannot be
earned until Hornet 1 is fought and beaten. Branch `campaign/hk-hornet-2026-10-15`.

## What the original does

- `Fungus1_04` (level132) is the arena room. Its `BossLoader`
  (`SceneAdditiveLoadConditional`) additively loads `Fungus1_04_boss` (level133)
  while `hornet1Defeated` is false. Hornet herself is in that second scene, so the
  port reads the two as one room exactly as it does for `Crossroads_10` and
  `Crossroads_10_boss`.
- The Cloak is `Cloak Corpse/Shiny Item` (level132:5138, `Shiny Control`,
  variable `Dash Cloak`, `PD Bool Name hasDash`). Its `Dash` state writes
  `hasDash`, `canDash` and `corn_greenpathLeft`. `Cloak Corpse Control`
  (level132:5165) keeps the shiny inactive until it receives `HORNET LEAVE`, or
  until `Check PD` finds `hornet1Defeated` on a later visit.
- `Hornet Infected Knight Encounter` (level133:86, `Encounter` FSM) starts the
  fight: the Knight entering its trigger (x 24.1 to 32.3, y 25.9 to 47.3)
  starts a conversation, closes the battle gates (`BG CLOSE`), then sends `WAKE`
  and `BOSS HORNET`. `Hornet Boss 1` (level133:67) has 225 HP (hkref run
  `hornet1`, `actors.csv`; `Control=Inert` until the encounter wakes her).
- No FSM of either scene sends `HORNET LEAVE`: the defeat to Cloak chain goes
  through native code (HealthManager death), not yet traced.

## What the port has

- The road (Fungus1_22, 21, 04) is admitted: scene rows 60 to 62 in
  `host/quality.py` and `host/hk-cook/src/quality.rs`, with the measured view
  layouts of their scene packs (32, 17 and 21 views). The catalogue is now 1,013
  chunks, so `region_cache.rs` accepts four digit region names.
- The arena gate closed set is a u32 (Fungus1_04 has two gates, taking the
  catalogue to 17). Fungus1_04's `Battle Gate (1)` is mirrored and scaled, so it
  is a collider with no art (`ART_MASK`). Fungus1_21's `Battle Gate Bone` is a
  different FSM variant on a non-terrain layer and is recognised and dropped.
- The Cloak Corpse shiny is a pickup (`host/pickups.py`): its grant is read from
  the shiny's own routing, it stays hidden until `hornet1Defeated`
  (`persist::HORNET_1_DEFEATED`, PlayerData bit 26), and `Grant::Ability` sets the
  saved ability bit (`HAS_DASH`, bit 2) that `frame.rs` already reads for the dash.
  The corpse's own art stays in its initial pose.
- `Hornet Boss 1` is an unadmitted actor (no controller, no art, no sounds).
  Nothing sets `hornet1Defeated`, so the Cloak cannot be earned yet.

## Journeys

Both replay on the final disc with no cheat, no death and zero faults
(`HK_VALIDATE_ONLY=hornet-road,hornet-road-defeated cargo hk-build validate`).

- `hornet-road`: seeded at (24, 12) in Fungus1_22, climbs the 120 unit shaft, takes
  the top gate into Fungus1_21, its left gate into Fungus1_04 and walks to the
  Cloak Corpse (region 1005, two gate loads). `hornet1Defeated` is clear, so
  pressing UP at the shiny does nothing: `HK_WORLD_PLAYER` 0, `HK_ABILITY_DRAWN` 0.
- `hornet-road-defeated`: the same tape on a card whose only difference is
  `hornet1Defeated`. The Knight takes the Cloak, `HK_WORLD_PLAYER` is 0x4000004
  (bits 26 and 2) and the dash runs (`HK_ABILITY_DRAWN` 7).

The tape was authored by a best-first planner over the shared player step,
closed loop against the emulator (plan twelve moves, replay, re-plan from the
observed position). The fixture is seeded: the Knight does not get to (24, 12)
by walking, see below.

## What blocks a walk from Greenpath

- Fungus1_22's lower hall (the arrival from Fungus1_30's `top1`) is sealed by a
  `Metal Gate` (level150:6741, x 21.3 to 26.3, y 10.5). Its `Gate Switch`
  (level150:10065) is at y 13.4, above it: the original opens it from the
  shaft side, so the shaft is a one way shortcut. The planner confirms the hall
  has no way out. Walking Fungus1_30 to Fungus1_22 works (`HK_SCENE_GATE_LOADS`
  1, region 945); that replay reports ten missed input vblanks, as `greenpath-walk`
  does on the retained frontend used for this work, so it is not a registered route.
- The other way into Fungus1_21 is Fungus1_32, reached from Fungus1_31's top.
  Fungus1_31 (33 views) and Fungus1_03 (its mid height entrance, 13 views) are
  not admitted, and the planner finds no climb from Fungus1_31's mid entrance
  on the 2026-09-20 pack geometry (unexplained, maybe an ability or a macro the
  planner lacks).
- RAM is the wall. This build links with no byte free below the module pool. The
  70 new views cost about 3.4 KB of per-view and per-scene tables; the cost was
  paid by narrowing `REGION_SCENE_LOCAL` to two bytes and `SCENE_REGIONS` and
  `SCENE_ATLAS_RANGES` to u16 (2.4 KB). `REGION_SCENES` (1,012 bytes) could be
  replaced by 63 scene start indices (about 880 bytes) if `scene_of` is rewritten
  and the standalone world tests follow. Fungus1_03, 31 and 32 would need
  roughly 500 bytes more than that.
- On the retained frontend (not the pinned one) the Greenpath rooms run near 20
  fps: 2,318 of 2,352 frames over two vblanks in `hornet-road`, 840 of 841 in
  `greenpath-walk`. The `hornet-road` pacing ceilings are measured on that
  frontend; several older ceilings fail on it for the same reason.

## What running Hornet from her original FSM needs

Measured over the 30 FSMs of `Fungus1_04_boss` (`tools/census` `fsm-graph`):

| | Hornet needs | FSM VM today (`hk-format::fsm`, `hk-sim::fsm`) |
| --- | ---: | ---: |
| `Hornet Boss 1 Control` | 77 states, 349 enabled actions | 256 states |
| actions in one state | up to 19 | 16 |
| variables in one FSM | 94 | 16 |
| distinct action types in the fight set | 106 | 16 |
| cross-FSM traffic | `SendEventByName` 38, `SetFsmBool` 19, `GetFsmInt`, `SetFsmFloat` | none |
| object model | 14 child objects (needle, thread, sphere, dash colliders, range triggers), `ActivateGameObject` 39, `SetParent` 17 | one owner and one camera role |

The Crawler and Gruzzer graphs run in the 16 opcode core; Hornet is two orders of
magnitude beyond it. Running her honestly means a second FSM bank format
(typed float, int, bool, object and event slots, wide states), roughly 100 action
kernels, an entity table with Rigidbody2D and collider state for about 15
objects, cross-FSM event delivery, the tk2d animation clips with their event
frames, the sprite, sound and music cook for the fight, a boss module in the
streaming pool, and the native pieces the FSMs lean on (`HealthManager`,
`Recoil`, `ExtraDamageable`, `DamageHero`, `EnemyHitEffectsUninfected`, the
conversation and the `HORNET LEAVE` path). None of that exists, and a per-attack
substitute is excluded by the brief.

The headless original gives ground truth: `tools/hkref` with
`og.scene = Fungus1_04`, `og.enemies = [{"match": "Hornet Boss 1"}]`.
