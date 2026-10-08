//! Scripted gate tour for `gate-tour` builds: once gameplay starts, stand for
//! `DWELL` ticks after each entry walk, then take the next listed gate through
//! the ordinary gate path, so every load is a real scene-gate transition.
//! Pad input is ignored while the tour runs. Ordinary builds compile nothing.
#![allow(dead_code)]
#[cfg(feature="gate-tour")]
mod imp {
    include!("gate_tour_list.rs");
    const DWELL:u32=match option_env!("HK_TOUR_DWELL") {Some(s)=>parse(s),None=>150};
    const fn parse(s:&str)->u32 {let b=s.as_bytes();let mut i=0;let mut v=0;while i<b.len() {v=v*10+(b[i]-b'0') as u32;i+=1;}v}
    #[no_mangle] pub static mut HK_TOUR_STEP:u32=0;
    #[no_mangle] pub static mut HK_TOUR_MISSES:u32=0;
    #[no_mangle] pub static mut HK_TOUR_DONE:u32=0;
    static mut IDLE:u32=0;
    /// Ticks the Knight has stood in the scene since its entry finished.
    pub fn tick(entering:bool) {unsafe {if entering {IDLE=0;} else {IDLE=IDLE.saturating_add(1);}}}
    pub fn pad(_bits:u16)->u16 {0}
    /// The tour's next (target scene, target region), if it is due now.
    pub fn due(scene:usize)->Option<(usize,usize)> {
        unsafe {
            if HK_TOUR_DONE!=0 || IDLE<DWELL {return None;}
            let step=HK_TOUR_STEP as usize;
            if step>=TOUR.len() {HK_TOUR_DONE=1;return None;}
            let (from,to,region)=TOUR[step];
            if from as usize!=scene {HK_TOUR_MISSES+=1;HK_TOUR_DONE=2;return None;}
            Some((to as usize,region as usize))
        }
    }
    pub fn taken() {unsafe {HK_TOUR_STEP+=1;IDLE=0;}}
    pub fn active()->bool {unsafe {HK_TOUR_DONE==0}}
    /// The next listed destination after the current step, for an oracle prefetch.
    pub fn upcoming()->Option<(usize,usize)> {
        let step=unsafe {HK_TOUR_STEP} as usize;
        TOUR.get(step).map(|&(_,to,region)|(to as usize,region as usize))
    }
}
#[cfg(feature="gate-tour")]
pub use imp::{tick,pad,due,taken,active,upcoming};
#[cfg(not(feature="gate-tour"))] #[inline(always)] pub fn tick(_:bool) {}
#[cfg(not(feature="gate-tour"))] #[inline(always)] pub fn pad(bits:u16)->u16 {bits}
#[cfg(not(feature="gate-tour"))] #[inline(always)] pub fn due(_:usize)->Option<(usize,usize)> {None}
#[cfg(not(feature="gate-tour"))] #[inline(always)] pub fn taken() {}
#[cfg(not(feature="gate-tour"))] #[inline(always)] pub fn active()->bool {false}
#[cfg(not(feature="gate-tour"))] #[inline(always)] pub fn upcoming()->Option<(usize,usize)> {None}
