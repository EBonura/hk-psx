//! Exact64/32 division when the caller proves the quotient fits one word.
const RADIX:u32=1<<16;
/// Append one base65536 dividend digit to a normalized remainder. Divisor's
/// upper digit is>=32768, so the upper-digit estimate is at most2 too large.
#[inline(always)]
fn digit(remainder:u32,next:u32,divisor:u32)->(u32,u32) {
    let upper=divisor>>16;let lower=divisor&65535;
    let mut q=remainder/upper;let mut r=remainder%upper;
    // Explicit bounded corrections prevent loop-idiom lowering from
    // reintroducing a software wide division on the R3000 target.
    if q>=RADIX||q*lower>(r<<16)+next {
        q-=1;r+=upper;
        if r<RADIX && (q>=RADIX||q*lower>(r<<16)+next) {q-=1;}
    }
    // The exact nonnegative remainder fits32 bits. Low-word cancellation is
    // sufficient even though the two intermediate products can be wider.
    let rem=(remainder<<16).wrapping_add(next).wrapping_sub(q.wrapping_mul(divisor));
    (q,rem)
}
/// Requires divisor>0 and numerator's high word<divisor. The two quotient
/// digits use two native divisions; no approximate reciprocal changes rounding.
#[inline(never)]
pub(super) fn quotient32(numerator:u64,divisor:u32)->u32 {
    debug_assert!(divisor>0 && numerator>>32<divisor as u64);
    let shift=divisor.leading_zeros();let normalized=divisor<<shift;
    let hi=(numerator>>32)as u32;let lo=numerator as u32;
    let carry=if shift==0 {hi}else{(hi<<shift)|(lo>>(32-shift))};let tail=lo<<shift;
    let (upper,remainder)=digit(carry,tail>>16,normalized);
    let (lower,_)=digit(remainder,tail&65535,normalized);
    (upper<<16)|lower
}
#[cfg(test)]mod tests {
    use super::*;
    fn check(n:u64,d:u32){assert_eq!(quotient32(n,d),(n/d as u64)as u32,"n={n} d={d}");}
    #[test]fn two_million_full_numerators_cover_every_normalization_shift(){
        let mut seed=0x9ab43f876546bcadu64;
        for i in 0..2000000 {
            seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;
            let d=((seed as u32)>>(i%32)).max(1);let hi=((seed>>32)as u32)%d;
            check(((hi as u64)<<32)|seed as u32 as u64,d);
        }
    }
    #[test]fn every_short_divisor_and_quotient_digit_carry_boundary(){
        for d in (1u32..=65536).chain([65537,0x7fffffff,0x80000000,0x8000ffff,0xffff0001,u32::MAX]) {
            for q in [0u32,1,65534,65535,65536,65537,0x7fffffff,0xfffffffe,u32::MAX] {
                for rem in [0,d/2,d-1] {check(q as u64*d as u64+rem as u64,d);}
            }
        }
    }
    #[test]fn normalized_digit_corrections_are_bounded_and_remainders_exact(){
        let mut seed=0x32189ab1u32;let mut counts=[0;3];
        for _ in 0..1000000 {
            seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let divisor=seed|0x80000000;
            seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let remainder=seed%divisor;
            seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let next=seed&65535;
            let estimate=remainder/(divisor>>16);let(q,r)=digit(remainder,next,divisor);let n=((remainder as u64)<<16)|next as u64;
            assert_eq!(q,(n/divisor as u64)as u32);assert_eq!(r,(n%divisor as u64)as u32);
            assert!(estimate>=q&&estimate-q<=2);counts[(estimate-q)as usize]+=1;
        }
        assert!(counts.into_iter().all(|n|n>100));
    }
}
