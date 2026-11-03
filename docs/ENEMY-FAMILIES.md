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

## Mossman_Shaker (2026-11-03): the Runner's Walker with a gas attack

The Shaker's Walker is the Mossman Runner's (speed 2, pauses 1.5 to 2.5 s, walking waits 1.5 to 4 s), so it rides the Runner controller with a third attack, `runner::Attack::Gas` (Fungus Zombie Attack: Ready on sight and alert, Attack Delay 0 to 45 ticks with the Walker still walking, Antic 45 ticks, Attack 48 ticks, CD 30, Idle Pause 30, Reset), recognised in host/runner.py and its Rust twin (pinned FSM fingerprint, clip contract, corpse `Corpse_Mossman_Shaker` as a `CorpseFungusExplode`). Hurt, recoil, death and corpse are the Runner's. The gas hit box is the 7-point polygon of `Gas Hit Box` scaled 0.2 to 1 over 24 ticks (easeOutCirc), DamageHero 1, tested against the Knight's body (`runner::gas_polygon`). The Runner needed no new body hook; the compact-tier body (state block, handlers) is for the boss-tier families.
Visible gaps: the gas is three ParticleSystems in the original (Gas Antic, Gas Attack, Gas Idle); the port raises the scene's boss emitters (`SHAKER_LOOK`, soft yellow-green puffs every 12 ticks of the attack) as a stand-in, not matched to frames. Not presented: the five Impact Lines flashes, the antic particles, the EnemyKillShake camera shake, the corpse's spore burst and flame, the Shaker's own footsteps and gas sounds (its walk loop is neither Runner bank's, so it walks silently and needs no bank).
