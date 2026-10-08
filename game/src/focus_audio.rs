//! Full source Focus clips loaded once into SPU through startup scratch.
//! No event-time CD reads; two heal voices retain tails across Focus release.
use psx_spu::{self as spu,Adsr,SpuAddr,Voice,Volume};
#[path="focus_audio_state.rs"]mod state;
#[path="volume.rs"]mod volume;
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/focus-audio.rs"));
const CHARGE:u8=18;
const HEALS:[u8;2]=[19,20];
static mut STATE:state::State=state::State::new();
static mut LEVEL:u8=10;
static mut NEXT_HEAL:usize=0;
#[no_mangle]pub static mut HK_FOCUS_AUDIO_READY:u32=0;
#[no_mangle]pub static mut HK_FOCUS_CHARGE_STARTS:u32=0;
#[no_mangle]pub static mut HK_FOCUS_HEAL_SOUNDS:u32=0;
#[no_mangle]pub static mut HK_FOCUS_CHARGE_GAIN:u32=0;
pub fn ready()->bool {unsafe {HK_FOCUS_AUDIO_READY!=0}}
pub fn upload(bytes:&[u8])->bool {
    if bytes.len()!=BANK_BYTES || BANK_BYTES%16!=0 || CHARGE_BYTES%16!=0
        || CHARGE_BYTES==0 || CHARGE_BYTES+16>=BANK_BYTES || SPU_BASE%16!=0
        || SPU_BASE+BANK_BYTES as u32>0x80000 {return false;}
    let end=crate::ambience::CLIPS.iter().map(|c|c.spu_address+c.spu_bytes as u32).max().unwrap_or(0);
    if SPU_BASE<end {return false;}
    let mut checksum=0x811c9dc5u32;
    for &b in bytes {checksum=(checksum^u32::from(b)).wrapping_mul(0x01000193);}
    if checksum!=BANK_CHECKSUM {return false;}
    if FOCUS_BYTES>BANK_BYTES || FOCUS_BYTES%16!=0 || CHARGE_BYTES+16>=FOCUS_BYTES {return false;}
    for (i,b) in bytes.chunks_exact(16).enumerate() {
        let offset=i*16;
        if b[0]>>4>4 || b[0]&15>12 {return false;}
        if offset>=FOCUS_BYTES {
            // The ability one-shots: no loop flags, each ends on a silent
            // END block, as every one-shot bank's clips do.
            if b[1]>1 || (b[1]==1&&b[2..].iter().any(|&n|n!=0)) {return false;}
            continue;
        }
        let flags=if offset==0 {4}else if offset+16==CHARGE_BYTES {3}
            else if offset+16==FOCUS_BYTES {1}else{0};
        if b[1]!=flags {return false;}
        if (offset==0 || offset==CHARGE_BYTES)&&b[0]>>4!=0 {return false;}
        if offset+16==FOCUS_BYTES&&b[2..].iter().any(|&n|n!=0) {return false;}
    }
    spu::upload_adpcm(SpuAddr::new(SPU_BASE),bytes);
    Voice::new(CHARGE).configure_sample(SpuAddr::new(SPU_BASE),CHARGE_RATE,Volume::SILENCE,Adsr::sample_one_shot());
    Voice::new(CHARGE).set_loop_addr(SpuAddr::new(SPU_BASE));
    for voice in HEALS {
        Voice::new(voice).configure_sample(SpuAddr::new(SPU_BASE+CHARGE_BYTES as u32),HEAL_RATE,
            Volume(volume::scale(GAIN,unsafe {LEVEL})),Adsr::sample_one_shot());
    }
    unsafe {HK_FOCUS_AUDIO_READY=1;}
    true
}
fn charge_volume(gain:u8) {
    let v=Volume((i32::from(volume::scale(GAIN,unsafe {LEVEL}))*i32::from(gain)/128)as i16);
    Voice::new(CHARGE).set_volume(v,v);
    unsafe {HK_FOCUS_CHARGE_GAIN=gain as u32;}
}
pub fn set_volume(level:u8) {
    unsafe {LEVEL=level.min(10);}
    if !ready() {return;}
    charge_volume(unsafe {STATE.gain()});
    let gain=Volume(volume::scale(GAIN,level));
    for voice in HEALS {Voice::new(voice).set_volume(gain,gain);}
}
pub fn tick(focus:&hk_sim::Focus,events:hk_sim::FocusEvents) {
    if !ready() {return;}
    let phase=match focus.animation() {
        None=>state::Phase::Off,
        Some((hk_sim::FocusClip::End|hk_sim::FocusClip::GetOnce,age))=>state::Phase::Fade(age),
        Some(_)=>state::Phase::Charge,
    };
    let e=unsafe {STATE.step(phase,events.started,events.completed,FADE_TICKS)};
    if e.stop {Voice::key_off(1<<CHARGE);}
    if e.volume_changed||e.start {charge_volume(e.gain);}
    if e.start {
        Voice::key_off(1<<CHARGE);Voice::key_on(1<<CHARGE);
        Voice::new(CHARGE).set_loop_addr(SpuAddr::new(SPU_BASE));
        unsafe {HK_FOCUS_CHARGE_STARTS=HK_FOCUS_CHARGE_STARTS.saturating_add(1);}
    }
    if e.heal {
        let voice=unsafe {let v=HEALS[NEXT_HEAL];NEXT_HEAL=(NEXT_HEAL+1)%HEALS.len();v};
        Voice::key_off(1<<voice);Voice::key_on(1<<voice);
        unsafe {HK_FOCUS_HEAL_SOUNDS=HK_FOCUS_HEAL_SOUNDS.saturating_add(1);}
    }
}
/// Damage/scene cancellation stops charging immediately, preserving heal tails.
pub fn interrupt() {
    unsafe {STATE=state::State::new();}
    if ready() {charge_volume(0);Voice::key_off(1<<CHARGE);}
}
