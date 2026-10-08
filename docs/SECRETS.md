# Secrets: hidden walls, cracked floors and reveal masks

Every hidden wall, cracked floor and secret-area mask of the on-disc scenes is
cooked from the Windows source and run by the guest. `host/secret_breaks.py`
turns the two FSM families `host/breakables.py` recognizes into multi-hit
breakables; `host/reveal_masks.py` recognizes the masks, including the ones a
break uncovers; `game/src/secret_breaks.rs` and `game/src/reveal_masks.rs` run
them.

## What is cooked

| Scene | Secret | Family | Hits | Uncovers |
|---|---|---|---|---|
| Crossroads_03 | `Break Wall 2` | tk2d wall | 4 nail or 1 spell | `crossroads_03_mask` (1.0 s, replays on every load) |
| Crossroads_07 | `Breakable Wall_Silhouette` | wall | 4 nail or 1 spell | nothing (its `Masks` is empty) |
| Crossroads_08 | `Break Wall 2` | tk2d wall | 4 nail or 1 spell | `break_wall_masks` |
| Crossroads_10 | `Breakable Wall` | wall | 4 nail or 1 spell | `Mask 1` and the eggs under it |
| Crossroads_18 | `Breakable Wall Waterways` | wall | 4 nail or 1 spell | `msk_generic 2`, `msk_generic 2 (1)` |
| Crossroads_21 | `Breakable Wall` | wall | 4 nail or 1 spell | `Mask 1`; also takes `Camera Locks`' terrain box |
| Tutorial_01 | `Break Floor 1` | floor | 3 nail, Hero Range | nothing |
| Crossroads_04 | `Break Floor 1` | floor | 3 nail, Hero Range | nothing |
| Crossroads_09 | `Break Floor 1` | open floor | 3 nail | `msk_generic` (0.4 s) |
| Crossroads_13 | `Break Floor 1` | floor (turned into a wall) | 3 nail, Hero Range | nothing |
| Crossroads_37 | `Break Floor 1` | floor | 3 nail, Hero Range | nothing |
| Fungus1_08 | `Break Floor 1`, `Break Floor 1 (1)` | floor (turned into walls) | 3 nail, Hero Range | the first takes a camera lock with it; the second's shaft masks go with `floor 2` |

The six walls and seven floors share the Breakable state space: a secret is a
breakable (flag 8) numbered from the top of its scene's 128 states down,
followed in the metadata bank by a `KIND_SECRET` object that holds its family,
facing, hit count, spell rule, Hero Range and the sagging planks' stage quads.
The broken bit, the terrain exclusion and the `Kind::Breakable` save item are
the ordinary breakable's, so a broken secret stays broken across a room
reload, a death and a save. Hits taken are FSM state in the source and are not
saved: leaving the scene or dying starts an unbroken secret over.

## Behaviour (source states in brackets)

- Walls take a hit only in `Idle` and then lock out for `Hit X` + `Return X`
  (12 ticks). A hit that leaves the wall standing moves its art 0.1 units
  along its `Facing` and back over those ticks; the killing hit does not.
  Vengeful Spirit breaks a wall at once (`Spell Destroy`). Hit sound:
  `breakable_wall_hit_1` or `_2` at 1:1, pitch 0.85..1.15. Break:
  `breakable_wall_death` and `secret_discovered_temp`, `AverageShake`.
- Floors take a nail hit only while the Knight's body overlaps `Hero Range`
  (the open Crossroads_09 floor has none), 15 ticks apart. `Hit 1` and
  `Hit 2` sag `floor 1` and `floor 2` by their authored Translate and Rotate
  (cooked as stage quads), play `barrel_death_1` and send `EnemyKillShake`.
  The break plays `barrel_death_1` and `breakable_wall_death` and sends
  `AverageShake`.
- Every accepted hit spawns the nail Slash Impact at the secret's strike
  point (`Strike Nail R` in the source) and the family's particle bursts.
- Masks fade through the subtractive CLUT when their art is binary black and
  through the draw's gain otherwise (iTween multiplies the material colour).
  A one-way mask's save key is its rank among the scene's saved one-way masks
  (`persist_slot`), which does not move when another mask is admitted.

## Particles

A secret's bursts go through the shared break-effect pool with PS1 budgets:
each hit burst is cut to 24 particles and each break to 96, keeping the
emitters' proportions and emission times (the source authors up to 85 for a
floor's hit and 330, or 525 on Crossroads_09, for its break).
`break_effects.secret_relax` carries the four emitter features the bounded
model does not run as documented approximations: Local scaling, cone and
single-sided-edge shapes, local-space force and velocity; a random gravity
multiplier is a per-particle force range.

## Routes

`cargo hk-build build` replays one `secret-*` route per secret beside the
other routes (tapes under `tools/tapes`, cards under `tools/cards`, seeded
with `tools/seed_card.py`). Each boots seated next to its secret and nails it
at polls 220, 256, 292 (and 328 for a wall), then walks or jumps on. The pins
are the source hit count, one break, no refused hits, no deaths and no scene
SFX misses. `secret-c03-reload`, `secret-c10-reload` and `secret-c18-reload`
boot a card whose save already holds the broken wall (`--item
1:SCENE:STATE:1`) and walk through where it stood; their pins are no hits and
the Knight's final x past the wall. `secret-c46-eggs` walks into the Ancestral
Mound egg room and ends with its three masks faded.

Some seats frame poorly and are kept because the break is what they prove:
the Crossroads_04 floor is nailed from the room under it, which the room's
own six masks keep black (the base build frames it the same way), and the
Crossroads_07 and Crossroads_21 walls are nailed from behind terrain the
Knight cannot walk past afterwards.

`secret-c37` nails the Crossroads_37 floor early (polls 60, 96, 132) and
`c37-stand` stands in that room for 3,000 polls. The first panicked the
guest at poll 159 (`input sampler contract`, `HK_INPUT_FAULT` 4 =
QueueFull), and the second at poll 125 on the ship6 disc: the room's
thirteen Husks each swept every edge of their view with the exact i64 ray
test twice a tick, which left a simulation tick costing about a VBlank. The
secrets build stood there at 7.3 fps with the simulation a few ticks behind
the pad, and the floor's particles tipped it over. `runner_senses::Sweep` now rejects an edge
whose box misses the rays' box before the exact test (24 fps standing there),
and the sampler never panics when the simulation falls behind: see
`game/src/input_sampler.rs`.

## Not reproduced

- The shiny's `White Wave` pulse (WaveEffectControl, about 0.27 s while a room
  fades in) is not drawn; it used to be drawn as a permanent white shape.
- The Crossroads_21 Husk Guard's `Wall Breaker` swipe (its guard is refused).
- Flung rock and wood pool objects are not simulated as rigid bodies.
- The `Crumble` clip (eight transparent frames) is not played.
