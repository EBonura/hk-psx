//! Source one-shots resident in SPU RAM. No CD seek at a gameplay event.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path="volume.rs"]mod volume;
#[path="movement_audio.rs"]mod movement;
static mut LEVEL:u8=10;
/// Every one of the SPU's 24 voices is allocated: 0..5 and16..17 here, 6..11
/// ambience, 12..14 Geo, 18..20 Focus, 21..23 Runner. Voice15 therefore
/// carries both the Great Door's hit variant and the False Knight's one-shots.
/// No scene holds both, the door being Tutorial_01 and the fight Crossroads_10,
/// and every play reconfigures the voice so neither inherits the other's
/// sample address or rate.
const SHARED_VOICE:u8=15;
const VOICES:[u8;8]=[0,1,2,3,4,5,16,17];
static mut MOVEMENT:movement::State=movement::State::new();
static mut GREAT_DOOR_RANDOM:u32=0x47524452;
#[no_mangle]pub static mut HK_GREAT_DOOR_HIT_SFX:u32=0;
#[no_mangle]pub static mut HK_FOOTSTEP_STARTS:u32=0;
#[no_mangle]pub static mut HK_HARD_LAND_SFX:u32=0;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/sfx.rs"));
static mut BANK_READY: bool = false;
#[no_mangle]
pub static mut HK_SFX_COUNT: u32 = 0;
#[no_mangle]
pub static mut HK_SFX_EVENT_COUNTS: [u32; 8] = [0; 8];
/// One per admitted False Knight voice: landing, then mace swing.
#[no_mangle]
pub static mut HK_BOSS_SFX_COUNTS: [u32; 2] = [0; 2];

pub fn ready()->bool {unsafe {BANK_READY}}

/// Owns the SPU reset for every bank in the game, so it runs before the title
/// screen: the retail BIOS leaves a live reverb preset behind and `spu::init`
/// is what silences it. The samples themselves arrive later, from the disc.
pub fn init() {
    assert!(SAMPLES.len()==VOICES.len());
    // set_volume applies one gain to the shared voice, so everything that
    // reaches it has to carry the same source gain.
    assert!(BOSS_SAMPLES[0].2==SAMPLES[0].2 && BOSS_SAMPLES[1].2==SAMPLES[0].2);
    spu::init();
    unsafe {GREAT_DOOR_RANDOM=0x47524452;HK_GREAT_DOOR_HIT_SFX=0;HK_BOSS_SFX_COUNTS=[0;2];
        HK_FOOTSTEP_STARTS=0;HK_HARD_LAND_SFX=0;MOVEMENT=movement::State::new();}
}

/// Bootstrap bank upload. `bank` is the disc chunk staged in the scene arena,
/// checked for length and checksum by the loader; the SPU owns the samples
/// from here and nothing stays resident in main RAM. No voice is configured
/// until its sample is actually in SPU RAM, so a failed load leaves silence
/// rather than voices pointed at uninitialised memory.
pub fn upload(bank:&[u8]) {
    assert!(bank.len()==BANK_BYTES);
    assert!(BANK_BYTES <= 0x14000-0x1010);
    spu::upload_adpcm(SpuAddr::new(0x1010), bank);
    for (index, &(address, rate, volume)) in SAMPLES.iter().enumerate() {
        // Pinned SDK fast release + reserved silence repeat address avoid
        // percussive sustain truncation and sample()'s indefinite release.
        Voice::new(VOICES[index]).configure_sample(
            SpuAddr::new(address), rate, Volume(volume::scale(volume,unsafe {LEVEL})), Adsr::sample_one_shot(),
        );
    }
    // The Great Door requests this same clip as one of its two hit variants.
    // A separate voice avoids replacing ordinary breakable/Geo playback.
    let (address,rate,gain)=SAMPLES[0];
    Voice::new(SHARED_VOICE).configure_sample(SpuAddr::new(address),rate,
        Volume(volume::scale(gain,unsafe {LEVEL})),Adsr::sample_one_shot());
    unsafe {BANK_READY=true;}
}

/// Apply to already playing voices too, including exact mute.
pub fn set_volume(level:u8) {
    unsafe {LEVEL=level.min(10);}
    for (index,&(_,_,source)) in SAMPLES.iter().enumerate() {
        let gain=Volume(volume::scale(source,level));
        Voice::new(VOICES[index]).set_volume(gain,gain);
    }
    let gain=Volume(volume::scale(SAMPLES[0].2,level));
    Voice::new(SHARED_VOICE).set_volume(gain,gain);
}

/// Retrigger the shared voice on one clip. Two owners take turns on it, so the
/// address and rate are written every time rather than left from `init`.
fn play_shared(address:u32,rate:u32,gain:i16,pitch:Option<u16>) {
    Voice::key_off(1<<SHARED_VOICE);
    Voice::new(SHARED_VOICE).configure_sample(SpuAddr::new(address),rate,
        Volume(volume::scale(gain,unsafe {LEVEL})),Adsr::sample_one_shot());
    if let Some(pitch)=pitch {Voice::new(SHARED_VOICE).set_pitch(Pitch::raw(pitch));}
    Voice::key_on(1<<SHARED_VOICE);
    unsafe {HK_SFX_COUNT=HK_SFX_COUNT.saturating_add(1);}
}

/// Resident first authored variant, with the source's .85..1.15 pitch range.
/// Alternate hit/death clips remain absent; this does not substitute the hit
/// sample for the distinct destruction sound at stages4,8 and13.
pub fn great_door_hit() {
    let random=unsafe {
        GREAT_DOOR_RANDOM=GREAT_DOOR_RANDOM.wrapping_mul(1664525).wrapping_add(1013904223);
        GREAT_DOOR_RANDOM>>16
    };
    let [low,high]=GREAT_DOOR_HIT_PITCH;
    let pitch=low as u32+random*(high as u32-low as u32+1)/65536;
    let (address,rate,gain)=SAMPLES[0];
    play_shared(address,rate,gain,Some(pitch as u16));
    unsafe {HK_GREAT_DOOR_HIT_SFX=HK_GREAT_DOOR_HIT_SFX.saturating_add(1);}
}

/// `FalseyControl`'s own one-shots, both `AudioPlaySimple` at source gain and
/// pitch. `boss_land` is `S Land`, `State 2` and `Land Noise`, the armoured
/// body hitting the floor; `boss_swing` is `S Attack` and `JA Hit 2`, the mace
/// coming down. `Slam`'s own `false_knight_strike_ground` is not resident:
/// host/cook_audio.py's refusal table carries its measured size, and what the
/// slam has instead is `camera::request(Shake::Big)` on the same state.
fn boss_play(index:usize) {
    let (address,rate,gain)=BOSS_SAMPLES[index];
    play_shared(address,rate,gain,None);
    unsafe {HK_BOSS_SFX_COUNTS[index]=HK_BOSS_SFX_COUNTS[index].saturating_add(1);}
}
pub fn boss_land() {boss_play(0);}
pub fn boss_swing() {boss_play(1);}

fn play(index: usize) {
    // One dedicated voice per event class. Retrigger replaces only that class;
    // jump/land, nail impacts and hurt can overlap without an unbounded pool.
    let mask = 1 << VOICES[index];
    Voice::key_off(mask);
    Voice::key_on(mask);
    unsafe {
        HK_SFX_COUNT = HK_SFX_COUNT.saturating_add(1);
        HK_SFX_EVENT_COUNTS[index] = HK_SFX_EVENT_COUNTS[index].saturating_add(1);
        if index==6 {HK_HARD_LAND_SFX=HK_HARD_LAND_SFX.saturating_add(1);}
        if index==7 {HK_FOOTSTEP_STARTS=HK_FOOTSTEP_STARTS.saturating_add(1);}
    }
}

pub fn door() { play(0); }
pub fn jump() { play(1); }
pub fn land() { play(2); }
pub fn nail() { play(3); }
pub fn hurt() { play(4); }
pub fn enemy_hit() { play(5); }

/// Call once after each consumed60Hz player physics step, replacing the old
/// standalone soft-land call. `running` describes the supported RUNNING state,
/// excluding pause/dialogue/recoil; `falling` is post-step vy<0. Normal dry
/// terrain allows hard landing; NoHardLanding/acid surfaces must pass false.
pub fn movement_tick(was_grounded:bool,grounded:bool,falling:bool,running:bool,hard_allowed:bool) {
    let events=unsafe {MOVEMENT.tick(was_grounded,grounded,falling,running,hard_allowed,
        HARD_FALL_MIN_TICKS,SOFT_LANDING_TICKS,RUN_SEQUENCE_TICKS)};
    if events.stop_run {Voice::key_off(1<<VOICES[7]);}
    match events.landing {
        movement::Landing::None=>{},
        movement::Landing::Soft=>play(2),
        movement::Landing::Hard=>play(6),
    }
    if events.start_run {play(7);}
}

/// Pause/dialogue entry must stop the current sequence even when simulation
/// ticks are suspended. Preserve falling duration across that suspension.
pub fn stop_footsteps() {
    Voice::key_off(1<<VOICES[7]);
    unsafe {MOVEMENT.stop_run();}
}

/// Scene/respawn/Select reset: discard the previous location's fall duration.
pub fn reset_movement() {
    stop_footsteps();
    unsafe {MOVEMENT=movement::State::new();}
}
