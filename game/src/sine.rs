//! Q16 sine of an angle in Q16 degrees, from a 91-sample quarter wave with linear interpolation
//! (deterministic sub-degree fling and cocoon scatter). One table for geo.rs and lifeblood.rs; they
//! each had a copy of it and of `sin`. The interpolation product is never negative (the table rises
//! over the quarter and the fraction is unsigned), so the floor and the truncating Q16 multiply agree.
use hk_sim::ONE;

const SINE: [i32; 91] = [
    0, 1144, 2287, 3430, 4572, 5712, 6850, 7987, 9121, 10252, 11380, 12505, 13626, 14742, 15855,
    16962, 18064, 19161, 20252, 21336, 22415, 23486, 24550, 25607, 26656, 27697, 28729, 29753,
    30767, 31772, 32768, 33754, 34729, 35693, 36647, 37590, 38521, 39441, 40348, 41243, 42126,
    42995, 43852, 44695, 45525, 46341, 47143, 47930, 48703, 49461, 50203, 50931, 51643, 52339,
    53020, 53684, 54332, 54963, 55578, 56175, 56756, 57319, 57865, 58393, 58903, 59396, 59870,
    60326, 60764, 61183, 61584, 61966, 62328, 62672, 62997, 63303, 63589, 63856, 64104, 64332,
    64540, 64729, 64898, 65048, 65177, 65287, 65376, 65446, 65496, 65526, 65536,
];
pub fn sin(angle: i32) -> i32 {
    let a = angle.rem_euclid(360 * ONE);
    let quadrant = a / (90 * ONE);
    let mut t = a % (90 * ONE);
    if quadrant == 1 || quadrant == 3 {
        t = 90 * ONE - t;
    }
    let i = (t / ONE) as usize;
    let v = if i == 90 {
        ONE
    } else {
        SINE[i] + psx_math::int32::mul_shr_i32(SINE[i + 1] - SINE[i], t % ONE, 16)
    };
    if quadrant >= 2 {
        -v
    } else {
        v
    }
}
