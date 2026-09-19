# Placement variants of already ported actors

Four of the 24 unadmitted Crossroads placements belong to families whose
controller is already written and shipped, or nearly so. They are refused by a
recognizer check about the placement rather than by anything about the
behaviour. This note records what each check rejects and what was measured when
it was relaxed.

Two of the four are now admitted and two are not, for reasons worth keeping.

- **Crossroads_39's Zombie Runner: admitted.** `host/runner.py` selects the
  driving FSM by name instead of demanding exactly one component, so the extra
  `enemy_corpse` no longer refuses it. Cost was nil: the scene's tightest region
  stayed at 290 textures.
- **Crossroads_37's mirrored Leaper: admitted**, but it needed more than a
  recognizer relax. The claim that its controller already supported it was
  wrong. `Runner::with_params` hardcoded `facing: -1` and `enemies.rs` asserted
  `initial_direction == -1`, so relaxing the host check alone would have shipped
  a guaranteed panic in a scene that already admits twelve actors. The
  controller now takes a starting facing, the guest accepts either sign, and the
  cooker reads the sign off the transform mirror rather than assuming.
- **The two duplicate-box Climbers: measured and dropped.** The recognizer side
  works, and with it relaxed both admit. The cost does not fit: one Climber art
  set is +15 textures, which puts Crossroads_03's tightest region at **421
  against the 416 CLUT budget**, the same wall the Knight ability clips hit
  earlier. ShamanTemple's would fit at 400, but admitting only that half needs a
  scene-specific carve-out, which is not a relax. The projection is a floor
  rather than an estimate: `similarity_dedup` protects every animation frame, so
  none of the 15 can be collapsed.
- The pure dedup half of that work, routing `runner.recognize`,
  `climber.recognize` and `baldur.py` through the one `body_box` tolerance that
  is currently written three times, is admission-neutral and would land on its
  own.

## Crossroads_39 Zombie Runner, `level69:3429`

`runner.recognize` calls `_one(records, 'PlayMakerFSM')` and this placement
carries two: the usual `Zombie Swipe` and an extra `enemy_corpse`. With the
`enemy_corpse` component ignored, the placement recognizes completely as an
ordinary `ZombieSwipeWalker`: walk speed 1.5, lunge speed 6.0, `Attack::Swipe`,
alert bounds `[-365036, -144507, 365036, 12730]` in Q16, which satisfies the
mirror stable box rule `recognize` already enforces.

The check to change is the FSM selection: pick the single FSM named
`Zombie Swipe` or `Zombie Leap` rather than requiring the object to carry
exactly one FSM. `enemy_corpse` is a death effect FSM and does not drive
movement. Nothing else about the placement differs.

## Crossroads_03 and Shaman Temple Climbers, `level39:5321` and `level76:14561`

Both carry the same `BoxCollider2D` twice (size 1.09375 by 0.921875, offset
0.015625, 0.4765625, both enabled, neither a trigger). `climber.recognize` calls
`runner._one(records, 'BoxCollider2D')`, which raises
`Runner requires exactly one BoxCollider2D`. With the duplicate dropped, both
recognize completely as `Climber` with `start_right` true and the audited speed
2.0, spin time 0.25, wall ray padding 0.1 and minimum turn distance 0.25.

`host/runner.py` already has the right helper: `body_box(records, who)` accepts
one or two boxes provided their size, offset, trigger flag and enabled flag
match, and raises otherwise. `host/baldur.py` implements the same tolerance a
third time inline. The fix is to route `climber.recognize` (and
`runner.recognize`) through `body_box` instead of `_one`.

Cost caveat before doing it. Crossroads_03 admits no Climber at all today, and
neither does the Shaman Temple, so admitting these two adds a whole Climber art
set to both scene banks rather than reusing art already resident. The comment in
`host/climber.py` records that the Crossroads_03 and Crossroads_05 views already
sit at the 416 texture budget after similarity dedup, so this is a budget
question, not just a recognizer question. Crossroads_05's `Climber 1` is already
admitted, which is why that scene is cheaper.

## Crossroads_37 Zombie Leaper, `level67:4145`

`runner.recognize` refuses with `unsupported Runner layer or initial scale`,
from `any(abs(matrix[i][i] - 1) > 1e-6 for i in range(2))`. The placement's world
basis is `[[-1, 0], [0, 1]]`: a plain x mirror with no rotation, unit magnitude
on both axes. With the scale sign flipped back to +1 the placement recognizes
completely: `Attack::Leap` with jump speed y 20.0, jump x factor 1.25, idle time
0.5 and trigger ticks 15 from the `Attack` clip's single trigger frame, walk
speed 2.25, and an alert box that is already mirror stable
(`[-378470, -110541, 378470, 154034]`).

Two things have to travel with the relaxed check:

- The sign has to reach `initial_direction`. `walker_parameters` returns -1 and
  `generated_actor_specs` hardcodes -1 for the `ZombieSwipeWalker` branch, so
  today a mirrored placement would start facing the wrong way.
- The body box is not symmetric about the actor origin. Its Q16 bounds are
  `[-25600, -96256, 24576, 72704]`, because the collider offset x is
  -0.0078125. `axis_aligned_bounds` already takes the world matrix, so the
  mirrored bounds come out correct; what matters is that runtime facing changes
  mirror the body offset with the sprite, which the comment in
  `runner.recognize` already calls out.

## The two Climbers that are not placement variants

`level39:5320` and `level41:4024`, both named `Climber 2`, have unit scale and a
local rotation quaternion of `(0, 0, -1, ~0)`: a 180 degree turn about z. They
are ceiling mounted Tiktiks, drawn upside down. Relaxing the basis check is not
enough for these: the guest would have to draw a rotated sprite, transform the
body box (offset y 0.4765625 becomes -0.4765625), and start the Climber's
surface follower attached to a ceiling. `docs/CLIMBER.md` already lists rotated
rendering and guest collision as prerequisites, so these two belong with the
rotated rendering work rather than with the three placements above.

## Verification

Every claim above was measured by editing the parsed trees in a worker and
re-running the real recognizer: for the Leaper, setting `m_LocalScale.x` to its
absolute value and clearing the `Scene.world` cache; for the Climbers, removing
the duplicate `BoxCollider2D` from `Scene.objects`; for the Runner, removing the
`enemy_corpse` `PlayMakerFSM`. The source files on disk are never written.
`host/remaining_actors.py` reports the refusals themselves, per recognizer, so
the starting point is reproducible with
`PYTHONPATH=host .venv/bin/python host/remaining_actors.py`.
