//! Exact fixed-numerator particle perspective division for the R3000 divider.
const DISTANCE:i32=2496922;
const NUMERATOR:u64=crate::KNIGHT_SCALE as u64*DISTANCE as u64;
const MIN_FAST:i32=1<<21;
// The quotient/correction bound is certified for this authored camera scale.
const _:()=assert!(crate::KNIGHT_SCALE==60693 && NUMERATOR>>6<=u32::MAX as u64);
/// Caller has already rejected depth<=ONE. Truncating both positive operands
/// by six bits gives an upper estimate: q=floor(floor(N/64)/floor(d/64))
/// is at least floor(N/d). In this domain d>=2^21, q overshoots by at most3.
/// Exact wide product comparisons remove that excess; coordinates never change.
#[inline(never)]
pub(super) fn scale(depth:i32)->i32 {
    if depth>=MIN_FAST {
        let divisor=depth as u32;
        let mut quotient=(NUMERATOR>>6)as u32/(divisor>>6);
        let mut product=quotient as u64*divisor as u64;
        // Keep the proven three corrections explicit: LLVM otherwise turns
        // the subtract loop back into a software64-bit ceiling division.
        if product>NUMERATOR {
            quotient-=1;product-=divisor as u64;
            if product>NUMERATOR {
                quotient-=1;product-=divisor as u64;
                if product>NUMERATOR {quotient-=1;}
            }
        }
        quotient as i32
    }else{(NUMERATOR as i64/depth as i64)as i32}
}
#[cfg(test)]
pub(super) fn assert_vertices(position:[i32;3],angle:i32,size:i32,camera:(i32,i32)) {
    let depth=DISTANCE+position[2];if depth<=hk_sim::ONE{return;}
    let actual=scale(depth);let expected=(NUMERATOR/depth as u64)as i32;
    assert_eq!(actual,expected,"depth {depth}");
    for corner in [[-size/2,size/2],[size/2,size/2],[-size/2,-size/2],[size/2,-size/2]] {
        let p=crate::world::debris::rotate(corner,angle);
        let legacy=|s:i32|((160+((((position[0]+p[0]-camera.0)as i64>>8)*s as i64)>>20))as i16,
            (120-((((position[1]+p[1]-camera.1)as i64>>8)*s as i64)>>20))as i16);
        let original_break=|s:i32|((160+(((position[0]as i64+p[0]as i64-camera.0 as i64)*s as i64)>>28))as i32,
            (120-(((position[1]as i64+p[1]as i64-camera.1 as i64)*s as i64)>>28))as i32);
        assert_eq!(legacy(actual),legacy(expected));assert_eq!(original_break(actual),original_break(expected));
    }
}
#[cfg(test)]mod tests {
    use super::*;
    fn check(depth:i32) {assert_eq!(scale(depth),(NUMERATOR as i64/depth as i64)as i32,"depth {depth}");}
    #[test]fn exhaustive_near_plane_and_eight_unit_source_depth_range() {
        let mut maximum_correction=0;
        for depth in hk_sim::ONE+1..=DISTANCE+8*hk_sim::ONE {
            check(depth);
            if depth>=MIN_FAST {let estimate=(NUMERATOR>>6)as u32/(depth as u32>>6);maximum_correction=maximum_correction.max(estimate-scale(depth)as u32);}
        }
        assert_eq!(maximum_correction,3);
    }
    #[test]fn every_fast_quotient_boundary_and_extreme_depth_is_exact() {
        for quotient in 1..=NUMERATOR/MIN_FAST as u64 {
            let boundary=NUMERATOR/quotient;
            for delta in [-1,0,1] {
                let depth=boundary as i64+delta;
                if depth>=MIN_FAST as i64&&depth<=i32::MAX as i64 {check(depth as i32);}
            }
        }
        for depth in [MIN_FAST-1,MIN_FAST,MIN_FAST+1,DISTANCE-1,DISTANCE,DISTANCE+1,i32::MAX-1,i32::MAX] {check(depth);}
        let mut seed=0x659abb31u32;
        for _ in 0..100000 {seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);check((seed%(i32::MAX as u32-65536)+65537)as i32);}
    }
    #[test]fn rotated_quad_projection_keeps_both_existing_rounding_orders() {
        for depth in [65537,MIN_FAST-1,MIN_FAST,MIN_FAST+1,DISTANCE-65536,DISTANCE,DISTANCE+65536,i32::MAX] {
            for angle in [0,1,45*65536,90*65536,177*65536,359*65536] {
                for camera in [(0,0),(12345,-765432),(-512*65536,512*65536)] {
                    assert_vertices([59*65536+31,-3*65536+255,depth-DISTANCE],angle,123457,camera);
                }
            }
        }
    }
}
