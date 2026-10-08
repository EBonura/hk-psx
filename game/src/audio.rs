//! Source one-shots resident in SPU RAM. No CD seek at a gameplay event.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path="volume.rs"]mod volume;
#[path="movement_audio.rs"]mod movement;
static mut LEVEL:u8=10;
/// Every one of the SPU's 24 voices is allocated: 0..5 and16..17 here, 6..11
/// ambience, 12..14 Geo, 18..20 Focus, 21..23 Runner. Voice15 therefore
/// carries the Great Door's hit variant, the False Knight's one-shots, the
/// menu clips and the world bank's five. No scene holds both the door
/// (Tutorial_01) and the fight (Crossroads_10), the menus run with play
/// stopped, and every play reconfigures the voice so none inherits another's
/// sample address, rate or pitch. What overlaps is a kill, a death or a cocoon
/// on top of each other, which retrigger the way every event class here does.
const SHARED_VOICE:u8=15;
const VOICES:[u8;8]=[0,1,2,3,4,5,16,17];
static mut MOVEMENT:movement::State=movement::State::new();
static mut GREAT_DOOR_RANDOM:u32=0x47524452;
#[no_mangle]pub static mut HK_GREAT_DOOR_HIT_SFX:u32=0;
#[no_mangle]pub static mut HK_FOOTSTEP_STARTS:u32=0;
#[no_mangle]pub static mut HK_HARD_LAND_SFX:u32=0;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/sfx.rs"));
/// The world one-shots (host/cook_audio.py WORLD_EVENTS): streamed from the
/// disc before the title into their own SPU range below the Focus bank.
mod world {include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/world-sfx.rs"));}
pub use world::{BANK_BYTES as WORLD_BANK_BYTES,BANK_CHECKSUM as WORLD_BANK_CHECKSUM};
static mut WORLD_READY:bool=false;
static mut WORLD_RANDOM:u32=0x574f524c;
/// Plays per world event: enemy death, hero death, menu confirm, menu start,
/// cocoon break.
#[no_mangle]pub static mut HK_WORLD_SFX:[u32;5]=[0;5];
// Word aligned so the upload takes the SDK's DMA path; include_bytes! alone
// promises byte alignment, and where the linker put it decided which path ran.
#[repr(C,align(4))]struct Aligned([u8;UI_BYTES]);
static UI_CLIPS:Aligned=Aligned(*include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/ui_sfx.adpcm")));
static UI: &[u8] = &UI_CLIPS.0;
#[no_mangle]pub static mut HK_UI_SFX:u32=0;
static mut BANK_READY: bool = false;
#[no_mangle]
pub static mut HK_SFX_COUNT: u32 = 0;
#[no_mangle]
pub static mut HK_SFX_EVENT_COUNTS: [u32; 8] = [0; 8];
/// One per admitted False Knight voice: landing, then mace swing.
#[no_mangle]
pub static mut HK_BOSS_SFX_COUNTS: [u32; 2] = [0; 2];

pub fn ready()->bool {unsafe {BANK_READY}}
pub fn world_ready()->bool {unsafe {WORLD_READY}}
/// The SFX volume step (0..10) every one-shot bank scales its gain by.
pub fn level()->u8 {unsafe {LEVEL}}

/// Owns the SPU reset for every bank in the game, so it runs before the title
/// screen: the retail BIOS leaves a live reverb preset behind and `spu::init`
/// is what silences it. The samples themselves arrive later, from the disc.
pub fn init() {
    assert!(SAMPLES.len()==VOICES.len());
    // set_volume applies one gain to the shared voice, so everything that
    // reaches it has to carry the same source gain.
    assert!(BOSS_SAMPLES[0].2==SAMPLES[0].2 && BOSS_SAMPLES[1].2==SAMPLES[0].2);
    assert!(world::SAMPLES.len()==5 && world::SAMPLES.iter().all(|s|s.2==SAMPLES[0].2 && s.3<=s.4));
    spu::init();
    // The menu clips sit just above where the disc's SFX bank will land; that
    // upload writes only BANK_BYTES, so they survive it.
    assert!(UI.len()==UI_BYTES && UI_SAMPLES[0].0 as usize==0x1010+BANK_BYTES && 0x1010+BANK_BYTES+UI_BYTES<=0x14000);
    spu::upload_adpcm(SpuAddr::new(UI_SAMPLES[0].0), UI);
    unsafe {GREAT_DOOR_RANDOM=0x47524452;HK_GREAT_DOOR_HIT_SFX=0;HK_BOSS_SFX_COUNTS=[0;2];
        WORLD_RANDOM=0x574f524c;HK_WORLD_SFX=[0;5];HK_HERO_EXTRA_SFX=[0;7];HK_ABILITY_SFX=[0;6];
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
    // The shared voice is left alone: every play on it configures it first,
    // and the title's start clip may still be sounding on it while this bank
    // arrives, which a reconfiguration here would re-pitch mid-play.
    unsafe {BANK_READY=true;}
}

/// The world bank, staged by the disc loader like the SFX bank. It only
/// writes SPU RAM; the shared voice is configured per play.
pub fn upload_world(bank:&[u8]) {
    assert!(bank.len()==WORLD_BANK_BYTES);
    assert!(world::SPU_BASE%16==0 && world::SPU_BASE as usize+WORLD_BANK_BYTES<=0x7fff0);
    spu::upload_adpcm(SpuAddr::new(world::SPU_BASE),bank);
    unsafe {WORLD_READY=true;}
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

/// A clip from a scene bank on the shared voice (`scene_sfx::play_shared`).
pub fn shared_clip(address:u32,rate:u32,gain:i16,pitch:u16) {play_shared(address,rate,gain,Some(pitch));}
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
/// `MenuAudioController.PlaySelect` / `PlaySlider`, on the shared voice: the
/// title screen runs before any gameplay sound, and a pause menu stops play.
fn ui_play(index:usize) {
    let (address,rate,gain)=UI_SAMPLES[index];
    play_shared(address,rate,gain,None);
    unsafe {HK_UI_SFX=HK_UI_SFX.saturating_add(1);}
}
/// One world-bank clip on the shared voice, at a pitch drawn from the source's
/// own range when it has one (EnemyDeathEffects' .75..1.25). Silent, and not
/// counted, if the bank never arrived.
fn world_play(index:usize) {
    if !world_ready() {return;}
    let (address,rate,gain,low,high)=world::SAMPLES[index];
    let pitch=if low==high {low} else {
        let random=unsafe {WORLD_RANDOM=WORLD_RANDOM.wrapping_mul(1664525).wrapping_add(1013904223);WORLD_RANDOM>>16};
        (low as u32+random*(high as u32-low as u32+1)/65536) as u16
    };
    play_shared(address,rate,gain,Some(pitch));
    unsafe {HK_WORLD_SFX[index]=HK_WORLD_SFX[index].saturating_add(1);}
}
/// `EnemyDeathEffects.enemyDeathSwordAudio`, once per kill.
pub fn enemy_death() {world_play(0);}
/// The death FSM's `hero_death_v2` layer.
pub fn hero_death() {world_play(1);}
/// `MenuAudioController.PlaySubmit`/`PlayCancel` (the same clip).
pub fn ui_confirm() {world_play(2);}
/// `MenuAudioController.PlayStartGame`.
pub fn ui_start() {world_play(3);}
/// `HealthCocoon.deathSound`.
pub fn cocoon_break() {world_play(4);}
pub fn ui_select() {ui_play(0);}
pub fn ui_slider() {ui_play(1);}
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

/// Plays per extra hero one-shot (host/cook_audio.py HERO_EXTRA order):
/// alternate swing, down swing, dash, wall jump, wings, mantis claw, shade dash.
#[no_mangle]pub static mut HK_HERO_EXTRA_SFX:[u32;7]=[0;7];
/// Plays per ability one-shot riding the Focus bank (host/focus_audio.py
/// ABILITY order): Crystal Heart charge, ready, burst, wall hit, air brake,
/// and Vengeful Spirit's cast.
#[no_mangle]pub static mut HK_ABILITY_SFX:[u32;6]=[0;6];
pub const NAIL_ALT:usize=0;
pub const NAIL_DOWN:usize=1;
pub const DASH:usize=2;
pub const WALLJUMP:usize=3;
pub const WINGS:usize=4;
pub const CLAW:usize=5;
pub const SHADE_DASH:usize=6;
pub const SUPER_CHARGE:usize=0;
pub const SUPER_READY:usize=1;
pub const SUPER_BURST:usize=2;
pub const SUPER_WALL:usize=3;
pub const SUPER_BRAKE:usize=4;
pub const FIREBALL:usize=5;

/// Point `voice` at one clip and key it on. For the voices whose clip changes
/// from play to play (the nail's swings, voice 0's movement one-shots), so the
/// address, rate and gain are written every time.
fn play_on(voice:u8,(address,rate,gain):(u32,u32,i16)) {
    let mask=1<<voice;
    Voice::key_off(mask);
    Voice::new(voice).configure_sample(SpuAddr::new(address),rate,
        Volume(volume::scale(gain,unsafe {LEVEL})),Adsr::sample_one_shot());
    Voice::key_on(mask);
    unsafe {HK_SFX_COUNT=HK_SFX_COUNT.saturating_add(1);}
}

/// Voice 0 is the breakable-wall hit's; the Knight's movement extras share it,
/// so it is pointed back at its own clip on every play.
pub fn door() {
    if !ready() {return;}
    play_on(VOICES[0],SAMPLES[0]);
    unsafe {HK_SFX_EVENT_COUNTS[0]=HK_SFX_EVENT_COUNTS[0].saturating_add(1);}
}
pub fn jump() { play(1); }
pub fn land() { play(2); }
pub fn nail() { nail_kind(0); }
/// One nail swing in `hk_sim::Nail::kind` order (normal, alternate, up, down).
/// The Knight's attack objects carry their own clips: Slash, UpSlash and
/// WallSlash `sword_3`, AltSlash `sword_4`, DownSlash `sword_2`.
pub fn nail_kind(kind:u16) {
    if !ready() {return;}
    let sample=match kind {1=>EXTRA_SAMPLES[NAIL_ALT],3=>EXTRA_SAMPLES[NAIL_DOWN],_=>SAMPLES[3]};
    play_on(VOICES[3],sample);
    unsafe {
        HK_SFX_EVENT_COUNTS[3]=HK_SFX_EVENT_COUNTS[3].saturating_add(1);
        match kind {
            1=>HK_HERO_EXTRA_SFX[NAIL_ALT]=HK_HERO_EXTRA_SFX[NAIL_ALT].saturating_add(1),
            3=>HK_HERO_EXTRA_SFX[NAIL_DOWN]=HK_HERO_EXTRA_SFX[NAIL_DOWN].saturating_add(1),
            _=>{}
        }
    }
}
/// The Knight's movement one-shots (`DASH`..`SHADE_DASH`) on voice 0.
pub fn hero_extra(index:usize) {
    if !ready() {return;}
    play_on(VOICES[0],EXTRA_SAMPLES[index]);
    unsafe {HK_HERO_EXTRA_SFX[index]=HK_HERO_EXTRA_SFX[index].saturating_add(1);}
}
/// Crystal Heart's one-shots on voice 0, Vengeful Spirit's cast on the shared
/// voice. The clip rides the Focus bank (`focus_audio::ABILITY_SAMPLES`), so
/// the caller passes it and only calls once that bank is uploaded.
pub fn ability(index:usize,sample:(u32,u32,i16)) {
    let voice=if index==FIREBALL {SHARED_VOICE} else {VOICES[0]};
    play_on(voice,sample);
    unsafe {HK_ABILITY_SFX[index]=HK_ABILITY_SFX[index].saturating_add(1);}
}
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
