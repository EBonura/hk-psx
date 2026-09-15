//! Source-order events for the frame's certified opaque tile owners.
//! Each owned tile is inserted once and cleared when its original source draw
//! is reached. The fifteen row words then describe only strictly later draws.
pub struct Events<const DRAWS:usize> {heads:[u16;DRAWS],next:[u16;300]}
impl<const DRAWS:usize> Events<DRAWS> {
    pub const fn new()->Self {Self{heads:[0;DRAWS],next:[0;300]}}
    #[inline(never)]
    pub fn build(&mut self,owners:&[u16;300],rows:&mut[u32;15]) {
        self.heads.fill(0);rows.fill(0);
        for (tile,&rank) in owners.iter().enumerate() {
            if rank==0 {continue;}
            let draw=if rank>1024 {rank-1025}else{rank-1}as usize;
            // Valid generated owners always identify the same live source
            // that will be visited in its matching BACK or FRONT pass.
            debug_assert!(draw<DRAWS);
            self.next[tile]=self.heads[draw];self.heads[draw]=tile as u16+1;
            rows[tile/20]|=1<<(tile%20);
        }
    }
    #[inline]
    pub fn reach(&self,draw:usize,rows:&mut[u32;15]) {
        let mut entry=self.heads[draw];
        while entry!=0 {
            let tile=(entry-1)as usize;
            rows[tile/20]&=!(1<<(tile%20));entry=self.next[tile];
        }
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn events_match_strict_owner_order_through_both_source_passes() {
        let mut seed=0x318f09abu32;let mut events=Events::<480>::new();
        for round in 0..200 {
            let front:[bool;480]=core::array::from_fn(|i|(i*7+round)%5==0);
            let owners=core::array::from_fn(|_|{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);if seed&3==0{0}else{let d=(seed as usize>>8)%480;d as u16+1+if front[d]{1024}else{0}}});
            let mut rows=[u32::MAX;15];events.build(&owners,&mut rows);
            for pass in [false,true] {for d in 0..480 {
                if front[d]!=pass {continue;}
                events.reach(d,&mut rows);let rank=d as u16+1+if pass{1024}else{0};
                for y in 0..15 {let expected=(0..20).fold(0,|bits,x|bits|if owners[y*20+x]>rank{1<<x}else{0});assert_eq!(rows[y],expected,"round{round} pass{pass} draw{d} row{y}");}
            }}
            assert_eq!(rows,[0;15]);
        }
    }
    #[test]fn empty_rebuild_discards_previous_heads_and_uninitialized_links_are_never_read() {
        let mut events=Events::<480>::new();let mut rows=[0;15];events.build(&[480;300],&mut rows);events.reach(479,&mut rows);assert_eq!(rows,[0;15]);
        events.build(&[0;300],&mut rows);for d in 0..480 {events.reach(d,&mut rows);}assert_eq!(rows,[0;15]);
    }
}
