//! What a boss's death asks of the frame, the same for every boss.
//!
//! A boss runtime calls `request` from its own tick (the False Knight's `Steam` and `Blow`; Hornet's and
//! the later bosses' death states do the same from theirs) and the frame raises the matching emitter
//! (`fx::BOSS_BLEED`, `fx::BOSS_BURST`) where the boss stands. What the emitters look like is data:
//! host/hk-cook/src/break_effects/fx.rs `BossLook`, one per boss scene, so a new boss adds a `BossLook`
//! and a call from its death state and no frame code. The camera's hold on a point of interest is
//! `camera::frame_x`.

/// The long state before the body opens: raised every twelve ticks.
pub const BLEED: i32 = 1;
/// The body opening: raised once.
pub const BURST: i32 = 2;

/// (kind, x, y, scene) of this tick's request, 0 for none. Four words the actor loop writes without a call.
static mut REQUEST: [i32; 4] = [0; 4];

/// Ask for `kind` at `x`, `y` (world Q16, the boss's feet) in `scene`; a later request in the same tick replaces it.
pub fn request(kind: i32, x: i32, y: i32, scene: usize) {
    unsafe { REQUEST = [kind, x, y, scene as i32] }
}
/// This tick's request, if any: (kind, [x, y], scene).
#[inline(never)]
pub fn take() -> Option<(i32, [i32; 2], usize)> {
    let [kind, x, y, scene] = unsafe { core::mem::replace(&mut *(&raw mut REQUEST), [0; 4]) };
    (kind != 0).then_some((kind, [x, y], scene as usize))
}
