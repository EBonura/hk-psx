# Enemy families on the compact actor tier

Enemies whose behaviour is a PlayMaker tree run the original tree on the HKFSM03 kernel (docs/FSM-V2-DESIGN.md)
with no body of their own. A family is a row of `host/enemy_families.json`; there is no per-family recognizer.

## The row

Name prefixes and required components; the FSMs the placement carries (by name, with the `fsm_digest` of each, so
changed retail data fails the cook); which of them the port drops as inert (with the reason); the child object whose
trigger box the tree listens on; the clips the guest binds (at most eight, `clips[0]` is slot 0) and the clip the
object rests on; the limitations written into the cook report.

## Where each part lives

| Step | Python (the path the shipping build uses) | Rust |
| --- | --- | --- |
| Recognise | `host/enemy_family.py`, called from `actors.actor_sources` | `host/hk-cook/src/enemy_family.rs` (feeds `actor_persistence`; `hk-cook-parity actors` compares the control records) |
| Spec and art | `actors.generated_actor_records` (`ActorController::FsmActor`), `cook.append_actor_art` (`art_bindings`, basis scale) | none: the Rust spec path serves only the Mossman and Crossroads_22 scenes |
| Banks and tables | | `host/hk-fsm/src/enemy_family.rs` (`cook-families`, part of `cook_all`): `data/<name>3.hkf3`, `data/enemy_families.rs` |
| Run | | `shared/hk-sim/src/fsm_actor.rs`, `game/src/enemies.rs` `Runtime::FsmActor`, module `actor_fsm` |

## Plant Trap (Fool Eater), landing 1

* No body. tk2d writes a Box sprite's collider into the object each time the frame changes, so the hurt and
  `DamageHero` box exists only on the Snap and first Retract frames (`colliderType` 2 of the sprite definition); the
  cooker turns that into a per-frame table and the guest's `local_bounds` returns it, turned by the placement's quarter
  turn (placements stand on floors, ceilings and walls).
* The `Detector` child's trigger box is authored per placement (four sizes in Fungus1_22), so it is in the control
  record and the actor spec.
* Deviation: the `damages_enemy` relay FSM (29 of the 43 actions of the original: an enemy-versus-enemy damage
  forwarder) is dropped through the manifest's inert list. No route situation has an enemy hit by a Plant Trap.
  Reconsider for the Moss Knight, whose Shot Point carries the same FSM.
* Not presented yet: the Ready Grass particles (`PlayParticleEmitter`, raised as `Env::fx`) and the ready growl
  (`AudioPlaySimple`, raised as `Env::sound`).
* The corpse is the object's own Death clip played in place (the original's `Corpse Plant Trap` plays the same clip
  and then hides itself); `no_corpse` in the control record.

## Plant Trap against the original (2026-11-03)

`tools/hkref/profiles/pt22.json` (fsmtrace of the original on Fungus1_22 with the Knight placed 1.9 units above the trap at `(29.386, 14.494)`; `HK_REFERENCE_BOXES=1` adds a `box` token with the root's own BoxCollider2D, world size, to every ENV row). Original, game time from the trigger: Ready 0 s, Snap 0.76 s, Retract 1.76 s, Cooldown 2.356 s, Idle again 2.856 s. The port: Snap 45 ticks (0.75 s) after Ready, Retract 60 (1.0 s) after Snap, Retract clip 36 ticks, Cooldown 30 ticks (0.5 s). The collider boxes the original puts on the object while it plays (width x height, world units) are 4.016x1.734, 1.5x3.922, 1.609x3.125, 1.047x3.125, 1.0x2.547 and then off at 2.02 s; the cooked `BOXES_PLANT_TRAP` frames are exactly those five (the sixth entry repeats one), and at rest the original has no live collider. Not compared: frames (the original run carries no screenshots in fsmtrace mode), the Ready Grass particles and growl (not presented).

`walk-plant-trap-kill` (tools/walks/plant-trap-kill.walk, card greenpath-22-switch.mcd): the Knight jumps into the trigger, takes three snaps (5 to 2 masks) and kills the trap with four nail hits through `fight 4 27 33`, closed loop, no cheat, 1,504 ticks, the same twice.
