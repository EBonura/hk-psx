//! Each admitted Crawler runs its own checked original graph.
use crate::fsm::{Error,Machine,Phase};
use hk_format::fsm::Program;
include!("../../../data/crawler_fsm.rs");
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Crawler { machine:Machine,last_real_tick:u32,graph:u8,clock_ready:bool }
const _:()=assert!(core::mem::size_of::<Crawler>()<=88);
fn bytes(index:usize)->&'static[u8]{
    let (_,_,start,len)=GRAPHS[index];&BANK[start as usize..start as usize+len as usize]
}
impl Crawler {
    #[inline(never)]
    pub fn new(scene:usize,source:u32,seed:u32)->Option<Self>{
        let graph=GRAPHS.iter().position(|(s,id,_,_)|*s as usize==scene&&*id==source)?;
        // SAFETY: the cooker validates every complete graph before emitting
        // this immutable bank; the module transport verifies its packed hash
        // before publication. No graph bytes are mutated during execution.
        let p=unsafe{Program::from_verified(bytes(graph))};
        Some(Self{machine:Machine::new(p,seed),last_real_tick:0,graph:graph as u8,clock_ready:false})
    }
    pub fn state(&self)->u16{self.machine.state()}
    #[inline(never)]
    pub fn elapsed_real_ticks(&mut self,now:u32)->u16{
        let delta=if self.clock_ready{now.wrapping_sub(self.last_real_tick)}else{0};
        self.last_real_tick=now;self.clock_ready=true;
        // Admitted Wait durations are bounded to ten seconds. Any longer
        // absence therefore expires the timer even after this saturation.
        delta.min(u16::MAX as u32) as u16
    }
    #[inline(never)]
    pub fn step(&mut self,kernel:&mut crate::fsm_actions::WalkingActions<crate::fsm_body::Walking>)->Result<(),Error>{
        // The immutable bank was checked by cooking and module admission.
        let p=unsafe{Program::from_verified(bytes(self.graph as usize))};
        self.machine.step_walk(&p,Phase::Update,kernel)
    }
    pub fn die(&mut self){self.machine.die();}
    #[cfg(test)]
    pub fn fixture(seed:u32)->Self{
        Self{machine:Machine::new(Program::parse(bytes(0)).unwrap(),seed),last_real_tick:0,graph:0,clock_ready:false}
    }
}
