//! Source one-shots resident in SPU RAM. No CD seek at a gameplay event.
use psx_spu::{self as spu, Adsr, SpuAddr, Voice, Volume};
#[path="volume.rs"]mod volume;
static mut LEVEL:u8=10;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/sfx.rs"));
// Word alignment permits the SDK's SPU DMA upload path. No staging allocation.
#[repr(align(4))]
struct Bank([u8; BANK_BYTES]);
static BANK: Bank = Bank(*include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/sfx.adpcm")));
#[no_mangle]
pub static mut HK_SFX_COUNT: u32 = 0;
#[no_mangle]
pub static mut HK_SFX_EVENT_COUNTS: [u32; 6] = [0; 6];

pub fn init() {
    assert!(BANK_BYTES <= 32768);
    spu::init();
    spu::upload_adpcm(SpuAddr::new(0x1010), &BANK.0);
    for (index, &(address, rate, volume)) in SAMPLES.iter().enumerate() {
        // Pinned SDK fast release + reserved silence repeat address avoid
        // percussive sustain truncation and sample()'s indefinite release.
        Voice::new(index as u8).configure_sample(
            SpuAddr::new(address), rate, Volume(volume::scale(volume,unsafe {LEVEL})), Adsr::sample_one_shot(),
        );
    }
}

/// Apply to already playing voices too, including exact mute.
pub fn set_volume(level:u8) {
    unsafe {LEVEL=level.min(10);}
    for (index,&(_,_,source)) in SAMPLES.iter().enumerate() {
        let gain=Volume(volume::scale(source,level));
        Voice::new(index as u8).set_volume(gain,gain);
    }
}

fn play(index: usize) {
    // One dedicated voice per event class. Retrigger replaces only that class;
    // jump/land, nail impacts and hurt can overlap without an unbounded pool.
    let mask = 1 << index;
    Voice::key_off(mask);
    Voice::key_on(mask);
    unsafe {
        HK_SFX_COUNT = HK_SFX_COUNT.saturating_add(1);
        HK_SFX_EVENT_COUNTS[index] = HK_SFX_EVENT_COUNTS[index].saturating_add(1);
    }
}

pub fn door() { play(0); }
pub fn jump() { play(1); }
pub fn land() { play(2); }
pub fn nail() { play(3); }
pub fn hurt() { play(4); }
pub fn enemy_hit() { play(5); }
