//! Bounded topology and motion preload plan. GPU banks retain direct exits;
//! RAM priority follows source movement without a speculative collision update.
pub const EMPTY:usize=usize::MAX;
const ONE:i32=65536;
#[derive(Clone,Copy)]
pub struct Cell<'a> {pub bounds:[i32;4],pub neighbours:&'a[usize],pub scene:usize,pub flat_floor_heights:&'a[i32]}
pub struct Plan {pub gpu:[usize;3],pub gpu_len:usize,pub ram:[usize;4],pub ram_len:usize}

impl Plan {
    /// Give a required boundary destination first priority without making
    /// already-read useful successors obsolete while its decode/upload finishes.
    /// GPU priorities remain separate; exactly four unique RAM targets fit.
    pub fn with_demand(&self,id:usize)->Self {
        if id==EMPTY {return Self {gpu:self.gpu,gpu_len:self.gpu_len,ram:self.ram,ram_len:self.ram_len};}
        let mut next=Self {gpu:self.gpu,gpu_len:self.gpu_len,ram:[EMPTY;4],ram_len:1};
        next.ram[0]=id;
        for &region in &self.ram[..self.ram_len] {
            if region==EMPTY || next.ram.contains(&region) {continue;}
            if next.ram_len==next.ram.len() {break;}
            next.ram[next.ram_len]=region;next.ram_len+=1;
        }
        next
    }
}

/// Current contact state and source-derived body-bottom offset.
#[derive(Clone,Copy)]
pub struct Motion {pub grounded:bool,pub bottom:i32,pub vy:i32}
// Velocity is Q16 world units/second, matching Player::step's vy/60.
// This short lead only changes approaching direct-exit priority; it cannot
// cover a whole CD read/decode and does not predict gravity or a future jump.
const VERTICAL_HORIZON_TICKS:i64=12;
fn approaching_vertical(a:[i32;4],b:[i32;4],x:i32,y:i32,m:Motion)->bool {
    // At a horizontal seam/corner the destination is ambiguous; retain the
    // established ranking instead of inventing a horizontal trajectory.
    if m.grounded || x<=b[0] || x>=b[2] {return false;}
    let (distance,speed)=if m.vy<0 && b[3]<=a[1] {
        ((y as i64-b[3] as i64).max(0),-(m.vy as i64))
    }else if m.vy>0 && b[1]>=a[3] {
        ((b[1] as i64-y as i64).max(0),m.vy as i64)
    }else{return false;};
    // Four world units is the source terminal fall's maximum12-tick reach;
    // retain a spatial bound even if another caller supplies extreme velocity.
    distance<=4*ONE as i64 && distance*60<=speed*VERTICAL_HORIZON_TICKS
}
fn adjacent(a:[i32;4],b:[i32;4])->bool {
    let x=b[2].min(a[2])-b[0].max(a[0]);
    let y=b[3].min(a[3])-b[1].max(a[1]);
    // Corner-only contacts require an intervening cell, not a direct GPU bank.
    x>=0&&y>=0&&(x>0||y>0)
}
fn distance(b:[i32;4],x:i32,y:i32)->(i32,i32) {
    ((b[0]-x).max(x-b[2]).max(0),(b[1]-y).max(y-b[3]).max(0))
}
fn forward(a:[i32;4],b:[i32;4],facing:i32)->bool {
    if facing>0 {b[0]>=a[2]}else{b[2]<=a[0]}
}
pub fn plan<'a>(index:usize,x:i32,y:i32,facing:i32,cell:impl Fn(usize)->Cell<'a>)->Plan {
    plan_inner(index,x,y,facing,None,cell)
}
pub fn plan_motion<'a>(index:usize,x:i32,y:i32,facing:i32,motion:Motion,cell:impl Fn(usize)->Cell<'a>)->Plan {
    plan_inner(index,x,y,facing,Some(motion),cell)
}
fn plan_inner<'a>(index:usize,x:i32,y:i32,facing:i32,motion:Option<Motion>,cell:impl Fn(usize)->Cell<'a>)->Plan {
    let current=cell(index);let mut p=Plan {gpu:[EMPTY;3],gpu_len:0,ram:[EMPTY;4],ram_len:0};
    let mut scores=[i32::MAX;3];
    for &id in current.neighbours {
        let next=cell(id);let b=next.bounds;
        if id==index||next.scene!=current.scene||!adjacent(current.bounds,b)||p.gpu.contains(&id) {continue;}
        let(dx,dy)=distance(b,x,y);
        let horizontal=b[0]>=current.bounds[2]||b[2]<=current.bounds[0];
        let score=if !horizontal && motion.is_some_and(|m|approaching_vertical(current.bounds,b,x,y,m)) {
            -2*ONE
        }else if horizontal {
            (if forward(current.bounds,b,facing)&&dy==0 {-ONE}else{dx+2*ONE})+dy
        }else{dx+dy+if b[3]<=current.bounds[1] {ONE}else{3*ONE}};
        for slot in 0..3 {if score<scores[slot] {
            for i in (slot+1..3).rev() {scores[i]=scores[i-1];p.gpu[i]=p.gpu[i-1];}
            scores[slot]=score;p.gpu[slot]=id;p.gpu_len=(p.gpu_len+1).min(3);break;
        }}
    }
    p.ram[..p.gpu_len].copy_from_slice(&p.gpu[..p.gpu_len]);p.ram_len=p.gpu_len;
    let mut second=None;
    // Only three admitted direct exits are expanded; no all-room search.
    for &via in &p.gpu[..p.gpu_len] {
        let through=cell(via);
        for &id in through.neighbours {
            let next=cell(id);let b=next.bounds;
            if id==index||next.scene!=current.scene||p.ram.contains(&id)
                ||adjacent(current.bounds,b)||!adjacent(through.bounds,b)
                ||!forward(through.bounds,b,facing) {continue;}
            let(dx,dy)=distance(b,x,y);
            // Distance to the prospective landing/traversal cell favors the
            // continuation below a ledge over a distant upper continuation.
            // Prefer a same-height continuation only through a complete static
            // floor at the exact foot height. No proof means old ranking.
            let supported=dy==0 && motion.is_some_and(|m|m.grounded &&
                y.checked_add(m.bottom).is_some_and(|foot|through.flat_floor_heights.contains(&foot)));
            let rank=(motion.is_some_and(|m|m.grounded)&&!supported,dx+dy*2,id);
            if second.is_none_or(|(best,_)|rank<best) {second=Some((rank,id));}
        }
    }
    if let Some((_,id))=second {p.ram[p.ram_len]=id;p.ram_len+=1;}
    p
}
#[cfg(test)] mod tests {
    use super::*;
    fn cell<'a>(bounds:[i32;4],neighbours:&'a[usize])->Cell<'a> {Cell {bounds:bounds.map(|x|x*ONE),neighbours,scene:0,flat_floor_heights:&[]}}
    #[test] fn lower_exit_continuation_is_preloaded_before_falling_into_it() {
        let cells=[cell([0,10,10,20],&[1,2]),cell([0,0,10,10],&[0,3]),
            cell([10,10,20,20],&[0,3,4]),cell([10,0,20,10],&[1,2]),cell([20,10,30,20],&[2])];
        let p=plan(0,2*ONE,11*ONE,1,|i|cells[i]);
        assert_eq!(&p.gpu[..p.gpu_len],&[2,1]);
        assert_eq!(&p.ram[..p.ram_len],&[2,1,3]);
        assert!(!p.gpu.contains(&3));
    }
    #[test] fn reverse_travel_selects_a_second_hop_in_the_reverse_direction() {
        let cells=[cell([0,0,10,10],&[1]),cell([10,0,20,10],&[0,2]),cell([20,0,30,10],&[1])];
        let p=plan(2,25*ONE,5*ONE,-1,|i|cells[i]);assert_eq!(&p.ram[..p.ram_len],&[1,0]);
    }
    #[test] fn corner_edges_scene_changes_and_duplicate_links_never_claim_direct_banks() {
        let cells=[cell([0,0,10,10],&[1,1,2,3]),cell([10,0,20,10],&[0]),
            cell([10,10,20,20],&[0]),Cell {scene:1,..cell([0,10,10,20],&[0])}];
        let p=plan(0,5*ONE,5*ONE,1,|i|cells[i]);assert_eq!(&p.gpu[..p.gpu_len],&[1]);
        assert_eq!(&p.ram[..p.ram_len],&[1]);
    }
    #[test] fn current_room_and_direct_targets_cannot_reappear_as_second_hops() {
        let cells=[cell([0,0,10,10],&[1,2]),cell([10,0,20,10],&[0,2]),cell([0,10,20,20],&[0,1])];
        let p=plan(0,5*ONE,5*ONE,1,|i|cells[i]);assert_eq!(p.ram_len,p.gpu_len);
        assert!(!p.ram.contains(&0));
    }
}

#[cfg(test)] mod demand_tests {
    use super::*;
    fn p()->Plan{Plan{gpu:[1,2,3],gpu_len:3,ram:[1,2,3,4],ram_len:4}}
    #[test]fn already_requested_destination_moves_first_without_discarding_second_hop(){
        for id in 1..=4{let q=p().with_demand(id);assert_eq!(q.ram[0],id);assert_eq!(q.ram_len,4);
            for old in 1..=4{assert_eq!(q.ram.iter().filter(|&&r|r==old).count(),1);}
            assert_eq!(q.gpu,[1,2,3]);assert_eq!(q.gpu_len,3);}
    }
    #[test]fn new_destination_only_displaces_lowest_priority_when_full(){
        let q=p().with_demand(5);assert_eq!(q.ram,[5,1,2,3]);assert_eq!(q.ram_len,4);
        assert_eq!(q.with_demand(5).ram,q.ram);
        let short=Plan{gpu:[1,EMPTY,EMPTY],gpu_len:1,ram:[1,2,EMPTY,EMPTY],ram_len:2};
        let q=short.with_demand(5);assert_eq!(q.ram,[5,1,2,EMPTY]);assert_eq!(q.ram_len,3);
    }
    #[test]fn empty_demand_and_empty_plans_are_bounded(){
        let q=p().with_demand(EMPTY);assert_eq!(q.ram,p().ram);
        let empty=Plan{gpu:[EMPTY;3],gpu_len:0,ram:[EMPTY;4],ram_len:0};
        let q=empty.with_demand(0);assert_eq!(q.ram,[0,EMPTY,EMPTY,EMPTY]);assert_eq!(q.ram_len,1);
    }
}


#[cfg(test)] mod flat_support_tests {
    use super::*;
    fn c<'a>(b:[i32;4],n:&'a[usize],floors:&'a[i32])->Cell<'a>{
        Cell{bounds:b.map(|v|v*ONE),neighbours:n,scene:0,flat_floor_heights:floors}
    }
    fn contact(grounded:bool)->Motion{Motion{grounded,bottom:-91136,vy:0}}
    #[test] fn source13_at_foot10_enables_grounded14_without_changing_direct_banks() {
        // 2,12,4,13,28,5,14: original bounds and admitted through13 heights.
        let cells=[c([48,11,72,27],&[1,2,3,4],&[655360]),c([24,11,48,27],&[0],&[]),
            c([48,-5,72,11],&[0,5],&[]),c([72,11,84,27],&[0,5,6],&[655360,917504]),
            c([48,27,72,43],&[0],&[]),c([72,-5,96,11],&[2,3],&[]),c([84,11,96,27],&[3],&[655360])];
        let old=plan(0,60*ONE,746496,1,|i|cells[i]);assert_eq!(old.ram[3],5);
        let new=plan_motion(0,60*ONE,746496,1,contact(true),|i|cells[i]);
        assert_eq!(new.gpu,old.gpu);assert_eq!(new.ram[..3],old.ram[..3]);assert_eq!(new.ram[3],6);
        // Exact fixed-point match, not a body-height or slope tolerance.
        for y in [746495,746497]{let p=plan_motion(0,60*ONE,y,1,contact(true),|i|cells[i]);assert_eq!(p.ram[3],5);}
        let air=plan_motion(0,60*ONE,746496,1,contact(false),|i|cells[i]);assert_eq!(air.ram,old.ram);
        let reverse=plan_motion(0,60*ONE,746496,-1,contact(true),|i|cells[i]);assert!(!reverse.ram.contains(&6));
    }
    #[test] fn source22_without_floor10_keeps8_even_when20_starts_grounded() {
        // 20,18,22,7,21,8,23: source22 only has whole-width support at32.
        let cells=[c([132,11,144,19],&[1,2,3,4],&[]),c([120,11,132,19],&[0],&[]),
            c([144,11,156,27],&[0,5,6],&[2097152]),c([120,-5,144,11],&[0,5],&[]),
            c([132,19,144,27],&[0],&[]),c([144,-5,168,11],&[2,3],&[]),c([156,11,168,27],&[2],&[2097152])];
        for grounded in [true,false]{let old=plan(0,132*ONE,746496,1,|i|cells[i]);
            let new=plan_motion(0,132*ONE,746496,1,contact(grounded),|i|cells[i]);
            assert_eq!(new.ram,old.ram);assert_eq!(new.ram[3],5);assert_eq!(new.gpu,old.gpu);}
        // Falling/rising height changes never reorder direct requests in this candidate.
        for y in [12*ONE,15*ONE,18*ONE]{let old=plan(0,135*ONE,y,1,|i|cells[i]);
            let new=plan_motion(0,135*ONE,y,1,contact(false),|i|cells[i]);assert_eq!(new.ram,old.ram);}
    }
    #[test] fn absent_proof_retains_old_ranking_for_steps_gaps_and_dynamic_surfaces() {
        let cells=[c([0,10,10,20],&[1,2],&[]),c([0,0,10,10],&[0,3],&[]),
            c([10,10,20,20],&[0,3,4],&[]),c([10,0,20,10],&[1,2],&[]),c([20,10,30,20],&[2],&[])];
        for x in [ONE,5*ONE,9*ONE]{for facing in [-1,1]{let old=plan(0,x,11*ONE,facing,|i|cells[i]);
            let new=plan_motion(0,x,11*ONE,facing,contact(true),|i|cells[i]);assert_eq!(new.ram,old.ram);}}
    }
}

#[cfg(test)] mod vertical_motion_tests {
    use super::*;
    fn c<'a>(b:[i32;4],n:&'a[usize])->Cell<'a>{Cell{bounds:b.map(|v|v*ONE),neighbours:n,scene:0,flat_floor_heights:&[]}}
    fn m(vy:i32)->Motion{Motion{grounded:false,bottom:-91136,vy}}
    #[test]fn source21_fall_prioritizes20_over_facing22_only_near_boundary(){
        // Logged IDs21,22,19,20,35; zero-based fixture indices0..4.
        let cells=[c([132,19,144,27],&[1,2,3,4]),c([144,11,156,27],&[0]),
            c([120,19,132,27],&[0]),c([132,11,144,19],&[0]),c([132,27,144,35],&[0])];
        let old=plan(0,134*ONE,21*ONE,1,|i|cells[i]);assert_eq!(old.gpu[0],1);
        let near=plan_motion(0,134*ONE,21*ONE,1,m(-15*ONE),|i|cells[i]);
        assert_eq!(near.gpu[0],3);assert_eq!(near.ram[0],3);
        let far=plan_motion(0,134*ONE,23*ONE,1,m(-15*ONE),|i|cells[i]);assert_eq!(far.gpu,plan(0,134*ONE,23*ONE,1,|i|cells[i]).gpu);
        for motion in [m(0),Motion{grounded:true,..m(-15*ONE)},m(15*ONE)]{
            assert_eq!(plan_motion(0,134*ONE,21*ONE,1,motion,|i|cells[i]).gpu,old.gpu);
        }
    }
    #[test]fn source21_rise_prioritizes35_and_source33_fall_prioritizes19(){
        let a=[c([132,19,144,27],&[1,2]),c([120,19,132,27],&[0]),c([132,27,144,35],&[0])];
        assert_eq!(plan(0,142*ONE,25*ONE,-1,|i|a[i]).gpu[0],1);
        assert_eq!(plan_motion(0,142*ONE,25*ONE,-1,m(12*ONE),|i|a[i]).gpu[0],2);
        let b=[c([120,27,132,35],&[1,2]),c([132,27,144,35],&[0]),c([120,19,132,27],&[0])];
        assert_eq!(plan_motion(0,129*ONE,29*ONE,1,m(-20*ONE),|i|b[i]).gpu[0],2);
    }
    #[test]fn exact_horizon_and_spatial_bounds_do_not_thrash_distant_exits(){
        let a=[0,0,10,10].map(|v|v*ONE);let below=[0,-10,10,0].map(|v|v*ONE);
        assert!(approaching_vertical(a,below,5*ONE,2*ONE,m(-10*ONE)));
        assert!(!approaching_vertical(a,below,5*ONE,2*ONE+1,m(-10*ONE)));
        assert!(!approaching_vertical(a,below,5*ONE,5*ONE,m(i32::MIN)));
        for x in [-ONE,0,10*ONE,11*ONE]{assert!(!approaching_vertical(a,below,x,ONE,m(-20*ONE)));}
        let side=[10,0,20,10].map(|v|v*ONE);assert!(!approaching_vertical(a,side,15*ONE,ONE,m(-20*ONE)));
    }
    #[test]fn corner_only_and_other_scene_are_never_promoted(){
        let cells=[c([0,0,10,10],&[1,2,3]),c([10,-10,20,0],&[0]),
            Cell{scene:1,..c([0,-10,10,0],&[0])},c([10,0,20,10],&[0])];
        let p=plan_motion(0,9*ONE,ONE,1,m(-20*ONE),|i|cells[i]);assert_eq!(&p.gpu[..p.gpu_len],&[3]);
    }
}
