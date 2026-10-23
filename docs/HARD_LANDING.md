# Hard landing

A Knight who falls for more than 1.1 s lands in a crouch and cannot act for 0.8 s. Everything here is read from the Windows install (Assembly-CSharp IL, the Knight prefab in `resources.assets`); nothing is measured from footage.

## The original's rules

| Rule | Value | Source |
| --- | --- | --- |
| Fall time that arms it | `BIG_FALL_TIME` 1.1 s | `HeroController` serialized scalar, `resources.assets:20602` |
| Lock | `HARD_LANDING_TIME` 0.8 s | same |
| Fall counting | `fallTimer += deltaTime` every `Update` while `rb2d.linearVelocity.y <= -1e-6` and `CheckTouchingGround()` is false; a wall slide holds it at 0; any other frame (`vy > -1e-6`) resets the timer, `cState.falling` and `cState.willHardLand` (kept only while `transitionState == ENTERING_SCENE`) | `HeroController::FallCheck` |
| Arming | `fallTimer > BIG_FALL_TIME` sets `cState.willHardLand` and starts the fall rumble | `FallCheck`, `StartFallRumble` |
| Other resets | `Dash()`, `LeaveScene()` and `SteepSlope` call `ResetHardLandingTimer`; starting a wall slide clears `willHardLand` | `HeroController::Dash`, `LeaveScene`, `LookForInput`, `FinishedDashing`, `SteepSlope` |
| Trigger | floor contact (`FindCollisionDirection` bottom) while `ShouldHardLand`: the collider has no `NoHardLanding` component, `willHardLand`, not `inAcid`, state is not already `hard_landing`. Otherwise `BackOnGround()` (the soft landing) | `OnCollisionEnter2D`, `OnCollisionStay2D`, `ShouldHardLand` |
| Entry | `AffectedByGravity(true)`, `ResetInput()`, `SetState(hard_landing)`, `CancelAttack()`, `hardLanded = true`, sound `HardLanding`, spawn `hardLandingEffectPrefab` at the Knight | `DoHardLanding` |
| Effect | `HardLandEffect.OnEnable`: camera `AverageShake` (0.15 extents, 1 s, priority 7), then the child for `playerData.environmentType` (Dust, Grass, Bone, Spa, Metal, Wet: an impact-lines sprite animation and the `Knight Hard Land Dust B/F` particle systems), two flung rock bursts (2 to 3 each, 12 to 15 units/s, 95 to 140 and 40 to 85 degrees from 0.9 below the Knight) for dust, grass and bone, 50 white spatter for Spa; recycled after 1.5 s | `HardLandEffect`, prefab `resources.assets:4666` |
| Lock | `Update` adds `deltaTime` to `hardLandingTimer`; past `HARD_LANDING_TIME` it does `SetState(grounded)` and `BackOnGround()`. `FixedUpdate` calls `ResetMotion()` every step in the state. `LookForInput` is not called in the state (no walking, turning or look), `CanJump`, `CanDoubleJump`, `CanDash`, `CanAttack` and `CanSwim` refuse it | `Update`, `FixedUpdate`, `CanJump`, `CanDash`, `CanAttack` |
| Not blocked by the state | `CanCast` checks only `no_input`, so a spell can start during the crouch. `CanFocus`, `CanInteract`, `CanDreamNail`, `CanSuperDash` and `CanBackDash` need `cState.onGround`, which stays false until `BackOnGround` | `CanCast`, `CanFocus`, `CanInteract` |
| Fail safe | `FailSafeChecks` leaves the state after `HARD_LANDING_TIME + 0.3 s` regardless | `FailSafeChecks` |
| Animation | tk2d clip `HardLand` of the Knight library: 10 frames (sprites 78, 78, 77, 77, 79, 79, 79, 80, 80, 76) at 12.5 fps, plays once, so 0.8 s exactly | `HeroAnimationController::UpdateAnimation` |
| Gate drop-ins | a Knight arriving by a gate is in `no_input` when he lands, which goes through `FinishedEnteringScene`; the timers were reset by `LeaveScene`. He never hard-lands | `OnCollisionEnter2D` (state 7 branch) |
| Exceptions | `NoHardLanding` sits on 11 Fungus2 levels (174 to 177, 183 to 185, 191, 192, 195) and a test scene; acid; a hit or death ends the state (`no_input`) | `NoHardLanding`, `ShouldHardLand` |

A fall of 1.1 s from rest is about 17.8 world units: gravity is 47.4 units/s squared (Physics2D -60 times the 0.79 scale), the 20 units/s cap is reached after 0.42 s (4.2 units) and the rest is 0.68 s at the cap. A full jump rises about 3 units, so only a real drop gets there.

## What is ported

* `shared/hk-sim/src/hard_landing.rs`: `Player::hard_landing_tick` counts the descent and decides the landing (`fall_ticks` = 67, `floor(1.1 * 60) + 1` updates, the same rule the landing sound used), starts a 48 tick lock (`floor(0.8 * 60)`: the update that finds the timer past 0.8 already returns control), and cancels a held or queued jump. `Player::hard_landing`, `hard_landing_age` and `end_hard_landing` are the rest of the surface. Tests: a fall one tick under and exactly at the threshold, the lock length and the age, a mashed jump, upward velocity, wall slide, gate entry and dash resets.
* `game/src/frame.rs`: while locked the Knight gets no direction, jump, dash, strike, focus, bench, shop, NPC or map input (the same gates a Focus or a cast use); a hit ends the lock. A jump held through the lock is not a new press afterwards. A spell is not blocked, as in the source. The landing sound follows the simulation's verdict (the audio no longer keeps its own fall count).
* `game/src/hard_landing.rs`: camera `AverageShake` (the existing `camera::Shake::Average`), the `HardLand` clip (cooked into the ability art, `host/ability_art.py`), telemetry `HK_HARD_LANDINGS` and `HK_HARD_LOCK_TICKS`.
* The clip's frames-per-second is stored as 12 (the ability art keeps whole fps), so the pose stretches the age by 25/24 to land on the 12.5 fps frame at every tick.

## Not ported

* The dust, impact lines and rocks of `HardLandEffect`: a placeholder raises the Knight's Focus dust (`Dust L`/`Dust R`, cooked into every scene) at his feet. The hk-effects-fidelity lane builds the particle side; swap `hard_landing::landed` when it lands.
* The fall rumble (`RumblingFall`: a 0.015 unit looping camera shake and sound 12 from 1.1 s on), invisible at this scale.
* `NoHardLanding` and acid: no cooked scene carries one (the Fungus2 scenes are not cooked), so `allowed` only excludes gate drop-ins. The cooker lists any such component as unsupported when Fungal Wastes is cooked.
* `ForceHardLanding` (called from PlayMaker FSMs) and the `FailSafeChecks` timeout.
* A dash held through the lock is read as a new press when control returns (the dash input is gated, not buffered).
