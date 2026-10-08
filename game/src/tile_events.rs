//! Source-order events for the frame's certified opaque tile owners.
//! Each (source draw, occupied row) gets one node. Reaching the source clears
//! every cell it owns in that row, leaving only strictly later draw owners.
const FRONT_OFFSET:usize=1024;
pub struct Events<const DRAWS:usize> {heads:[u16;2048],next:[u16;300],masks:[u32;300]}
impl<const DRAWS:usize> Events<DRAWS> {
 pub const fn new()->Self{Self{heads:[0;2048],next:[0;300],masks:[0;300]}}
 #[inline(never)]
 pub fn build(&mut self,owners:&[u16;300],rows:&mut[u32;15]){
  self.heads.fill(0);let mut used=0usize;
  for (row,cells) in owners.chunks_exact(20).enumerate(){
   let mut occupied=0;let mut bit=1;
   for &rank in cells{
    if rank!=0{
     occupied|=bit;
     let draw=if rank>1024{rank-1025}else{rank-1}as usize;
     debug_assert!(draw<DRAWS);
     // Rows are visited monotonically. A source's latest node is its
     // only possible node for this row, even when its cells are separated
     // by other sources or empty cells. Older rows cannot match.
     let head_index=draw+usize::from(rank>1024)*FRONT_OFFSET;
     let head=self.heads[head_index];
     if head!=0 && self.masks[head as usize-1]>>20==row as u32{
      self.masks[head as usize-1]|=bit;
     }else{
      // Each node owns at least one distinct cell, hence at most 300 nodes.
      self.masks[used]=((row as u32)<<20)|bit;
      self.next[used]=head;self.heads[head_index]=used as u16+1;used+=1;
     }
    }
    bit<<=1;
   }
   rows[row]=occupied;
  }
 }
 #[inline]
 pub fn reach(&self,draw:usize,front:bool,rows:&mut[u32;15]){
  let mut entry=self.heads[draw+usize::from(front)*FRONT_OFFSET];
  while entry!=0{
   let index=entry as usize-1;let word=self.masks[index];
   rows[(word>>20)as usize]&=!(word&0xfffff);entry=self.next[index];
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
                events.reach(d,pass,&mut rows);let rank=d as u16+1+if pass{1024}else{0};
                for y in 0..15 {let expected=(0..20).fold(0,|bits,x|bits|if owners[y*20+x]>rank{1<<x}else{0});assert_eq!(rows[y],expected,"round{round} pass{pass} draw{d} row{y}");}
            }}
            assert_eq!(rows,[0;15]);
        }
    }
    #[test]fn empty_rebuild_discards_previous_heads_and_uninitialized_links_are_never_read() {
        let mut events=Events::<480>::new();let mut rows=[0;15];events.build(&[480;300],&mut rows);events.reach(479,false,&mut rows);assert_eq!(rows,[0;15]);
        events.build(&[0;300],&mut rows);for d in 0..480 {events.reach(d,false,&mut rows);events.reach(d,true,&mut rows);}assert_eq!(rows,[0;15]);
    }

    #[test]
    fn row_nodes_coalesce_separated_cells_and_preserve_other_row_bits() {
        let mut events=Events::<480>::new();
        let owners=core::array::from_fn(|tile|match tile%20 {
            0|4|19=>480,
            1|3|18=>1025,
            _=>0,
        });
        let mut rows=[u32::MAX;15];events.build(&owners,&mut rows);
        // Two sources across fifteen rows require thirty nodes, even though
        // each owns several non-adjacent cells in every row.
        for draw in [0,479] {
            let mut entry=events.heads[draw+if draw==0 {FRONT_OFFSET}else{0}];let mut seen=0u16;let mut count=0;
            while entry!=0 {
                let i=entry as usize-1;let row=events.masks[i]>>20;
                assert_eq!(seen&(1<<row),0);seen|=1<<row;count+=1;
                entry=events.next[i];
            }
            assert_eq!(count,15);assert_eq!(seen,0x7fff);
        }
        for row in &mut rows {*row|=0xfff00000;}
        events.reach(479,false,&mut rows);
        assert_eq!(rows,[0xfff00000|(1<<1)|(1<<3)|(1<<18);15]);
        events.reach(479,false,&mut rows);events.reach(0,true,&mut rows);
        assert_eq!(rows,[0xfff00000;15]);
        assert_eq!(core::mem::size_of::<Events<480>>(),5896);
    }
    #[test]
    fn maximum_nodes_and_rebuilds_match_independent_owner_scan() {
        let mut events=Events::<480>::new();let mut rows=[0;15];
        // Every cell has a distinct source: this exercises all 300 nodes.
        // Repeat with dense sharing and no owners to expose stale links.
        for mode in 0..4 {
            let owners=core::array::from_fn(|tile|match mode {
                0=>tile as u16+1,
                1=>if tile%3==0{0}else{480},
                2=>if tile%2==0{1025}else{1504},
                _=>0,
            });
            events.build(&owners,&mut rows);let mut remaining=owners;
            // Correctness also holds when sources are reached repeatedly or
            // out of order; this independently checks each source's ownership.
            for draw in (0..480).rev() {
                events.reach(draw,false,&mut rows);events.reach(draw,true,&mut rows);
                for owner in &mut remaining {
                    if *owner==draw as u16+1||*owner==draw as u16+1025 {*owner=0;}
                }
                for y in 0..15 {
                    assert_eq!(rows[y],(0..20).fold(0,|bits,x|bits|if remaining[y*20+x]!=0{1<<x}else{0}));
                }
            }
            assert_eq!(rows,[0;15]);
        }
    }

    #[test]
    fn layer_passes_keep_front_owners_until_back_geometry_is_drawn() {
        let mut events=Events::<480>::new();
        let mut owners=[0u16;300];
        // Back draw 0 owns the left tile; front draw 0 owns the right tile.
        // The front source is deliberately visited at the same authored index:
        // layer order, rather than source index, controls retirement.
        owners[0]=1;
        owners[1]=1025;
        let mut rows=[0u32;15];
        events.build(&owners,&mut rows);
        // Back pass: retire only back owners. The front tile remains a later
        // owner and therefore still hides any back primitive beneath it.
        events.reach(0,false,&mut rows);
        assert_eq!(rows[0],1<<1);
        // Front pass: now retire the front owner and finish the frame.
        events.reach(0,true,&mut rows);
        assert_eq!(rows[0],0);
    }
}
