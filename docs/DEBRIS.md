# Rigid debris

What a Breakable flings when it breaks and the guest solves as a rigid body: the pieces
with a Rigidbody2D, a sprite and a collider among its `debrisParts` (the ones with a
ParticleSystem are `BREAK_EFFECTS.md`). The original is `Breakable.Break` (every part is
activated, turned to `localEulerAngles.z + angleOffset`, given a velocity of `flingSpeed`
10 to 17 at the break's angle) followed by what each part carries.

## Classes

All 418 pieces of the 45 admitted scenes carry `flingSpeed` 10..17 and `angleOffset` -60.
The behaviour that differs is the part's own components (IL of `SpinSelf`, `SimpleRock`,
`SpinSelfSimple`, `FinishingRigidBody` in Assembly-CSharp):

| Components | Start rotation | Spin (AddTorque, once, on the second physics step) | Other |
|---|---|---|---|
| `SpinSelf` | random 0..359, overwriting the authored pose | `vx * spinFactor` | |
| `SimpleRock` + `FinishingRigidBody` | random | `vx * -7.5` | one tint (0.489, 0.51, 0.559) |
| `SpinSelfSimple` | authored (`m_LocalRotation` z, plus the break's offset) | `-vx * spinFactor` | |
| neither | authored | none | |
| `FinishingRigidBody` (with any of the above) | | | asleep 8 s, shrinks for 1 s, then deactivated; also deactivated after ten consecutive frames off screen |

`host/effects.py fragment_geometry` reads that from the components and refuses, by name,
only what the solver cannot hold: a round collider, an unmeasured finishing timeline, a
polygon over eight points, a scale outside 0.5..2 (judged on the world matrix's column
lengths; a negative determinant mirrors x) and a spin the bank's six signed bits cannot
count in halves (observed: 4, 5, 7.5, 8, 10, -7.5). Not ported: `SimpleRock.OnTriggerEnter`
(a nail knock of 44 pieces) and the nine `CircleCollider2D` pieces (two of them also auto mass).

## Where it lives

- The scene's metadata bank (`tools/world_metadata.py`, `shared/hk-format/src/world_meta.rs`):
  one `KIND_FRAGMENT` object per piece at the head of the scene's first region (a piece's
  index is its object index there), and per region one `KIND_FRAGMENT_FRAMES` object whose
  index list is (piece index, frame in that region's animation bank) pairs. A region cooks
  art and frames for exactly the owners its interaction bounds touch, the filter its
  breakables use, so a piece with no pair in the drawn region is simulated but not drawn.
- `game/src/debris.rs`: `decode` turns a bank object into a `Spec` where it is used; the
  pool is 28 slots keyed by (scene, piece index). A full pool retires the piece stopped
  longest, or drops the newcomer when every piece is still moving (`HK_DEBRIS_EVICTED`,
  `HK_DEBRIS_DROPPED`; `HK_DEBRIS_FINISHED` counts the finishing timeline's frees).
- The contact solver reads the piece's own inverse inertia. It used to recover it from the
  torque, which is right only at spin factor 4: pieces of factor 8 and 10 answered a contact
  two to two and a half times too lightly in rotation.

Tests: `tests/test_fragment_classes.py` (classification), `tests/test_world_metadata_fragments.py`
(the bank, with `tests/golden/fragment_bank.hkwm` that the Rust side reads),
`shared/hk-format` (`world_meta` validation), `shared/hk-sim/tests/debris.rs` (solver, pool,
finishing, eviction, decode), `tests/world_runtime.rs` (a break launches its owner's pieces).
`python tools/fragment_audit.py --plan` runs the cooker over the real source, read-only, and
prints per scene what the bank gains.

## Measured (2026-10-23, `tools/fragment_audit.py --plan` over the installed game, no cook)

409 of the 418 authored pieces cook (the nine refused are seven `CircleCollider2D` and two with
auto mass or linear damping, both round). They sit in 45 scenes: 185 authored-pose, 154
random-start, 44 SimpleRock (random, finishing, tinted) and 26 finishing-only. The most any one
region can draw is 14 pieces (Crossroads_ShamanTemple and Crossroads_06), half the pool.

| | before | after |
|---|---|---|
| pieces that cook | 67 | 409 |
| linked data for debris (DEBRIS_CATALOG) | about 7.7 KB | 0 |
| scene metadata banks | | +48,876 B over the 45 scenes, 120 B a piece |
| largest bank (the shared arena is sized by it) | Tutorial_01 69,076 B | Tutorial_01 72,524 B (+3,448) |
| next largest | | ShamanTemple 60,100 (+6,368, the biggest growth), Fungus1_09 58,356 |
| static texture area one region adds | | at most 6,708 px at screen size (Crossroads_01), a tenth of one 4bpp page |

The pool keeps its 28 slots, so a break's worst case is a 14-piece region plus whatever is
still lying about; a full pool retires the piece stopped longest.
