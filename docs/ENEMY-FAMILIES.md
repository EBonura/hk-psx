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
