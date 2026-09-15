//! Main-thread presentation progress at cooperative simulation checkpoints.
//! Only the VBlank IRQ consumes the SDK's GP1 queue; it never accesses this
//! state. Arm after the final OT is complete, retire before any packet reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Idle, FinalDma, Drain, Queued }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action { None, KickFinal, Queue(u32) }
struct State { display:u32, phase:Phase }
const _:()=assert!(core::mem::size_of::<State>()<=8);
impl State {
    const fn new()->Self {Self {display:0,phase:Phase::Idle}}
    fn begin(&mut self,display:u32) {
        assert!(self.phase==Phase::Idle,"presentation still owns packets");
        self.display=display;self.phase=Phase::FinalDma;
    }
    #[inline(always)]
    fn needs_service(&self)->bool {matches!(self.phase,Phase::FinalDma|Phase::Drain)}
    fn advance(&mut self,dma_busy:bool)->Action {
        if dma_busy {return Action::None;}
        match self.phase {
            Phase::FinalDma=>{self.phase=Phase::Drain;Action::KickFinal}
            Phase::Drain=>{self.phase=Phase::Queued;Action::Queue(self.display)}
            Phase::Idle|Phase::Queued=>Action::None,
        }
    }
    fn finish(&mut self,queue_pending:bool) {
        assert!(self.phase==Phase::Queued&&!queue_pending,"presentation incomplete");
        self.phase=Phase::Idle;
    }
}
#[cfg(not(test))]
static mut STATE:State=State::new();
#[cfg(not(test))]
pub fn begin(display:u32) {unsafe {STATE.begin(display);}}
#[cfg(not(test))]
#[inline(always)]
pub fn queued()->bool {unsafe {STATE.phase==Phase::Queued}}
#[cfg(not(test))]
#[inline(always)]
pub fn checkpoint() {
    if unsafe {STATE.needs_service()} {service();}
}
#[cfg(not(test))]
#[inline(never)]
fn service() {
    // Do not queue on GPU readiness alone: an earlier linked-list DMA can
    // still own packet memory even when the GPU's FIFO reports ready.
    if crate::render::dma_pending() {return;}
    // End the state borrow before any hardware call. These calls do not invoke
    // cooperative checkpoints, and IRQ handlers never mutate STATE.
    let action=unsafe {STATE.advance(false)};
    match action {
        Action::None=>{},
        Action::KickFinal=>crate::render::kick_front(),
        Action::Queue(display)=>{
            psx_rt::interrupts::queue_gp1_at_vblank(display);
            unsafe {crate::render::HK_FRAME_FLIP_LINES=crate::render::frame_lines();}
        }
    }
}
#[cfg(not(test))]
pub fn finish() {unsafe {STATE.finish(psx_rt::interrupts::gp1_queue_pending());}}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_busy_schedule_preserves_final_dma_then_queue() {
        for schedule in 0..65536u32 {
            let mut state=State::new();
            assert!(!state.needs_service());
            assert_eq!(state.advance(false),Action::None);
            let display=0x05000000|schedule;
            state.begin(display);
            let mut actions=std::vec::Vec::new();
            for i in 0..18 {
                let busy=i<16&&schedule&(1<<i)!=0;
                let before=state.phase;
                let action=state.advance(busy);
                if busy {assert_eq!(action,Action::None);assert_eq!(state.phase,before);}
                if action!=Action::None {actions.push(action);}
            }
            assert_eq!(actions,[Action::KickFinal,Action::Queue(display)]);
            assert!(!state.needs_service());
            assert_eq!(state.advance(false),Action::None);
            state.finish(false);
            assert!(!state.needs_service());
            state.begin(display^1);
            assert_eq!(state.advance(false),Action::KickFinal);
            assert_eq!(state.advance(false),Action::Queue(display^1));
        }
    }
    #[test]
    #[should_panic(expected="presentation still owns packets")]
    fn cannot_rearm_while_final_dma_is_pending() {let mut s=State::new();s.begin(1);s.begin(2);}
    #[test]
    #[should_panic(expected="presentation incomplete")]
    fn cannot_retire_before_final_dma_drains() {let mut s=State::new();s.begin(1);s.advance(false);s.finish(false);}
    #[test]
    #[should_panic(expected="presentation incomplete")]
    fn cannot_retire_before_vblank_consumes_queue() {let mut s=State::new();s.begin(1);s.advance(false);s.advance(false);s.finish(true);}
}
