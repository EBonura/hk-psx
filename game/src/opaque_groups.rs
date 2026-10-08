//! Optional joint opaque seam proofs, bound to every original member.
//! Group memberships are bank-pool IDs in cooked data and local draw IDs after
//! resolution. Claim only after all members pass the caller's live/material
//! guards, using member0's projected vertex0 and the earliest member rank.
#[cfg(test)]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(C)]
pub struct Group {pub certificate:u16,pub members:[u16;4]}
#[cfg(test)]
#[derive(Clone,Copy)]
#[repr(C)]
pub struct GroupCert {pub gx:i16,pub gy:i16,pub width:u16,pub height:u16,pub offset:u32}
pub const NO_MEMBER:u16=u16::MAX;
#[cfg(not(test))]
pub use hk_format::coverage::{CoverageCert as GroupCert,CoverageGroup as Group};
#[cfg(not(test))]
pub const GROUP_POOL_CAPACITY:usize=crate::disc::COVERAGE_GROUP_POOL_CAPACITY;
/// Resolve complete membership in one region. A missing member disables the
/// group; sharing only its anchor across regions is insufficient evidence.
pub fn resolve(group:&Group,pool_ids:&[u16])->Option<Group> {
    let mut out=Group{certificate:group.certificate,members:[NO_MEMBER;4]};let mut count=0;let mut ended=false;
    for (slot,&pool) in group.members.iter().enumerate() {
        if pool==NO_MEMBER {ended=true;continue;}
        if ended || group.members[..slot].contains(&pool){return None;}
        let local=pool_ids.iter().position(|&v|v==pool)?;
        if local>=NO_MEMBER as usize{return None;}
        out.members[slot]=local as u16;count+=1;
    }
    if count<2 {None}else{Some(out)}
}
/// Constant-time member lookup after the region's first-occurrence inverse
/// map has been built. Missing/out-of-range members retain normal rendering.
pub fn resolve_indexed(group:&Group,inverse:&[u16])->Option<Group> {
    let mut out=*group;let mut count=0;let mut ended=false;
    for (slot,member) in out.members.iter_mut().enumerate() {
        let pool=*member;
        if pool==NO_MEMBER {ended=true;continue;}
        if ended || group.members[..slot].contains(&pool){return None;}
        let &local=inverse.get(pool as usize)?;
        if local==NO_MEMBER {return None;}
        *member=local;count+=1;
    }
    if count<2 {None}else{Some(out)}
}
/// Compact optional validation accumulator. `eligible` must include current
/// enabled/full-opacity/nonstreamed/FRONT/SOLID_BLACK/BLACK_AVERAGE checks.
/// Exact bank/region geometry leases remain the caller's responsibility.
pub struct Guard {scale:i32,rank:u16,origin:(i32,i32),count:u8,valid:bool}
impl Guard {
    pub fn new()->Self{Self{scale:0,rank:u16::MAX,origin:(0,0),count:0,valid:true}}
    pub fn push(&mut self,rank:u16,scale:i32,eligible:bool,v:&[(i32,i32);4]) {
        if !self.valid{return;}
        if !eligible || scale<=0 || self.count==4 || (self.count!=0&&self.scale!=scale)
            || v.iter().any(|&(x,y)|!(-1024..=1023).contains(&x)||!(-1024..=1023).contains(&y))
            || [(0,1),(1,2),(2,0),(1,3),(3,2)].iter().any(|&(a,b)|(v[a].0-v[b].0).abs()>1023||(v[a].1-v[b].1).abs()>511)
        {self.valid=false;return;}
        if self.count==0{self.scale=scale;self.origin=v[0];}
        self.rank=self.rank.min(rank);self.count+=1;
    }
    pub fn finish(&self)->Option<((i32,i32),u16)>{if self.valid&&self.count>=2{Some((self.origin,self.rank))}else{None}}
}
#[inline]
pub fn intersects(certs:&[GroupCert],id:u16,origin:(i32,i32))->bool {
    certs.get(id as usize).is_some_and(|c|bounds(c,origin).is_some())
}
#[inline]
fn bounds(c:&GroupCert,(ox,oy):(i32,i32))->Option<[i32;4]> {
    let l=((ox+c.gx as i32*4+15)>>4).max(0);let t=((oy+c.gy as i32*4+15)>>4).max(0);
    let r=((ox+(c.gx as i32+c.width as i32)*4+15)>>4).min(20);let b=((oy+(c.gy as i32+c.height as i32)*4+15)>>4).min(15);
    if l<r&&t<b{Some([l,t,r,b])}else{None}
}
/// Upgrade only owners earlier than every member of this group. Unlike the
/// ordinary descending single-source collector, occupied cells are not final.
#[inline(never)]
pub fn claim(certs:&[GroupCert],bits:&[u32],id:u16,origin:(i32,i32),rank:u16,owners:&mut[u16;300])->(u32,u32) {
    let Some(c)=certs.get(id as usize)else{return(0,0);};claim_mask(c,bits,origin,rank,owners)
}
fn claim_mask(c:&GroupCert,bits:&[u32],origin:(i32,i32),rank:u16,owners:&mut[u16;300])->(u32,u32) {
    let Some([l,t,r,b])=bounds(c,origin)else{return(0,0);};let(mut added,mut reads)=(0,0);
    let x=((l*16-origin.0)>>2)-c.gx as i32;let y=((t*16-origin.1)>>2)-c.gy as i32;
    let mut row_bit=c.offset+y as u32*c.width as u32+x as u32;
    for row in t..b {
        let mut bit=row_bit;
        for col in l..r {
            let owner=&mut owners[row as usize*20+col as usize];
            if *owner<rank {reads+=1;if bits[bit as usize>>5]&(1<<(bit&31))!=0{*owner=rank;added+=1;}}
            bit+=4;
        }
        row_bit+=c.width as u32*4;
    }
    (added,reads)
}
const _:()={assert!(core::mem::size_of::<Group>()==10);assert!(core::mem::size_of::<GroupCert>()==12);};
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn inverse_resolution_matches_linear_first_occurrence_and_rejection(){
        let pools=[9,3,12,9];let mut inverse=[NO_MEMBER;16];
        for (i,&pool) in pools.iter().enumerate(){if inverse[pool as usize]==NO_MEMBER{inverse[pool as usize]=i as u16;}}
        for members in [[9,3,12,NO_MEMBER],[9,12,NO_MEMBER,NO_MEMBER],[9,9,NO_MEMBER,NO_MEMBER],
            [9,NO_MEMBER,3,NO_MEMBER],[9,15,NO_MEMBER,NO_MEMBER],[9,16,NO_MEMBER,NO_MEMBER],[9,NO_MEMBER,NO_MEMBER,NO_MEMBER]] {
            let group=Group{certificate:7,members};assert_eq!(resolve_indexed(&group,&inverse),resolve(&group,&pools));
        }
    }
    #[test]fn all_members_must_resolve_in_this_region(){let g=Group{certificate:7,members:[9,3,12,NO_MEMBER]};assert_eq!(resolve(&g,&[3,8,9,12]).unwrap().members,[2,0,3,NO_MEMBER]);assert!(resolve(&g,&[3,8,9]).is_none());assert!(resolve(&Group{members:[9,NO_MEMBER,3,NO_MEMBER],..g},&[3,9]).is_none());assert!(resolve(&Group{members:[9,9,NO_MEMBER,NO_MEMBER],..g},&[9]).is_none());}
    #[test]fn guards_reject_missing_state_scale_and_wrapped_edges(){let v=[(0,0),(30,0),(0,30),(30,30)];let mut g=Guard::new();g.push(1100,60693,true,&v);assert!(g.finish().is_none());g.push(1090,60693,true,&v);assert_eq!(g.finish(),Some(((0,0),1090)));for(kind,scale,eligible)in[(0,60692,true),(0,60693,false),(1,60693,true),(2,60693,true)]{let mut g=Guard::new();g.push(1100,60693,true,&v);let mut q=v;if kind==1{q[0].0=1024;}if kind==2{q[0].1=-512;}g.push(1090,scale,eligible,&q);assert!(g.finish().is_none());}}
    #[test]fn all_phase_floor_lookup_only_upgrades_earlier_owners(){let c=GroupCert{gx:-5,gy:-3,width:29,height:17,offset:32};let mut bits=[0u32;17];for y in 0..17{for x in 0..29{if (x+y*3)%7!=0{let bit=32+y*29+x;bits[bit/32]|=1<<(bit%32);}}}for ox in(-320..320).step_by(17){for oy in(-200..200).step_by(13){let mut owners=core::array::from_fn(|i|if i%3==0{1200}else{2});let mut expected=owners;for y in 0..15{for x in 0..20{let xx=((x as i32*16-ox)>>2)-c.gx as i32;let yy=((y as i32*16-oy)>>2)-c.gy as i32;if 0<=xx&&xx<c.width as i32&&0<=yy&&yy<c.height as i32{let bit=32+yy as usize*29+xx as usize;if bits[bit/32]&(1<<(bit%32))!=0{expected[y*20+x]=expected[y*20+x].max(1100);}}}}claim_mask(&c,&bits,(ox,oy),1100,&mut owners);assert_eq!(owners,expected);}}}
}
