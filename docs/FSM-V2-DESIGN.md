# Hollow Knight FSM runtime v2: design

Status: design plus truth pass, 2026-10-15 (branch `design/fsm-v2-truth-2026-10-15` of hk-psx). The truth pass replaced the census scratch scripts with a Rust tool (`fsm-census`, `host/hk-fsm`), cooked four real trees to the proposed layout (`fsm3-sizes`), and ran the three experiments. Sections 2.1 to 2.3, 2.7, 3, 4.4, 5 and 6 carry measured numbers and the decisions taken. Where a measured number replaced an estimate the old figure is kept in the text so the miss is visible. The original scratch scripts remain in `census-scratch/` next to this file and are superseded.

## 0. Decisions in one page

1. Keep the PlayMaker model exactly (ordered actions per state, per-frame re-run of `everyFrame` actions, FINISHED, event interrupts later actions, global transitions, cross-FSM events). Change everything underneath it.
2. Replace the two compact VMs that exist today (HKFSM01/02 in `hk-format::fsm` + `hk-sim::fsm*`, and HKSCR01 in `hk-sim::script`) with one: a typed bytecode, `HKFSM03`, cooked offline, run by one non-generic kernel (3.75 KB resident, measured in section 2.2).
3. Action implementations are table-driven handlers in 14 families. Family code lives in room modules (the streaming module system already in `host/code_modules.py`), reached through a 14-word resident entry table. Cook decides which families a room needs from its banks.
4. Everything sized from data, per FSM: cells, bit variables, object handles, scratch slots, bitsets. No `MAX_VARIABLES = 16`, no `MAX_STATE_ACTIONS = 16`.
5. Fixed-point Q16.16 with `psx_math::int32::{mul_shr_i32, mul_div_i32, isqrt_u32}`. No f32, no i64/u64 arithmetic in guest handlers (cook may use 64-bit).
6. Engine effects (physics body, tk2d animation, audio, particles, spawning, HealthManager and friends) stay native, reached by a small service interface. The VM never reimplements them.
7. Fidelity is proven by an action-level differential against the original through a new event-driven `FsmTrace` in `tools/hkref`, replayed natively on the host, then on the disc image. No FSM counts as done on a screenshot.
8. Order: dry-run cook and trace probe (done), format and reference interpreter, kernel, HKSCR01 fold (decision 2, it funds the kernel), Crawler/Gruzzer/Vengefly, Hornet 1, then the next bosses in map order. After Hornet 1 the VM is no longer the bottleneck (2 to 16 new action types per boss); art, audio and native components are.

## 1. Census of PlayMaker actions in enemy and boss FSMs

### 1.1 What was counted

Corpus: every `HealthManager` GameObject in the original Windows data (317 level files plus `resources.assets` prefabs), taken from the census cache `hk-census-finalise-2026-10-11/original/source-objects.jsonl.gz` (the Rust `hk-feature-census` output), then every PlayMakerFSM on that GameObject or any descendant of it (Transform hierarchy walked in the level file). The FSM objects were read with `host/source.py` (UnityPy) in a scratch script. Only enabled actions count. Disabled actions are ignored.

| | count |
|---|---:|
| HealthManager roots (enemy and boss placements, incl. prefabs) | 2,487 |
| of which outside GG_*, Dream_* and prefabs ("main game") | 2,297 |
| FSM instances on those trees | 7,768 |
| distinct FSM definitions (hash of name, state/action order, events) | 412 |
| distinct enabled action types | 338 |
| enabled action instances | 220,499 |

Instance totals are inflated by Godhome (GG_*) and Dream copies of the same bosses. The definition count is not. Roots with no FSM at all (native-only enemies) are inside the 2,487; 2,117 of 2,297 main-game roots carry at least one FSM action.

Reproduction: `host/hk-fsm/src/bin/fsm-census.rs` (commands in Appendix B) reads the same cache and the Windows Unity files with `hk-unity` and reproduces every table in sections 1.1 to 1.4 and Appendix A exactly, run to run identical, in 4 seconds. Two corrections to the text came out of it: the ranking key in 1.5 and the cumulative type count in 1.2.

Limits of this census, stated plainly: it does not include scene-level director FSMs that are not under a HealthManager (for Hornet that is the `Encounter` FSM, battle gates, boss loader: HORNET.md counts 30 FSMs and 106 types for the whole scene, my tree has 17 FSMs and 90 types). It does not analyse reachability; unreachable states count. It does not see actions injected at run time. `CallMethodProper` and `SendMessage` targets were only partly decoded (see 1.5).

Cross-check against HORNET.md: `Hornet Boss 1 Control` here is 77 states, 349 enabled actions, 19 actions in the widest state, 95 variables (HORNET.md says 94, counting convention). Same FSM.

### 1.2 Headline: the vocabulary is a long tail, not a sharp head

| Types supported (by FSM-definition rank) | share of all enabled action instances | FSM definitions fully runnable (of 412) | main-game enemy trees fully runnable (of 2,117) |
|---:|---:|---:|---:|
| 16 | 40.7% | 20 | 0 |
| 32 | 56.6% | 44 | 0 |
| 64 | 74.1% | 70 | 2 |
| 96 | 85.9% | 107 | 21 |
| 128 | 91.0% | 158 | 87 |
| 160 | 94.1% | 215 | 257 |
| 200 | 96.8% | 287 | 818 |
| 250 | 99.2% | 350 | 1,514 |
| 338 | 100% | 412 | 2,117 |

Three consequences.

- Instances concentrate (128 types is 91%) but whole enemies do not. A typical enemy tree needs 29 types (median), 59 at p90, and each one drags a private tail of two to eight rare types. This is why P10 froze the old runtime: an FSM runs only when every enabled action does. Plan for the full vocabulary eventually and make rare actions cheap, instead of hoping for a cut-off.
- The first four consumers (Crawler, Gruzzer, Vengefly, Hornet 1) need a closure of 104 types. That closure covers 80.6% of all enabled instances and 112 of 412 FSM definitions fully, but only 109 of 2,117 enemy trees. Hornet 1 alone brings 68 types that the other three do not use.
- Everything after Hornet 1 is cheap in vocabulary: see 1.4. Cumulative types after all 24 listed bosses: 246 (247 with the v1 types; the earlier 248 came from a slightly different boss list).

### 1.3 Families (what the actions do)

| family | types (of 338) | enabled instances | share | types in Hornet 1 | types used by any of 24 bosses |
|---|---:|---:|---:|---:|---:|
| math/vars | 47 | 38,553 | 17.5% | 12 | 37 |
| transform/move | 44 | 33,582 | 15.2% | 18 | 27 |
| object/spawn | 45 | 28,574 | 13.0% | 10 | 31 |
| flow/compare | 28 | 24,843 | 11.3% | 11 | 26 |
| events/messaging | 30 | 23,887 | 10.8% | 8 | 24 |
| animation | 22 | 20,469 | 9.3% | 5 | 15 |
| ai/sense/chase | 39 | 15,820 | 7.2% | 7 | 25 |
| audio | 15 | 11,502 | 5.2% | 8 | 14 |
| timers | 5 | 9,320 | 4.2% | 2 | 5 |
| combat/health | 19 | 4,031 | 1.8% | 3 | 8 |
| particles/fx | 8 | 3,315 | 1.5% | 1 | 8 |
| collider | 10 | 2,972 | 1.3% | 3 | 9 |
| raycast | 5 | 2,424 | 1.1% | 0 | 3 |
| playerdata/boss-meta | 21 | 1,207 | 0.5% | 2 | 14 |

Family assignment is my own grouping (`census-scratch/fam.py`); it is the unit of overlay in section 3.

### 1.4 What each boss needs (bosses in the order I propose to bring them up)

"new vs earlier rows" is the number of action types a row adds to the union of all rows above it, so it is the incremental vocabulary cost in this migration order.

| boss (scene) | FSMs | states (largest FSM) | enabled actions | max actions/state | variables | GameObjects | types | types beyond v1 | new vs earlier rows |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Crawler (Crossroads_05) | 1 | 5 (5) | 9 | 3 | 3 | 1 | 4 | 0 | 4 |
| Gruzzer (Crossroads_04) | 1 | 23 (23) | 36 | 8 | 19 | 3 | 13 | 0 | 12 |
| Vengefly (Fungus1_20_v02) | 2 | 15 (8) | 56 | 6 | 9 | 4 | 24 | 20 | 20 |
| **Hornet 1 (Fungus1_04_boss)** | 17 | 161 (77) | 526 | 19 | 205 | 19 | 90 | 78 | **68** |
| Mantis Lords (Fungus2_15_boss) | 1 | 59 (59) | 183 | 10 | 51 | 5 | 43 | 36 | 6 |
| Soul Master (Ruins1_24_boss) | 9 | 106 (73) | 436 | 24 | 105 | 22 | 79 | 70 | 16 |
| Collector (Ruins2_11_boss) | 7 | 98 (45) | 357 | 15 | 113 | 13 | 87 | 77 | 14 |
| Watcher Knights (Ruins2_03_boss) | 6 | 87 (52) | 304 | 13 | 78 | 18 | 71 | 61 | 6 |
| Dung Defender (Waterways_05_boss) | 13 | 146 (75) | 527 | 23 | 163 | 16 | 76 | 67 | 2 |
| Flukemarm (Waterways_12_boss) | 6 | 41 (17) | 126 | 12 | 53 | 19 | 46 | 40 | 7 |
| Uumuu (Fungus3_archive_02_boss) | 4 | 59 (34) | 222 | 15 | 54 | 12 | 63 | 55 | 5 |
| Traitor Lord (Fungus3_23_boss) | 13 | 131 (61) | 536 | 15 | 231 | 28 | 86 | 77 | 13 |
| Crystal Guardian (Mines_18_boss) | 10 | 72 (38) | 291 | 15 | 135 | 23 | 71 | 62 | 14 |
| Nosk (Deepnest_32) | 6 | 73 (53) | 255 | 15 | 64 | 14 | 58 | 49 | 2 |
| Hive Knight (Hive_05) | 5 | 87 (49) | 276 | 15 | 76 | 38 | 70 | 60 | 3 |
| Hornet 2 (Deepnest_East_Hornet_boss) | 23 | 222 (85) | 721 | 21 | 277 | 22 | 104 | 92 | 7 |
| Broken Vessel (Abyss_19) | 23 | 244 (71) | 687 | 19 | 257 | 19 | 94 | 82 | 9 |
| Grimm (Grimm_Main_Tent_boss) | 15 | 200 (105) | 779 | 39 | 193 | 45 | 97 | 87 | 8 |
| White Defender (Dream_04_White_Defender) | 19 | 199 (90) | 767 | 45 | 214 | 38 | 80 | 71 | 0 |
| Radiance (Dream_Final_Boss) | 45 | 433 (87) | 1,194 | 52 | 224 | 71 | 90 | 82 | 12 |
| Hollow Knight (Room_Final_Boss_Core) | 29 | 397 (164) | 1,521 | 61 | 371 | 85 | 128 | 117 | 7 |
| False Knight (Crossroads_10_boss, native today) | 6 | 146 (128) | 702 | 23 | 85 | 20 | 100 | 89 | 4 |
| Gruz Mother (Crossroads_04, native today) | 4 | 62 (30) | 235 | 12 | 55 | 4 | 60 | 45 | 3 |

Hard limits seen in the data, which set the v2 field widths: largest FSM 164 states (so state ids need u8 only), 46 events per FSM, up to 68 enabled actions in one state (so the finished-bitset needs up to 9 bytes, not 2), 371 variables in one boss tree, 85 GameObjects in one boss tree (Hollow Knight), 45 FSMs in one tree (Radiance). v1's limits (16 variables, 16 actions per state, 16 KiB bank) are exceeded by 109 of 412 definitions on variables and 46 on actions per state.

Variable classes by tree: Hornet 1 has 63 floats, 22 ints, 32 bools, 12 vec2, 12 vec3, 49 GameObject handles and 15 strings (strings are names: they become cook-time ids). Runtime bytes for all of Hornet's variables are about 630 B; Hollow Knight, the largest, about 1 KB. Variables are not the memory problem. Code and bank bytes are.

### 1.5 Ranking by coverage (top 80 is in Appendix A)

Ranking key: FSM definitions that use the type; ties keep the order of first appearance in the corpus walk (roots, FSM references, states, actions). The earlier text said "then enemy roots, then bosses", which the tables never did (in Appendix A rank 76 has fewer roots than rank 77), so the tie order is now stated as what it is and the tool uses it. The first 18 types alone (the "core", section 2.4) are 27.7% of instances and are what scene scripts need too. In order: GetOwner, Wait, BoolTest, Tk2dPlayAnimation, SendEventByName, SetVelocity2d, ActivateGameObject, Tk2dPlayAnimationWithEvents, GetPosition, NextFrameEvent, FloatCompare, AudioPlayerOneShotSingle, SetFloatValue, FindChild, SetScale, SetBoolValue, SetPosition, WaitRandom, RandomFloat, AudioPlaySimple.

Native method calls are few. `CallMethodProper` through `functionCallParams` names about 30 distinct methods in non-Godhome enemy trees, dominated by `FreezeMoment` (311), `NailParry` and `NailParryRecover` (297 each), `SetActive` (269), `SetBounceFactor` and `SetBounceAnimation` (76 each), then the `flash*` family (about 55 together) and a long tail of one-offs. That makes the "call a native method" op a small cook-time method table, not a reflection layer.

## 2. VM design

### 2.1 Shape

One kernel, one bank format, one object model, for enemies, bosses and (later) scene scripts.

```
Bank (per room or per tree, immutable, hash-checked at install like crawler.fsm today)
  header 16 B   magic HKF3, version, counts
  tree dir      per FSM: states, cells, bits, objs, scratch, max-finished-bits, offsets (16 B each)
  event map     tree-local event index -> destination FSM / local index (cook-resolved sends)
  per FSM:
    state table    4 B/state   {first_action u16, n_actions u8, n_trans u8}
    transitions    2 B each    {event u8, target u8}   (global transitions are a tail list)
    action stream  2 B header + 2 B per operand
    constant pool  4 B each, deduped per FSM
    init table     (slot u8, value i32) only for non-zero initials
```

- Action header: byte 0 is `family:4 | flags:4` (flags: UPDATE, FIXED, LATE, spare), byte 1 is the op index within the family. Operand count and kinds come from the op's schema (compiled into the family module), so there is no count byte. Variable-arity ops (FloatSwitch, SendRandomEvent arrays) carry their count as the first operand.
- Operand: u16, 3 tag bits + 13 bit payload. Tags: CONST (pool index), CELL (i32 slot), BIT (bool slot), OBJ (object handle), EVENT (tree-local), NONE (the original "None" variable, preserved distinct from zero as HANDOFF requires), INLINE (small signed literal, saves pool entries), ARRAY (offset to a counted list). Vectors are N consecutive CELL operands.
- Storage map per FSM, cooked from the declared variables: CELL for float, int, enum, event-valued ints; BIT for bool; OBJ for GameObject (index into the tree's entity table); strings are not stored, they become ids at cook time. Cell/bit/obj counts are bank data, so the instance block is `cells*4 + ceil(bits/8) + objs` bytes. Cross-FSM `SetFsm*`/`GetFsm*` are cook-resolved to (fsm, slot), no name lookup at run time.
- Cook enforces types (the declared variable kind must match the operand kind, a bool cell only ever holds 0 or 1). The runtime does not re-validate; hosts and tests keep a checked build of the same handlers. This is the largest per-type saving over v1, whose generic `read`/`write` checks and `Error::Parameter` paths account for much of the 460 to 955 B per type measured below.
- Disabled actions are dropped at cook. Pure-plumbing parameters (owner default, editor-only fields, `everyFrame` itself) become flags rather than operands.

Order fidelity: the kernel executes actions in authored order, once on entry, then only the UPDATE ones each tick, as the current `ordered_executor!` does. A returned event interrupts the remaining old-state actions at once. FINISHED fires when all enabled actions are finished, once. Transition budget stays bounded (32 today; make it a bank constant). Global transitions are searched after state transitions. `GotoPreviousState` needs a previous-state register (u8). Delayed events and `NextFrameEvent` go through one small per-tree queue.

#### Layout as built (truth pass)

`host/hk-fsm/src/v3.rs` lowers a tree to this layout, serializes it, decodes the bytes back and compares (round trip identical for all four trees), and `fsm3-sizes` checks the decoded states, enabled-action order and transitions against the source graph (G0 listing: clean for Crawler, Gruzzer, Vengefly and Hornet 1; no action type had an irregular operand count). Writing it forced these choices, each a change to the sketch above:

- **Action header needs an operand count.** The kernel must step over an action it does not run (a finished one, or a non-UPDATE one on a tick pass), and the sketch put the arity only inside the family module. Chosen: a resident table `ARITY[338]` indexed by `BASE[family] + op` (366 B resident), arity minus one when the OWNER flag is set. The alternatives cost more (a third header byte is +526 B on Hornet 1, an update chain per state about +600 B).
- **Transitions sit first in each state's block, the actions after them.** The 4 B state record `{block offset u16, n_actions u8, n_trans u8}` is then enough; the sketch could only find a state's transitions with a prefix sum over the states or a fifth field.
- **Header flags** are UPDATE, FIXED, LATE and OWNER (the spare bit). With OWNER set, the first FsmOwnerDefault parameter is "use owner" and its operand is omitted (206 of 526 Hornet actions; 104 more name a specific object and keep an OBJ operand).
- **Directory record is 32 B, not 16 B** (states, global count, start state, finished bytes, cells u16, bits u16, objs u16, scratch, wide-init flag, six section offsets, three counts). That is 544 B on Hornet 1's 17 FSMs. Narrowing cells, bits and objs to u8 and dropping the offsets the counts imply would give 20 B; not done, the 204 B saved is not worth the 255-cell limit (Hollow Knight has FSMs with 151 variables, and vec3 and colour variables take 3 to 4 cells each).
- **Event ids are tree-wide u8** (Hornet 1 has 44; FINISHED is 0), used directly in transitions and EVENT operands. The "event map" is therefore only a per-event bitmask of the FSMs that handle it (3 B per event for 17 FSMs, 132 B), used to deliver a send to the right FSMs without scanning all of them.
- **FsmEventTarget is 3 operands** (target and flags as INLINE, the target object as OBJ or NONE, the FSM name as CONST or NONE), not 1. SendEventByName is 5 operands, not 3.
- **Literals**: integral floats within +-4095 and ints within 13 bits are INLINE (602 of 650 Hornet literals); the rest go to the per-FSM pool (404 B in total). Vector literals are runs of consecutive pool words addressed by the first.
- **NONE is the second most common tag** (454 of 1,861 Hornet operands, 24 percent): unused store targets and optional events. Dropping trailing NONE operands through the op schema would save up to about 900 B (12 percent); not applied, it makes the arity depend on more than the flag.
- **PlayMaker global variables** (Hornet 1 reads `Hero`, `GameManager`, `AudioManager`, `CameraParent`, `AreaTitle`) are not declared on the FSM. The cook gives each a hidden slot; the native side must fill those slots at spawn. This is a new cook-to-service contract.
- **Init table**: `(slot u8, value i32)` is 5 B and the value is not word aligned, so the loader reads it with an unaligned load (one-time cost; an aligned 6 B entry is the alternative). Bit and object handles start from an image: a bit image only when some bit is set, one byte per object slot.
- **Not resolved yet**: cross-FSM `SetFsm*` and `GetFsm*` keep their name strings as pool constants (arity 4). Resolving them to (FSM, slot) is part of step 1; it shrinks each by about two operands and moves the byte totals by well under 5 percent.

### 2.2 The kernel (resident)

Non-generic, no trait objects, no per-enemy specialization (the `ordered_executor!` macro that stamps three copies of the executor goes away).

State per FSM instance (8 to 12 B): state u8, previous state u8, flags u8 (entered, finish-sent, active, dead), finished bitset (width from bank: 1 to 8 B), pointer/offset to its cell block. Scratch slots (u32 each) for stateful actions (Wait remaining ticks, animation-wait handles, tweens): cook allocates scratch per state and the kernel reserves the maximum over states, so Hornet's widest state costs 19 slots, not 349.

Kernel API for handlers (all `#[inline]`-free calls into resident code, `jal`, which modules may do): `i32(n)`, `set_i32(n,v)`, `bit(n)`, `set_bit(n,b)`, `obj(n)`, `event(n)`, `scratch(n)`, `send(event)`, `send_tree(fsm, event, delay)`, and the clock (`now_tick`, `fixed_steps`). A handler returns `{Continue | Finished | Event(e)}` packed in a u16.

Scheduling: the actor tree runs its FSMs in the original component order (cooked as the FSM order in the bank), phases Update (every sim tick) and Fixed (accumulated 50 Hz steps inside the 60 Hz tick, as the Vengefly work already does). Event delivery to another FSM in the same tree is processed in the same tick when the original does (to be settled by the trace, 4.3).

Resident size, measured (`tools/fsm-exp`: the kernel built with the game's compiler flags, link-map symbol sizes, functions kept out of line so each is attributable). Run, enter, deliver, transition and the all-finished check: about 1,200 B. Send: 288 B. Tree tick and start: 540 B. Tree install: 540 B. The operand API handlers import (`k_val`, `k_set`, `k_bit`, `k_set_bit`, `k_obj`, `k_event`, `k_scratch`, `k_rand`, `k_send`): 760 B. That is **3,328 B of code**, plus 422 B of resident data (`ARITY` 338, `BASE` 28, `FAMILY_ENTRY` 56): **3.75 KB** against the 3 to 4 KB budget. LTO inlining will trim some of it; what this kernel leaves out will add some (FIXED and LATE phases about 100 B, the `GotoPreviousState` handler). Treat 3.4 to 4.2 KB as the range.

Run on the real Hornet 1 bank with stub handlers (17 FSMs, 300 ticks), the tick pass costs about 7,000 cycles, 1.2 percent of a 60 Hz frame (564 k cycles), dominated by walking the action headers of 17 current states. A per-state update chain would cut that; not needed yet.

### 2.3 Handlers, grouped into the 14 families

Each family is one module with a single exported `entry(op, ctx)`; inside, ops are reached by a lookup table or a binary-search if-chain (the module rules forbid compiler jump tables in modules, `code_modules.py` rule 2). Handlers do three things at most: decode operands through the kernel API, do the arithmetic, or call one native service. That is what keeps them small.

| family | what it holds | native services it calls |
|---|---|---|
| flow/compare | BoolTest, BoolTestMulti, BoolAll/Any/NoneTrue, FloatCompare, FloatInRange, FloatSwitch, FloatSignTest, FloatTestToBool, IntCompare, IntSwitch, StringCompare (id compare), GameObjectIsNull/Compare, SendRandomEvent V1 to V3, NextFrameEvent, GotoPreviousState | none |
| math/vars | Set*Value, Float/Int add, sub, mul, div, operator, clamp, Random*, Convert*, BoolFlip*, vec2/vec3 get/set/add/lerp, SetGameObject, EaseFloat | none |
| timers | Wait, WaitRandom, RandomWait | clock |
| events/messaging | SendEventByName (V1, V2), SendEvent, SetFsm*/GetFsm* (cook-resolved), Trigger2dEvent, Collision2dEvent, SendMessage and CallMethodProper (method id table) | event bus, trigger and contact feeds |
| transform/move | Get/Set position, scale, rotation, FlipScale, Translate, Set/Get Velocity2d, SetVelocityAsAngle, Decelerate V1 to V2 and XY, AddForce2d, AccelerateVelocity, gravity scale, kinematic flag, iTween* (as VM tweens) | entity transform and body |
| object/spawn | GetOwner, FindChild, FindGameObject, GetParent, SetParent, GetChild*, ActivateGameObject, ActivateAllChildren, DestroySelf, CreateObject, SpawnObjectFromGlobalPool, FlingObjects*, GetHero, tags | entity table, pool cages |
| animation | Tk2dPlayAnimation (V1, V2, WithEvents), Tk2dPlayFrame, Tk2dWatchAnimationEvents, SetMeshRenderer, SetSpriteRenderer, colour ops | animation service (clip ids from cook) |
| ai/sense/chase | FaceObject, FaceDirection, FaceAngle, CheckCollisionSide(Enter), CheckTargetDirection, GetAngleToTarget2D, GetDistance, ChaseObject V1 to V3, IdleBuzz V1 to V3, Start/StopWalker, WalkLeftRight, DistanceFly, CheckCanSeeHero, CheckAlertRange | body sense, terrain query |
| raycast | RayCast2d V1 to V2, LineCast2d, GetColliderRange | terrain query |
| audio | AudioPlay*, AudioPlayerOneShot*, AudioStop, pitch/volume, FadeAudio, ApplyMusicCue, TransitionToAudioSnapshot | SPU voice service (per-scene banks) |
| particles/fx | Play/StopParticleEmitter, emission settings, SpawnBlood | particle pool |
| collider | SetCollider, Set*ColliderSize/Offset/Trigger | entity hitboxes |
| combat/health | SetInvincible, SetDamageHeroAmount, SetRecoilSpeed, SetIsDead, GetHP/SetHP, InstaDeath, SendHealthManagerDeathEvent | HealthManager component (native) |
| playerdata/boss-meta | Get/Set/Increment PlayerData, PlayerDataBoolTest, GG* checks (constant false in the base game), ShowGodfinderIcon | persist |

Handler sizes, measured. Fourteen handlers were written against the real encoding (BoolTest, FloatCompare, IntCompare, SendRandomEvent, NextFrameEvent, SetFloatValue, SetIntValue, SetBoolValue, FloatMultiply, RandomFloat, Wait, WaitRandom, SendEventByName, SetVelocity2d) and sized from the link map: 64 to 292 B, **mean 140 B, none over the 300 B go/no-go**. By family: flow/compare 72 to 204 B (mean 138, estimate 100), math/vars 64 to 140 B (mean 98, estimate 90), timers 208 and 292 B (mean 250, estimate 120), events/messaging 120 B (estimate 160), transform/move 160 B plus a 20 B service shim (estimate 220). Each family also needs an entry function: 64 to 208 B for the `match op` of the handlers written here, about 1.2 KB across the 13 families Hornet 1 uses. The sample is the easy end (flow, math, timers and one service call); ai/sense/chase, object/spawn and animation hold most of the estimated bytes and are not measured, so they carry the remaining risk. The estimates below stay as the budget for those.

Earlier estimate and v1 costs. Measured v1 costs in the current link map: `fsm_fly::execute` is 11,468 B for 12 action types (about 955 B per type), `fsm_walk::execute` is 1,844 B for 4 (about 460 B per type), and the Vengefly worktree reports the dispatcher at 11,496 B for roughly 20 types plus 3,772 B for variable load/save and 4,412 B for generic actor bridges. For v2 I budget **about 90 to 120 B** for pure flow/math handlers, **about 150 to 260 B** for ones that call a service, 200 B average over all families. That is an assumption justified by cook-time typing, shared decode and no generics; it is the first thing step 3 measures, with a go/no-go at 300 B per type.

### 2.4 What stays native, and what the VM provides to it

Native, never reimplemented in the VM:

- Physics body (Rigidbody2D solve against terrain, recoil displacement), contact queries, raycasts: existing bounded solver and `runner_senses`.
- tk2d clip playback, frame events, sprite state: the animation cache; handlers pass cook-resolved clip ids.
- Audio voices, music cues, snapshots: SPU banks; handlers pass cook-resolved clip ids.
- Particle emitters, blood, break effects.
- `HealthManager`, `Recoil`, `ExtraDamageable`, `DamageHero`, `EnemyHitEffects*`, `EnemyDeathEffects`, hit-stop (`FreezeMoment`), nail parry, Geo drops, the encounter conversation, battle gates, the `HORNET LEAVE` path. These are Unity MonoBehaviours in the original, not FSMs; they are the engine boundary. They get a generic native implementation shared by all enemies, not per-boss logic.
- Spawn pools: `SpawnObjectFromGlobalPool`, `CreateObject`, `FlingObjects` resolve to cook-reserved cages (the Hatcher precedent), with the cook refusing a scene whose worst case exceeds the cage.
- PlayerData and scene persistence.

Provided to native by the VM: a tree event input (`send_tree`), trigger/contact/animation-event feeds as events, and read access to cells for the HUD/debug. Native code never writes FSM state.

Core family (resident or tiny always-loaded module): the 18 types that cover 27.7% of instances and everything the HKSCR01 scene scripts use today: GetOwner, Wait, BoolTest, SetBoolValue, SetIntValue, SetFloatValue, IntCompare, FloatCompare, SendEventByName, NextFrameEvent, SendRandomEvent, RandomFloat, PlayerDataBoolTest, Get/SetPlayerData{Int,Bool}, Trigger2dEvent, plus the native-call escape.

### 2.5 Object model

The 85-GameObject Hollow Knight tree and Hornet's 19 mean an entity table per tree, sized from data. Per entity about 28 to 32 B: flags (active, collider bits, kinematic, visible), position x,y (z constant), scale x, rotation z, velocity x,y, parent u8, clip/frame/aux. Hornet: 19 entities, about 0.6 KB; Hollow Knight: 85, about 2.7 KB. `FindChild`, `GetOwner`, `GetParent` and the like resolve to constants at cook time (this is the "cook-time object model" P10 already found makes `FindChild` compile 881 of 1,018). Only `SetParent` and `ActivateGameObject` change entity state at run time. Per-tree runtime block for Hornet: FSM headers 17 x 12 B, variables about 0.63 KB, entities about 0.6 KB, queue 64 B, scratch 19 x 4 B: **about 1.7 KB**, allocated from the room's pool, never resident.

### 2.6 Numerics

Q16.16 as today (`ONE = 65536`). Add/sub direct. Multiply `mul_shr_i32(a, b, 16)`, divide `mul_div_i32`, distance `isqrt_u32` on a pre-shifted square. These helpers exist in `psx-math::int32`. `hk-sim` has 118 source lines mentioning `i64` today (`runner.rs` distance, jump factor, the v1 `Wait` conversion `delay as i64 * 60`); the Manny rule is no i64 arithmetic in guest code, so v2 handlers must not add to that. Each handler gets a numeric contract (range, rounding, overflow policy) written from the operand ranges observed in the original traces, and a host test against an f32 reference over those ranges. `GetDistance` is the known awkward one (squared Q16.16 distance overflows i32 past about 181 units); the contract will pick a prescale.

### 2.7 Known traps in the module system, and the proposed handling

- Modules may not reference other modules, and resident code reaches a module only through import sites (`HK_MODULE_IMPORTS`, 256 rows). A 14-word `FAMILY_ENTRY` table in resident data whose words target each family's link space is exactly the "resident data word" case the installer already binds (W32), and an evicted family points back at `__hk_module_trap`. The kernel calls `FAMILY_ENTRY[f]` with `jalr`. `fsm_body.rs` records a deliberate "no indirect call enters the guest action path", so this needed an experiment first (E1). Result, measured on a guest built with the game's compiler flags (all delay-slot filler searches on) and put through the SDK's `hazard-patch` and `hazard-scan` with its link map:

- The scanner reports `jalr` sites as hazards, as expected: LLVM puts a load in the delay slot and a register jump's callee is unknown (6 `jalr t9 | slot lw a1,... | callee unknown`, plus one ordinary `jal`). `hazard-patch` reroutes each through a 5-word entry in `HAZARD_TRAMPOLINES` (20 B per site), 0 remaining, and the final scan reports 0 hazards. The game's trampoline capacity is 256 words and the ordinary guest uses 117, so the kernel's one or two sites fit.
- Cost per dispatch, net of loop overhead, on the PSoXide cycle model (512 dispatches over 14 families, best of 7): direct call 12.4 cycles; `jalr` through the 14-word table 33.4 (this includes the trampoline hop and the main-RAM load of the table word); 14-arm `match` (jump table plus `jal`, the fallback) 36.8; two-level (family table, then op table) 71.9. The table is both faster and smaller than the fallback (the bench's table loop is 180 B, the `match` loop 392 B), so the single-level family entry with a `match op` inside the module is the choice and the two-level table is not.
- At that cost a tick that runs 200 actions spends 6,700 cycles in dispatch, 1.2 percent of a 60 Hz frame. Hornet's measured load is about 12 action runs per tick.
- The module side is settled by reading `harden()` in `host/code_modules.py` rather than by running it: resident relocations from `.data` into a module are accepted if they are `R_MIPS_32` (type 2), need no caller allow-list (that check is for `.text`), and each becomes one W32 import site bound to the trap while the module is evicted. 14 entries use 14 of the 256 import rows. This experiment did not link a real module and run `harden()`; step 3 proves that with the first family module, and the 14-`jal` fallback stays in the plan until then.
- If E1 fails, fallback is 14 direct `jal` import sites from a `match family` in the kernel (a resident switch is fine, rule 2 is about modules). Costs 14 of the 256 rows and about 100 B.
- Banks are data packages (no fixups) in the room's code chunk, hash-checked at install (`HK_CODE_HASH`).

## 3. Memory plan

### 3.1 Where the bytes stand (from the link map and receipts)

From `build/hk-psx-normal.map`, `.hkpsx/build-normal.json` (built 2026-10-15 13:35 from the hornet campaign tree) and `build/modules.json`:

| item | bytes |
|---|---:|
| resident code (.text) | 809,892 |
| resident data (.data) | 280,584 |
| bss (incl. scene arena 410,612) | 797,504 |
| module pool (below stack reserve) | 94,208 |
| free below pool | **0** |
| largest room code chunk | 44,520 (scene 26: gruzzer, climber, crawler) |
| worst room + carried Shade art | 80,972 (HANDOFF puts the allowance for adding code to the worst room, room 22, at 8,432 B after the 4 KiB window fence) |
| current module sizes (code + rodata) | false_knight 22,536; mawlek 18,044; husks 21,612; vengefly 8,076; gruzzer 21,392; gruz_mother 12,384; climber 13,620; crawler 7,492; arena 264 |

HANDOFF figures for the Vengefly-on-original-FSM attempt, which this design must beat: IMAGE overflow 5,528 B, provider 34,152 B before import tails against an 8,432 B room allowance, dispatcher 11,496 B, variable initial/load/save 3,772 B, generic actor bridges 4,412 B.

### 3.2 Resident vs overlay

| piece | where | bytes | basis |
|---|---|---:|---|
| kernel code (machine and operand API) | resident | 3,328 | measured (2.2) |
| `ARITY` 338, `BASE` 28, `FAMILY_ENTRY` 56 | resident data | 422 | measured; 14 import rows |
| hazard trampoline for each `jalr` site | resident | 20 per site | measured (E1) |
| core family (18 ops) | tiny module `fsm_core`, loaded with any room that has a v2 bank | about 2,500 to 2,900 | 18 types at the measured 140 B mean plus an entry; the earlier 1,800 to 2,200 was low; resident only if a scene-script room must run with no module load |
| other 13 families | one module each, loaded per room by bank-derived family mask | see 3.3 | |
| bank, Hornet 1 | data package in the room code chunk | **7,748** | measured; estimate was 6 to 9 K |
| banks, Crawler, Gruzzer, Vengefly | same | **249, 668, 856** | measured; v1 is 396, 1,320 and 1,668 |
| per-tree runtime block | module pool, allocated at actor spawn | Hornet 1: 970 (FSM blocks, measured) + about 600 (20 entities x 28 to 32 B, estimate) + 64 (queue) = about 1.6 K | the earlier 1.7 K holds |
| native service shims (entity table ops, spawn cages, method table) | with the family that calls them, or one `fsm_services` module | 2,000 to 3,000 | estimate, unchanged; the one shim written was 20 B |

Hornet 1 bank, by section (bytes, `fsm3-sizes`): action stream 4,774 (526 headers x 2 B plus 1,861 operands x 2 B; 9.1 B per action against the 8 assumed because the mean is 3.5 operands, not 3), state table 644 and transitions 408 (together 1,052 B, 6.5 B per state: exactly the estimate), constant pool 404 (estimate 600), init images 416 (estimate 700), array lists 380 (not in the estimate), directory 544 (estimate 300), tree header plus event map 148, alignment 30. The Control FSM alone is 4,685 B for 349 actions (13.4 B per action); the 15 other FSMs average 205 B, of which the 32 B directory record is 16 percent. Per tree, Appendix C. All four banks decode to the same graph they were cooked from.

Against v1 density (36 B per enabled action on gruzzer.fsm, 30 B on the Vengefly pair) the v2 banks are 51 to 63 percent of the size while carrying every enabled parameter of the original, and Hornet 1 would have been 16 to 19 KB at v1 density.

### 3.3 Family module estimates

Bytes per type by family. "Assumed" is the section 2.3 assumption; "measured mean" is the mean of the handlers written in `tools/fsm-exp` (14 handlers in 5 families, section 2.3). The Hornet column uses the measured mean where there is one.

| family | assumed B/type | measured mean | Hornet 1 types | Hornet 1 bytes | all 338 types (assumed) |
|---|---:|---:|---:|---:|---:|
| flow/compare | 100 | 138 (5 handlers) | 11 | 1,518 | 2,800 |
| math/vars | 90 | 98 (5) | 12 | 1,176 | 4,230 |
| timers | 120 | 250 (2) | 2 | 500 | 600 |
| events/messaging | 160 | 120 (1) | 8 | 1,280 (assumed) | 4,800 |
| transform/move | 220 | 160 + 20 shim (1) | 18 | 3,240 | 9,680 |
| object/spawn | 200 | not measured | 10 | 2,000 | 9,000 |
| animation | 150 | not measured | 5 | 750 | 3,300 |
| ai/sense/chase | 260 | not measured | 7 | 1,820 | 10,140 |
| raycast | 250 | not measured | 0 | 0 | 1,250 |
| audio | 120 | not measured | 8 | 960 | 1,800 |
| particles/fx | 120 | not measured | 1 | 120 | 960 |
| collider | 110 | not measured | 3 | 330 | 1,100 |
| combat/health | 120 | not measured | 3 | 360 | 2,280 |
| playerdata/boss-meta | 90 | not measured | 2 | 180 | 1,890 |
| total | | | 90 | **about 14.2 KB** (unchanged: the measured families moved up and down by about the same amount) | about 53.8 KB |

Add the family entry functions, about 1.2 KB for the 13 families Hornet 1 uses, which the earlier estimate did not carry: **about 15.4 KB** of family code.

Hornet's room therefore needs roughly: families 15.4 K + service shims 3 K + native Hornet-room components (HealthManager, Recoil, hit effects, encounter; unmeasured, say 3 to 5 K, now to be written clean-room, see decision 3) + bank 7.7 K + runtime block 1.6 K = **about 31 to 33 KB**, against a boss room that today carries almost nothing (the arena module is 264 B). With the 30,388 B Shade art carried, that is about 61 to 63 KB of 94 KB. It fits; the fit problem is the resident kernel and the shared rooms, not the boss room.

For the three existing enemies the point is a net saving. v1 modules replaced: crawler 7,492 + gruzzer 21,392 + vengefly 8,076 = 36,960 B. v2 projection for a room carrying all three: families for the 36-type union (36 x 140 B = 5.0 K, plus about 0.9 K of entries for the 8 families involved) + body bridges (Walking, Flying, Chase; about 3 K, unmeasured) + three banks (1.8 K, measured) + three thin actor adapters (about 2 K, unmeasured) = **about 13 to 15 KB**, roughly 22 KB less. The bridges and adapters are the unmeasured half of this number; step 4 settles it.


### 3.4 Funding the resident kernel at 0 B free

Candidates, with link-map evidence. None is applied; each needs a perf check against the 30 fps rule (Manny 2026-10-01). Decision 2 (section 6) sets the order: the HKSCR01 fold first, the sense-helper dedupe only with a measured frame cost that keeps the 30 fps rule, otherwise the kernel in an always-loaded module.

| source | evidence in the map | recoverable |
|---|---|---:|
| fold HKSCR01 into v2 | `hk_sim::script::Instance::run` 1,584, `send` 608, `transition_for` 98, `hk_psx::script::VARS` 448 bss; `hero_trigger` 280 stays as a service | about 2.7 K |
| monomorphization copies of the sense helpers | `line_of_sight_near` x4 = 7,000 B (4 copies, one is about 1.9 K), `walker_queries_with` x2 = 4,796, `walker_senses` x2 = 2,708, `climber_ray_hit` x4 = 4,612 | up to about 12.6 K (sum minus the largest copy of each) if the callback becomes one concrete type instead of a closure per call site |
| per-kind arms still inside the two giants | `EnemyWorld::tick` 35,624 B and `Actor::advance_in_view` 34,756 B (70 KB of resident `hk_psx::enemies`); the project already compiles scene-change-only functions for size | unknown, needs `cargo llvm-lines` style attribution; not done in the truth pass |
| `hk_sim::fsm` residue | 88 B | trivial |

Reading, with the measured kernel: the fold pays 2.7 K of the 3.75 K, a net resident cost of about **+1.0 K** (+0.7 to +1.5 K over the 3.4 to 4.2 K range). The fold alone does not close it at 0 B free; the rest has to come from the dedupe or from somewhere not yet found.

The fallback "kernel as a module" is only half available. MODULE RULES forbid a module referring to another module, and every family module calls the operand API (760 B, `k_val` and its siblings) on every operand. So the API and the tables (760 + 422 = 1.2 K) must stay resident whatever happens, and only the machine half (run, enter, deliver, send, tick, install: about 2.6 K) can live in an always-loaded `fsm_core` module reached from `Actor::advance` through ordinary import sites. With the fold that is a net resident change of **-1.5 K** (1.2 K stays, 2.7 K is freed), paid for with about 2.6 K of pool in every room that has a bank. That is the branch to take if the dedupe's frame cost is measurable. An all-in-one module (kernel plus family code per room) is not possible either: modules are linked at build time by name, not composed per room.

Also worth stating: v1 per-enemy FSM modules (crawler, gruzzer, vengefly, and later any native boss that moves onto v2) go away, which returns pool space, never resident space. The pool space is what makes Hornet and the later bosses fit.

## 4. Fidelity method

Oracle: `tools/hkref`, the instrumented macOS original run headless, 50 Hz physics, seeded `UnityEngine.Random`. Today it gives `actors.csv` (current state name per FSM, sampled in LateUpdate) and `enemy-trace.csv` (Walker/Climber fields). Its own README says sampling cannot establish intermediate callback order, transient states, or all spawns and deaths. A state-name sample per frame is not a differential for a 77-state FSM. So v2 needs an event-driven trace.

### 4.1 New: `FsmTrace` in the hkref driver

PlayMaker.dll exposes the choke points (confirmed by strings on the shipped assembly): `Fsm.SwitchState`, `Fsm.DoTransition`, `Fsm.ProcessEvent`, `FsmState.ActivateActions`, `FsmStateAction.OnEnter/OnUpdate/OnFixedUpdate`, `Finish`, plus `FsmLog.LogEnterState/LogEvent/LogSendEvent/LogAction`. The patcher already rewrites a private clone of `Assembly-CSharp.dll`; extend it to the clone of `PlayMaker.dll` with hooks on those methods. Whether `FsmLog` is live in the shipped player is unknown (experiment E3, 0.5 day); if not, the Cecil hooks are the route.

Per event record (CSV, one row each, tracked FSMs only, restricted to the tree under a named root): `physics_step, unity_frame, phase, fsm, kind {ENTER_STATE | EVENT_SENT | EVENT_HANDLED | ACTION_RUN | ACTION_FINISHED}, state, event, action_index, resolved operand values (for the operands the action reads), values written`. Plus a state-exit snapshot of every variable of the FSM (cheap, and enough to localize a divergence). Plus an environment record: everything an action reads from outside the FSM (hero position, contact bits, raycast results, animation-event times, `Random` results, incoming damage and `DIR MSG`), taken at the same hook points.

### 4.2 Three gates per FSM

- **G0 cook equivalence (no emulator).** Decompile the cooked bank back to a state/action/transition listing and compare with the source FSM dump: same states, same order, same enabled actions, same events, same None-versus-zero, same literals. Every operand type-checked. Zero tolerance.
- **G1 trace replay (host, native Rust).** Feed the recorded environment stream into the v2 kernel with the checked handlers and require the same observable stream the original produced: state sequence, event sequence (including order inside one tick), service calls with arguments, variable snapshots at state exit. State and event streams must match exactly. Numeric values within the handler's declared tolerance (Q16.16 against f32; the tolerance is set per handler, from the observed range, before the run, not after). Timing is compared on the original's own frame/step clock, so the 50 Hz versus 60 Hz mapping cannot hide a bug here.
- **G2 closed loop (headless disc vs original).** `hkref sweep` over at least 10 seeds (the existing paired standard is 600 ticks, 10 of 10 pairs) with the real physics and the shared tape. AI choices differ by seed, so compare distributions and event pairing within the existing 12-tick window; every state of the Control FSM must be reached somewhere in the sweep, or be reached by a forced entry stimulus (a driver call that sends the named event at a tick, so the original's reaction is the oracle; this is the one place the driver must invoke gameplay, so it is a flagged addition).

### 4.3 What counts as done

An FSM is done when: G0 is clean; G1 is clean over every recorded run for that tree (state and event streams exact, values in tolerance); every enabled action type in it is exercised by at least one G1 run (a coverage check generated from the bank); every state is entered by G1 or by a forced-entry run; G2 shows no state or event the original never produced and every one it did produce; the disc image links with zero hazards and the module fit receipt passes; and the fight (for a boss) is completed end to end on the disc by a no-cheat tape with the same HP and damage outcomes. Settling the cross-FSM same-tick versus next-tick delivery question is a G1 deliverable for Hornet, since her Control FSM sends 38 `SendEventByName` and 19 `SetFsmBool` to siblings.

Tolerances (decided, section 6): state and event order exact; transition tick within 1 sim tick of the original after the 50 Hz to 60 Hz clock mapping; numeric values within a per-handler bound fixed before the run.

### 4.4 E3 result: the trace works headlessly

The oracle is the macOS Steam build in a private APFS clone made by `hkref build` (batch mode, no window, audio sandboxed), not the CrossOver bottle; the bottle and the Steam install were not touched. `hkref --work $W fsmtrace profiles/hornet1-fsmtrace.json` now runs the original only and writes `fsmtrace.csv`.

- **`FsmLog` is not enough.** In the shipped PlayMaker.dll `FsmLog.LoggingEnabled` (a static, settable from the driver) gates `LogEnterState` and `LogEvent` only; `LogTransition` is guarded by `PlayMakerGlobals.IsEditor`, and actions are logged only when an action calls `Log` itself. So it could give state entries and events, not transitions or actions.
- **Cecil hooks are the route and they work.** The patcher (`tools/hkref/patcher`) now also rewrites the clone's `PlayMaker.dll`: calls into `HKReference.FsmTrace` at the start of `Fsm.EnterState`, `Fsm.ProcessEvent`, `Fsm.DoTransition` and `FsmStateAction.Finish`, and before each `OnEnter`, `OnUpdate`, `OnFixedUpdate` and `OnLateUpdate` call in `FsmState`. It checks the IL shape of every site and fails closed. The driver (`mod/FsmTrace.cs`) writes one CSV row per event for the FSMs under the GameObject roots named in the profile (`og.fsmtrace`, comma separated): `test_frame, physics_steps, unity_frame, kind, fsm, state, a, b, c` with kinds `STATE_ENTER`, `EVENT`, `TRANSITION`, `ACTION_ENTER` (action index and type), `ACTION_TICK` (update, fixed or late), `ACTION_FINISH`, and `FORCED_EVENT`.
- **Forced entry is needed and works.** Walking left from the `right1` gate of Fungus1_04 stops at x = 48.3 (blocked); the Hornet Encounter FSM never gets its START. With `og.fsmsend` (`GameObject|FSM|EVENT|test frame`) the driver sends START to `Hornet Infected Knight Encounter/Encounter` at test frame 60 (the tape also presses Cross every 24 frames). The Encounter goes Point, Box Up, Dialogue (frames 77 to 258), Box Down, Leap Antic, Land, Start Fight, wakes Hornet at test frame 304, and her Control FSM runs the fight loop (Wake, Flourish, Idle, Run, Jump, A-Dash, Land, Escalation, and the zero-duration decision states `G Sphere?`, `Can Throw?`, `Flip?`, `Run Away?`, `Move Choice A/B`, which a once-per-frame sampler cannot see).
- **Numbers** (1,700 test frames, about 1 min 47 s of wall clock per run). The first run wrote 76,067 rows (53,570 action ticks, 9,573 action enters, 3,579 action finishes, 3,171 events, 3,094 state entries, 3,079 transitions, 1 forced event) over 15 FSMs (the 14 active ones in Hornet's tree plus the Encounter); 65 action types were entered in her tree (71 with the Encounter). Hornet's Control entered 31, 37 and 36 of its 77 states in the three runs made after the tracking fix. Reaching the other states needs longer fights over several seeds (G2) and forced entry (G1).
- **Not in the trace yet**: resolved operand values and values written, the environment record (hero position, contacts, raycast results, `Random` draws, damage messages), and component-level events (HealthManager, Recoil, hit and death effects, the encounter conversation), which decision 3 now requires. All three are hooks in the same patcher (the Assembly-CSharp side is already rewritten by it).
- **Not repeatable run to run, and a bug found on the way.** Same seed, same tape, same forced event: two runs after the fix wrote 75,825 and 74,950 rows, and their Control state sequences agree for the first 14 entries and then differ. Scene loading is asynchronous against the tape (the first logged row falls at test frame -1 in one run and 0 in the other), the hero is physical, and other systems draw from the seeded `Random` too, so the original cannot be relied on to repeat. G1 therefore has to replay the environment record taken in the same run (4.1), and G2 compares distributions, as designed; test frame numbers must not be used to align runs (align on a trace event such as the first Encounter state). The bug: a first draft cached "not under the root" per FSM, and an enemy is instantiated before it is parented, so one run lost two thirds of the rows; misses are no longer cached.

## 5. Migration plan and effort

Unit: an agent-day is one agent working a full day with the usual build and gate cycle. Parallelism is limited by the shared compile and headless limits in HANDOFF (about two heavy jobs globally), so I do not assume more than two agents at once. Estimates are ranges; the Hornet figure carries the widest uncertainty because most of it is not VM work. "Was" is the estimate before the truth pass; "now" is after it, with the reason.

| step | content | was | now | why it moved | exit gate |
|---|---|---:|---:|---|---|
| 0. Truth pass | Rust census; real cook of four trees; E1, E2, E3 | 2 to 3 | **done** | about 40 minutes of wall clock for the coding and the runs; `cargo llvm-lines` attribution of `hk_psx::enemies` was outside the requested scope and is still open | measured bank bytes, kernel bytes, go/no-go on E1 (go) |
| 1. Format, cook, reference interpreter | `HKFSM03` in `hk-format`, cooker in `hk-fsm`, checked host interpreter, G0 decompiler, convert the v1 Crawler and Gruzzer graphs and show the same results as v1 over the existing tests | 4 to 6 | 3 to 5 | encoder, serializer, decoder and G0 listing exist as measuring code (`host/hk-fsm/src/v3.rs`); still to do: resolve cross-FSM refs and global-variable bindings, the checked interpreter, v1 equivalence | v1 and v2 agree on existing Crawler/Gruzzer tests |
| 2. `FsmTrace` and replay harness | operand and environment records, trace schema, Rust replay into the reference interpreter; first traces for Crawler, Gruzzer, Vengefly | 4 to 5 | 3 to 4 | hooks, trace file and forced entry exist and ran on Hornet 1 (4.4) | G1 clean for Crawler on the reference interpreter |
| 3. Guest kernel and module plumbing | `fsm_core` (or resident kernel per the 3.4 outcome), `FAMILY_ENTRY`, family mask from banks in `code_modules.py`, core family, first family module through `harden()` | 4 to 5 | 3 to 5 | kernel written and measured (3.75 KB, runs the Hornet 1 bank); still to do: the module build path and the receipts | link with zero hazards; kernel bytes known; fit receipt |
| 3b. HKSCR01 fold | scene scripts onto v2, delete `hk_sim::script` and `hk_psx::script` runtime | optional 3 to 4 | **required**, 3 to 4 | decision 2: the fold funds the kernel, so it comes before the enemies | Area Resetter and the other cooked scripts replay identically |
| 4. Crawler, Gruzzer, Vengefly on v2 | 36-type union in families, body bridges, delete the v1 closures and the `ordered_executor!` copies, G0/G1/G2 on all three, install | 6 to 9 | 6 to 9 | handler sizes came in under the budget on the easy families; bridges and adapters are unmeasured | the three installed from v2, net module bytes smaller than v1 on scenes 22 and 26, handler cost under 300 B/type |
| 6. Hornet 1 | 68 new action types (about 8 to 10 days of handler work), entity table and tree bus (4 to 5), generic native components HealthManager/Recoil/hit effects/death/encounter/battle gates and the defeat path to `hornet1Defeated` (4 to 6, **now 9 to 14**: clean-room from observed behaviour and verified with component traces like the FSMs, decision 3), art, sounds and music for the fight (6 to 10), G1/G2 tuning (4 to 6) | 26 to 37 | **31 to 45** | decision 3 adds 5 to 8 days of component observation and verification | the existing `hornet-road-defeated` journey becomes a real fight: Hornet beaten with no cheat, Cloak earned, zero faults |
| 7. Next bosses in map order | Mantis Lords, Soul Master, Collector, Watcher Knights, Dung Defender, Flukemarm, then by area | VM share 1 to 3 each, whole boss 8 to 15 each | unchanged | the generic components are paid for once in step 6 | per boss: G0/G1/G2 plus a no-cheat fight |

Totals: steps 1 to 4 with the fold, the three starter enemies on v2 with traces and scene scripts on v2: **18 to 27 agent-days** (was 18 to 25 for steps 1 to 4 without the fold). Adding step 6, Hornet 1 defeated and the Cloak earnable: **49 to 72 agent-days** from now, against 44 to 62 for the same scope before the truth pass (46 to 65 with step 0). The 5 to 10 day increase is the mandatory fold (+3 to 4) and decision 3 (+5 to 8), less about 2 to 3 days of prototype work that now exists. For bosses after Hornet the VM work is the small part of the 8 to 15 days; art, audio and native components dominate, and that part this design does not estimate better than "similar to Hornet's non-VM share, scaled by sprite count and attack count".

Sequencing notes.

- The three starter enemies go first because they are the only place equivalence to v1 can be shown cheaply: same graphs, known outputs, existing tests. Hornet then reuses the kernel and adds vocabulary. The fold (3b) goes between the kernel and the enemies because it funds the kernel and is the first real consumer of the core family.
- False Knight, Gruz Mother and Mawlek (native hand ports today, modules of 22.5, 12.4 and 18 KB) should not move to v2 until the end, and only if a room needs the pool space or a fidelity audit finds them wrong. They work; each costs a boss-sized effort.
- The sense-helper dedupe (3.4) is no longer unconditional: do it only with a measured frame cost that keeps the 30 fps rule (decision 2); otherwise take the half-module kernel. Measure its frame cost first, on the Crossroads route tapes, before choosing.
- The `hk_sim` fsm code used by Vengefly in worktree `gameplay/vengefly-original-fsm-2026-10-13` (HKFSM02 "normalized" operands, `Arguments::decode`, concrete kernels 9 of 9) is a valid stepping stone for operand normalization; HKFSM03 keeps its ideas (tagged operands, None distinct, lossless cells, original order) and drops its per-component hard-coded cell layout (`[4,5,2]` counts) and the 8-operand cap.

Design smells I noticed and did not touch (for separate issues): two PlayMaker VMs in `hk-sim` (HKSCR01 and HKFSM); 118 lines using `i64` in `hk-sim` against the no-i64-in-guest rule; hand-ported native bosses and original-data FSMs coexisting as parallel implementations of the same behaviour; the 70 KB of per-kind arms in `hk_psx::enemies::{tick, advance_in_view}`; `ordered_executor!` stamping three executors per kernel family.

## 6. Decisions and what is still open

Decided 2026-10-15 (coordinator) on the three questions this design asked. They are fixed inputs from here on.

1. **"Done" bar.** State and event order exact; transition time within 1 sim tick after the 50 Hz to 60 Hz clock mapping; numeric values within a per-handler bound fixed before the run. Nothing stricter (no 50 Hz internal step). Consequence: the G1 and G2 gates in section 4 stand as written.
2. **Funding the resident kernel.** Fold HKSCR01 into v2 first. De-specialise the sense helpers only with a measured frame cost that keeps the 30 fps rule; otherwise move the kernel to an always-loaded module. Consequence, from the truth pass: the fold is now step 3b and mandatory; the fold alone leaves about +1.0 KB resident (3.4); and the module fallback moves only the machine half (about 2.6 KB), because family modules cannot call a module and so the operand API and tables (1.2 KB) stay resident. The decision holds, but "move the kernel to a module" means that half.
3. **Native boundary.** The non-FSM Unity components (HealthManager, Recoil, hit and death effects, the encounter conversation, the HORNET LEAVE path) get one generic native implementation for all enemies, rewritten clean-room from observed behaviour (no copied or translated decompiled code, memory `no-copied-third-party-code`) and verified with hkref traces like the FSMs. Consequence: step 6 grows by 5 to 8 agent-days (section 5), and the trace needs component-level hooks in the Assembly-CSharp side of the patcher. The IL tools may be used to find which methods to observe (names and signatures), not to translate bodies.

Open after the truth pass.

- **Hard families are unmeasured.** The 14 handlers sized (mean 140 B) are flow, math, timers, one event send and one move with a service call. ai/sense/chase, object/spawn and animation hold the bytes that matter and could be larger; the 300 B/type go/no-go is only checked on the easy end.
- **Module binding of the family table is read, not run.** `harden()` accepts the `.data` words by reading; step 3 must link a real family module and run it (3.2, 2.7).
- **Global variable contract.** Hornet 1 reads five PlayMaker globals (`Hero`, `GameManager`, `AudioManager`, `CameraParent`, `AreaTitle`). Who owns those slots in the native service layer is undecided.
- **Trace coverage.** 31 of Hornet's 77 Control states were entered in one run; the oracle needs more seeds and forced entries before G1 can be called.
- **`cargo llvm-lines` attribution** of `hk_psx::enemies` (the third funding source in 3.4) was not part of the truth pass and is still unknown.

## Appendix A: top 80 action types by coverage

Columns: enabled instances over all 7,768 FSM instances, FSM definitions using it (of 412), enemy roots whose tree uses it (of 2,487), how many of the 24 listed bosses use it, whether v1 implements it today, whether Hornet 1 uses it.

| # | action | family | enabled instances | FSM defs (of 412) | enemy roots (of 2,487) | bosses (of 24) | v1 | Hornet 1 |
|---:|---|---|---:|---:|---:|---:|:-:|:-:|
| 1 | GetOwner | object/spawn | 5457 | 250 | 1887 | 24 | x | x |
| 2 | Wait | timers | 7329 | 217 | 1722 | 24 | x | x |
| 3 | BoolTest | flow/compare | 9384 | 184 | 1962 | 24 | x | x |
| 4 | Tk2dPlayAnimation | animation | 7505 | 183 | 1893 | 24 |  | x |
| 5 | SendEventByName | events/messaging | 5125 | 162 | 1251 | 24 |  | x |
| 6 | SetVelocity2d | transform/move | 8879 | 159 | 1695 | 23 |  | x |
| 7 | ActivateGameObject | object/spawn | 5967 | 158 | 894 | 24 |  | x |
| 8 | Tk2dPlayAnimationWithEvents | animation | 5225 | 148 | 1409 | 22 |  | x |
| 9 | GetPosition | transform/move | 6196 | 146 | 1617 | 24 |  | x |
| 10 | NextFrameEvent | flow/compare | 2647 | 144 | 988 | 24 |  | x |
| 11 | FloatCompare | flow/compare | 4139 | 141 | 1216 | 23 | x | x |
| 12 | AudioPlayerOneShotSingle | audio | 2760 | 128 | 964 | 23 |  | x |
| 13 | SetFloatValue | math/vars | 7499 | 125 | 1434 | 24 | x | x |
| 14 | FindChild | object/spawn | 3359 | 120 | 921 | 22 |  | x |
| 15 | SetScale | transform/move | 3125 | 119 | 949 | 23 |  | x |
| 16 | SetBoolValue | math/vars | 5251 | 118 | 1220 | 23 | x | x |
| 17 | SetPosition | transform/move | 2127 | 115 | 675 | 22 |  | x |
| 18 | WaitRandom | timers | 1888 | 114 | 1060 | 12 |  | x |
| 19 | RandomFloat | math/vars | 3403 | 107 | 949 | 22 | x | x |
| 20 | AudioPlaySimple | audio | 3013 | 106 | 1176 | 20 |  | x |
| 21 | PlayParticleEmitter | particles/fx | 1542 | 94 | 617 | 22 |  | x |
| 22 | FaceObject | ai/sense/chase | 1883 | 93 | 797 | 18 |  | x |
| 23 | IntCompare | flow/compare | 1450 | 93 | 516 | 23 |  | x |
| 24 | SendRandomEvent | flow/compare | 1300 | 89 | 714 | 19 |  | x |
| 25 | SetParent | object/spawn | 1970 | 87 | 849 | 21 |  | x |
| 26 | SetFsmBool | events/messaging | 5067 | 86 | 1286 | 23 |  | x |
| 27 | SetMeshRenderer | animation | 2274 | 85 | 655 | 20 |  | x |
| 28 | GetScale | transform/move | 1508 | 84 | 684 | 19 |  | x |
| 29 | AudioStop | audio | 1172 | 83 | 640 | 21 |  | x |
| 30 | SpawnObjectFromGlobalPool | object/spawn | 2710 | 82 | 574 | 19 |  | x |
| 31 | FloatMultiply | math/vars | 1833 | 77 | 574 | 21 |  | x |
| 32 | SetCollider | collider | 1742 | 75 | 585 | 18 |  | x |
| 33 | GetParent | object/spawn | 4114 | 71 | 1621 | 21 |  | x |
| 34 | Tk2dWatchAnimationEvents | animation | 2036 | 71 | 797 | 19 |  | x |
| 35 | CheckCollisionSide | ai/sense/chase | 1260 | 70 | 505 | 17 | x | x |
| 36 | FloatAdd | math/vars | 1344 | 69 | 528 | 18 |  |  |
| 37 | CheckCollisionSideEnter | ai/sense/chase | 996 | 65 | 504 | 17 | x | x |
| 38 | AudioPlayerOneShot | audio | 1385 | 63 | 561 | 17 |  | x |
| 39 | SetIntValue | math/vars | 883 | 59 | 348 | 21 |  | x |
| 40 | IntAdd | math/vars | 539 | 58 | 307 | 20 |  | x |
| 41 | StopParticleEmitter | particles/fx | 918 | 58 | 330 | 17 |  |  |
| 42 | SetInvincible | combat/health | 988 | 57 | 302 | 18 |  | x |
| 43 | FloatSubtract | math/vars | 3596 | 56 | 1108 | 17 |  | x |
| 44 | SetRotation | transform/move | 1556 | 55 | 708 | 16 |  | x |
| 45 | SetIsKinematic2d | transform/move | 1033 | 54 | 492 | 15 |  | x |
| 46 | CreateObject | object/spawn | 495 | 54 | 236 | 19 |  |  |
| 47 | SetDamageHeroAmount | combat/health | 1019 | 52 | 532 | 12 |  | x |
| 48 | GetHero | object/spawn | 2021 | 51 | 1276 | 5 |  |  |
| 49 | SendMessage | events/messaging | 1687 | 50 | 432 | 15 |  |  |
| 50 | BoolAllTrue | flow/compare | 1224 | 49 | 565 | 7 |  |  |
| 51 | FloatOperator | math/vars | 726 | 47 | 282 | 16 |  | x |
| 52 | FloatTestToBool | flow/compare | 1062 | 46 | 277 | 11 |  | x |
| 53 | BoolTestMulti | flow/compare | 901 | 46 | 334 | 13 |  | x |
| 54 | TransitionToAudioSnapshot | audio | 160 | 45 | 90 | 22 |  | x |
| 55 | GetHP | combat/health | 207 | 43 | 122 | 14 |  |  |
| 56 | Tk2dPlayFrame | animation | 1179 | 41 | 657 | 14 |  | x |
| 57 | FaceDirection | ai/sense/chase | 1415 | 41 | 885 | 4 | x |  |
| 58 | DecelerateV2 | transform/move | 475 | 41 | 260 | 9 |  | x |
| 59 | FlipScale | transform/move | 725 | 41 | 573 | 12 |  | x |
| 60 | CheckTargetDirection | ai/sense/chase | 901 | 41 | 370 | 11 |  | x |
| 61 | SetGameObject | math/vars | 253 | 40 | 187 | 20 |  | x |
| 62 | SetVelocityAsAngle | transform/move | 810 | 40 | 423 | 8 | x | x |
| 63 | FloatClamp | math/vars | 2117 | 39 | 714 | 10 |  |  |
| 64 | AudioPlayRandom | audio | 704 | 37 | 506 | 6 |  | x |
| 65 | GetDistance | ai/sense/chase | 799 | 36 | 546 | 3 | x |  |
| 66 | FlingObjectsFromGlobalPool | object/spawn | 507 | 36 | 291 | 7 |  |  |
| 67 | Trigger2dEvent | events/messaging | 6177 | 35 | 1209 | 15 |  | x |
| 68 | GGCheckIfBossScene | playerdata/boss-meta | 205 | 35 | 97 | 20 |  | x |
| 69 | IntOperator | math/vars | 220 | 34 | 114 | 16 |  | x |
| 70 | SetGravity2dScale | transform/move | 600 | 34 | 162 | 14 |  | x |
| 71 | SetHP | combat/health | 649 | 34 | 346 | 8 |  |  |
| 72 | GetAngleToTarget2D | ai/sense/chase | 376 | 31 | 247 | 8 |  | x |
| 73 | SendRandomEventV3 | flow/compare | 103 | 31 | 68 | 16 |  | x |
| 74 | AudioPlayInState | audio | 463 | 31 | 303 | 8 |  | x |
| 75 | SetVector3XYZ | math/vars | 215 | 30 | 150 | 9 |  |  |
| 76 | ApplyMusicCue | audio | 80 | 30 | 66 | 19 |  | x |
| 77 | SetFsmString | events/messaging | 81 | 30 | 75 | 18 |  | x |
| 78 | AudioPlay | audio | 473 | 30 | 344 | 6 |  |  |
| 79 | SetAudioClip | audio | 479 | 29 | 242 | 9 |  |  |
| 80 | StopWalker | ai/sense/chase | 584 | 29 | 382 | 1 |  |  |

## Appendix B: how the numbers were produced

Everything below is committed on the branch and runs from the repository root. `DATA` is the `hollow_knight_Data` directory of the Windows install; `CACHE` is the `hk-feature-census` row cache (`source-objects.jsonl.gz`). `CARGO_TARGET_DIR` should be private.

1. Census, sections 1.1 to 1.4 and Appendix A: `cargo run --release --manifest-path host/Cargo.toml -p hk-fsm-cook --bin fsm-census -- DATA CACHE OUT` writes `OUT/census.md` and `OUT/census.json`. It streams the cache for HealthManager and PlayMakerFSM rows, walks each level's Transform tree with `hk-unity`, reads the 7,768 FSM objects and tabulates. 4 seconds, byte-identical between runs. The action family map is `host/hk-fsm/src/families.rs` (338 types, all mapped).
2. Layout sizes, section 2.1 and 3.2 and Appendix C: `... --bin fsm3-sizes -- DATA CACHE OUT` lowers the Crawler, Gruzzer, Vengefly and Hornet 1 trees (`host/hk-fsm/src/v3.rs`), writes the banks, `fsm3-sizes.md` and `arity.rs`, and fails if the decode round trip or the G0 listing differs.
3. Guest experiments, sections 2.2, 2.3, 2.7: `tools/fsm-exp/run.sh` (needs the banks from step 2, the SDK hazard tools built from `.psoxide/tools/psoxide-hazard` and the PSoXide frontend). Symbol sizes are the linker's, from the map of a build with the kernel functions kept out of line. Cycles are the PSoXide model, not silicon.
4. Trace, section 4.4: `tools/hkref` (`hkref build`, then `hkref fsmtrace profiles/hornet1-fsmtrace.json`).
5. Memory numbers in 3.1 and 3.4 are unchanged from the first draft: `build/resident.map`, `build/hk-psx-normal.map`, `build/modules.json` and `.hkpsx/build-normal.json` as found on 2026-10-15; the dedupe estimate is `sum minus largest copy` over same-named instantiations and has not been tried.

The first draft's scratch scripts (`census-scratch/`, Python over UnityPy) are superseded; the Rust tool reproduces their tables exactly.

## Appendix C: measured sizes

`fsm3-sizes` output, bytes. All four trees: decode round trip identical, G0 listing against the source clean, no action type with an irregular operand count, no Q16.16 overflow, no variable type mismatch.

| tree | FSMs | states | enabled actions | operands | bank B | B/action | v1 B | runtime block B (FSM blocks) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Crawler | 1 | 5 | 9 | 57 | 249 | 27.7 | 396 | 26 |
| Gruzzer | 1 | 23 | 36 | 155 | 668 | 18.6 | 1,320 | 81 |
| Vengefly | 2 | 15 | 56 | 211 | 856 | 15.3 | 1,668 | 51 |
| Hornet 1 | 17 | 161 | 526 | 1,861 | 7,748 | 14.7 | 16 to 19 K at v1 density | 970 |

Bank sections, bytes (header and event map are on top of the FSM sections):

| tree | directory | state table | transitions | action stream | arrays | pool | init | align | header + event map | total |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Crawler | 32 | 20 | 14 | 132 | 0 | 24 | 5 | 2 | 20 | 249 |
| Gruzzer | 32 | 92 | 72 | 382 | 40 | 8 | 8 | 2 | 32 | 668 |
| Vengefly | 64 | 60 | 46 | 534 | 28 | 80 | 9 | 5 | 30 | 856 |
| Hornet 1 | 544 | 644 | 408 | 4,774 | 380 | 404 | 416 | 30 | 148 | 7,748 |

Hornet 1 by FSM (bank bytes): Control 4,685; Stun Control 289; Control (Needle) 237; Setup and Follow 197; five 8-state range and dash FSMs 262 to 272; six 4-state FSMs 129 to 159; Grow 97; Fluctuate 75. Operand tags over 1,861 operands: INLINE 566, NONE 454, CELL 236, OBJ 199, EVENT 160, CONST 157, BIT 37, ARRAY 52.

Guest experiment results (PSoXide cycle model):

| measurement | value |
|---|---|
| dispatch, direct call | 12.4 cycles |
| dispatch, `jalr` through the 14-word table | 33.4 cycles |
| dispatch, 14-arm `match` | 36.8 cycles |
| dispatch, two-level table | 71.9 cycles |
| hazards before / after `hazard-patch` | 7 / 0 (6 `jalr` sites, 1 `jal`), 33 of 256 trampoline words |
| kernel code / resident tables | 3,328 B / 422 B |
| tick pass on the Hornet 1 bank, stub handlers, 17 FSMs | about 7,000 cycles per tick |
| handlers (14) | 64 to 292 B, mean 140 B |
