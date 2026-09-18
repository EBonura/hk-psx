//! Shared PlayMaker flyer actions: IdleBuzz roaming (Vengefly Idle, Aspid Idle)
//! and DistanceFly (Aspid). One call is one source 50 Hz fixed step; the
//! per-step constants stay as authored. Evidence: .hkpsx/vengefly/actions.il,
//! .hkpsx/aspid/source.il.
use crate::ONE;

/// IdleBuzz(waitMin .75, waitMax 1, speedMax 1.75, accelerationMax 15, roamingRange 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleBuzz {
    pub start: [i32; 2],
    /// Per fixed step, Q16 units/s (already divided by 2000).
    accel: [i32; 2],
    /// waitTime in Q16 seconds.
    wait: i32,
}
pub const IDLE_SPEED_MAX: i32 = 114688; // 1.75
pub const IDLE_ACCELERATION_MAX: i32 = 15 * ONE;
pub const ROAMING_RANGE: i32 = ONE;
pub const WAIT_MIN: i32 = 49152; // .75 s
pub const WAIT_MAX: i32 = ONE;
impl IdleBuzz {
    pub const fn new(start: [i32; 2]) -> Self {
        Self { start, accel: [0; 2], wait: 0 }
    }
    /// OnEnter re-samples the roaming origin from the current position.
    pub fn enter(&mut self, position: [i32; 2]) {
        self.start = position;
    }
    /// DoBuzz: `range(low, high)` is the caller's Random.Range over Q16.
    pub fn step(&mut self, position: [i32; 2], velocity: &mut [i32; 2], range: &mut impl FnMut(i32, i32) -> i32) {
        let v = velocity;
        for axis in 0..2 {
            let low = position[axis] < self.start[axis] - ROAMING_RANGE;
            let high = position[axis] > self.start[axis] + ROAMING_RANGE;
            if (low && v[axis] < 0) || (high && v[axis] > 0) {
                self.accel[axis] = if low { IDLE_ACCELERATION_MAX } else { -IDLE_ACCELERATION_MAX } / 2000;
                v[axis] = ((v[axis] as i64) * 8 / 9) as i32; // /= 1.125
                self.wait = range(WAIT_MIN, WAIT_MAX);
            }
        }
        if self.wait <= 0 {
            for axis in 0..2 {
                let (lo, hi) = if position[axis] < self.start[axis] - ROAMING_RANGE {
                    (0, IDLE_ACCELERATION_MAX)
                } else if position[axis] > self.start[axis] + ROAMING_RANGE {
                    (-IDLE_ACCELERATION_MAX, 0)
                } else {
                    (-IDLE_ACCELERATION_MAX, IDLE_ACCELERATION_MAX)
                };
                self.accel[axis] = range(lo, hi) / 2000;
            }
            self.wait = range(WAIT_MIN, WAIT_MAX);
        }
        if self.wait > 0 {
            self.wait -= ONE / 50;
        }
        v[0] += self.accel[0];
        v[1] += self.accel[1];
        clamp(v, IDLE_SPEED_MAX);
    }
}
pub fn clamp(v: &mut [i32; 2], max: i32) {
    for axis in v.iter_mut() {
        *axis = (*axis).clamp(-max, max);
    }
}
/// DistanceFly.DoBuzz (targetsHeight false): farther than `distance` from the
/// target accelerates toward it on both axes, nearer accelerates away; clamp.
pub fn distance_fly(position: [i32; 2], target: [i32; 2], distance: i32, speed_max: i32, acceleration: i32, velocity: &mut [i32; 2]) {
    let dx = (position[0] as i64 - target[0] as i64) >> 8;
    let dy = (position[1] as i64 - target[1] as i64) >> 8;
    let d = distance as i64 >> 8;
    let far = dx * dx + dy * dy > d * d;
    for axis in 0..2 {
        let toward = position[axis] < target[axis];
        let sign = if toward == far { 1 } else { -1 };
        velocity[axis] += sign * acceleration;
    }
    clamp(velocity, speed_max);
}
/// DistanceFly.DoBuzz with `targetsHeight`: only x uses the far/near test,
/// while y always seeks `target.y + height`. Evidence: DistanceFly::DoBuzz
/// skips the paired y branch and converges on the height comparison.
pub fn distance_fly_height(position: [i32; 2], target: [i32; 2], distance: i32, height: i32, speed_max: i32, acceleration: i32, velocity: &mut [i32; 2]) {
    let dx = (position[0] as i64 - target[0] as i64) >> 8;
    let dy = (position[1] as i64 - target[1] as i64) >> 8;
    let d = distance as i64 >> 8;
    let far = dx * dx + dy * dy > d * d;
    let toward = position[0] < target[0];
    velocity[0] += if toward == far { acceleration } else { -acceleration };
    let goal = target[1] + height;
    if position[1] < goal {
        velocity[1] += acceleration;
    } else if position[1] > goal {
        velocity[1] -= acceleration;
    }
    clamp(velocity, speed_max);
}
