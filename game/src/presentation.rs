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
            unsafe {
                let now=psx_rt::interrupts::vblank_count();
                // The vblanks between this frame's flip and the last one's: the
                // flip follows the queue by one vblank either way.
                HK_FRAME_VBLANKS=now.wrapping_sub(QUEUED_AT);
                QUEUED_AT=now;
            }
            psx_rt::interrupts::queue_gp1_at_vblank(display);
            unsafe {crate::render::HK_FRAME_FLIP_LINES=crate::render::frame_lines();}
        }
    }
}
#[cfg(not(test))]
pub fn finish() {unsafe {STATE.finish(psx_rt::interrupts::gp1_queue_pending());}}
/// VBlanks a queued flip may wait for its GP0(1Fh) before it is written by
/// hand. psx-rt applies a queued word only once the GPU has signalled the end
/// of the frame's drawing (GPUSTAT bit 24); a frame that never does (a lost
/// GP0(1Fh), a wedged GPU) would otherwise wait forever, on a console and in
/// the emulator alike. A frame that heavy still flips, late and counted.
pub const FLIP_TIMEOUT_VBLANKS:u32=30;
#[no_mangle]pub static mut HK_FLIP_TIMEOUTS:u32=0;
#[cfg(not(test))]
static mut QUEUED_AT:u32=0;
/// Vblanks the last presented frame took (flip to flip); the renderer reads it
/// to see which resource a slow frame ran out of (render::begin_frame).
#[cfg(not(test))]
#[no_mangle]pub static mut HK_FRAME_VBLANKS:u32=0;
/// The main loop's queued flip has waited FLIP_TIMEOUT_VBLANKS for its GPU.
#[cfg(not(test))]
pub fn flip_overdue()->bool {
    queued()&&psx_rt::interrupts::gp1_queue_pending()
        &&psx_rt::interrupts::vblank_count().wrapping_sub(unsafe {QUEUED_AT})>=FLIP_TIMEOUT_VBLANKS
}
/// Take the queued display word back from psx-rt and write it now. Racing the
/// VBlank handler is harmless: it writes the same idempotent GP1 word.
#[cfg(not(test))]
pub fn force_flip() {
    let word=psx_rt::interrupts::take_pending_gp1();
    if word!=0 {psx_io::gpu::write_gp1(word);unsafe {HK_FLIP_TIMEOUTS=HK_FLIP_TIMEOUTS.wrapping_add(1);}}
}

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
