//! Per-scene one-shot banks (host/scene_sfx.py), read with the scene at its gate.
//!
//! The resident banks hold what every scene can play. A sound only some scenes
//! need lives in its scene's bank: one pack chunk first in that scene's disc
//! group, so the gate load reads it on the way to the scene with no extra seek,
//! and `disc::admit_scenes` hands it here after the scene's ambience stems are
//! in. Each clip goes to the SPU address the cook placed it at, in bytes no stem
//! that can sound in this scene uses; a resident stem the bank overwrites is
//! forgotten (`ambience::forget`) so it reloads when a cue wants it again.
//!
//! Playback: `play(EVENT)` with the event constants below, on a voice of the shared
//! pool (`audio::pool_play`) at the event's weight (`weight`). An event the current scene's
//! bank does not hold (not cooked for this scene, or refused for room) is a
//! no-op, counted in `HK_SCENE_SFX_MISSED`, so a call site never has to know
//! which scenes carry which sounds.
use crate::audio::{self, Claim, Weight};
use psx_fx::NrLcg;
use psx_spu::{self as spu, SpuAddr};
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scene_sfx.rs"));
// The pool's first voice is the one ambience leaves for the room sounds, and no pool voice is
// one ambience or the music keys.
const _: () = {
    let pool = audio::POOL_VOICES;
    assert!(pool[0] == crate::ambience::SCENE_SFX_VOICE);
    let mut i = 0;
    while i < pool.len() {
        assert!(pool[i] != crate::ambience::MUSIC_VOICE);
        let mut j = 0;
        while j < crate::ambience::AMBIENCE_POOL_VOICES.len() {
            assert!(pool[i] != crate::ambience::AMBIENCE_POOL_VOICES[j]);
            j += 1;
        }
        i += 1;
    }
};
const NONE: usize = usize::MAX;
/// Bytes of a clip sent to the SPU between pad checkpoints. The SPU port stalls the CPU on
/// every write, so a slice takes a good fraction of a frame; 1 KiB keeps a late slice
/// from carrying a poll over a VBlank (4 KiB did, in the first load of the False Knight
/// journey).
const UPLOAD_SLICE: usize = 1024;
static mut CURRENT: usize = NONE;
/// The looping event `loop_tick` keyed and the voice it holds; when a heavier sound takes the
/// voice, the next `loop_tick` starts the loop again on another.
static mut LOOPING: Option<(u8, Claim)> = None;
/// The voices this bank's sounds were last handed, one per pool voice: a new bank stops those
/// still sounding, since their clips' bytes are about to be overwritten.
static mut PLAYING: [Option<Claim>; audio::POOL_VOICES.len()] = [None; audio::POOL_VOICES.len()];
/// Plays per event, and plays asked for that the resident bank does not hold.
#[no_mangle]
pub static mut HK_SCENE_SFX: [u32; EVENTS] = [0; EVENTS];
#[no_mangle]
pub static mut HK_SCENE_SFX_MISSED: u32 = 0;
/// Banks uploaded, bytes uploaded, and the scene id (plus one) now resident.
#[no_mangle]
pub static mut HK_SCENE_SFX_LOADS: u32 = 0;
#[no_mangle]
pub static mut HK_SCENE_SFX_BYTES: u32 = 0;
#[no_mangle]
pub static mut HK_SCENE_SFX_SCENE: u32 = 0;

/// An ENTRIES row as cooked: (event, SPU address / 8, chunk offset / 16, bytes / 16, rate, pitch register).
type Packed = (u8, u16, u16, u16, u16, u16);
fn entries(scene: usize) -> &'static [Packed] {
    let (_, _, first, count) = BANKS[scene];
    &ENTRIES[first as usize..first as usize + count as usize]
}
/// A row widened: (event, SPU address, chunk offset, bytes, rate, pitch register).
fn unpack(e: &Packed) -> (u8, u32, u32, u32, u32, u16) {
    (e.0, u32::from(e.1) * 8, u32::from(e.2) * 16, u32::from(e.3) * 16, u32::from(e.4), e.5)
}
/// The chunk `scene`'s group carries: its length and FNV-1a.
pub const fn chunk(scene: usize) -> (usize, u32) {
    let (len, fnv, _, _) = BANKS[scene];
    (len as usize, fnv)
}
/// The resident bank is gone (a new admission began, or something else wrote
/// its bytes). Nothing plays from it until the next `upload`.
pub fn invalidate() {
    unsafe {
        CURRENT = NONE;
        SWITCH_CLOCK = SWITCH_IDLE;
        LOOPING = None;
        HK_SCENE_SFX_SCENE = 0;
    }
}
/// Another bank is about to write `lo..hi`: drop the resident one if it
/// overlaps, so a play never keys on bytes that are no longer its clip.
pub fn overwritten(lo: u32, hi: u32) {
    let current = unsafe { CURRENT };
    if current != NONE
        && entries(current)
            .iter()
            .map(unpack)
            .any(|(_, a, _, n, _, _)| a < hi && lo < a + n)
    {
        invalidate();
    }
}
/// `bytes` is the checked chunk for guest scene `scene`. Uploads every entry to
/// its cooked address, a slice at a time with the pad serviced between.
pub fn upload(scene: usize, bytes: &[u8]) {
    stop_all();
    invalidate();
    assert!(bytes.len() == BANKS[scene].0 as usize);
    let mut total = 0u32;
    for (event, address, offset, len, _, _) in entries(scene).iter().map(unpack) {
        assert!(address % 16 == 0 && offset % 16 == 0);
        crate::ambience::forget(address, address + len);
        let clip = &bytes[offset as usize..(offset + len) as usize];
        for (slice, part) in clip.chunks(UPLOAD_SLICE).enumerate() {
            // A timed-out upload is latched by psx_spu (first_upload_fault); there is no per-game retry.
            let _ = spu::upload_adpcm(SpuAddr::new(address + (slice * UPLOAD_SLICE) as u32), part);
            crate::input::checkpoint();
        }
        if event == MINER_IDLE_HUM || event == ZOMBIE_GUARD_RUN_LOOP {
            make_loop(address, clip);
        }
        total += len;
    }
    unsafe {
        CURRENT = scene;
        HK_SCENE_SFX_SCENE = scene as u32 + 1;
        HK_SCENE_SFX_LOADS = HK_SCENE_SFX_LOADS.saturating_add(1);
        HK_SCENE_SFX_BYTES = HK_SCENE_SFX_BYTES.saturating_add(total);
    }
}
/// Turn an uploaded one-shot into a looping sample. The cook ends every clip with a silent
/// END block after its data; the first data block gets the loop-start flag and the last one
/// loop-end with repeat, so the SPU plays the data round and round until key off.
fn make_loop(address: u32, clip: &[u8]) {
    let last = clip.len() - 32;
    for (at, flags) in [(0, 4u8), (last, 3u8)] {
        let mut block = [0u8; 16];
        block.copy_from_slice(&clip[at..at + 16]);
        block[1] = flags;
        // A timed-out upload is latched by psx_spu (first_upload_fault); there is no per-game retry.
        let _ = spu::upload_adpcm(SpuAddr::new(address + at as u32), &block);
    }
}
/// Stop every sound of this bank still sounding on a pool voice, the loop too.
fn stop_all() {
    unsafe {
        for claim in PLAYING.iter_mut() {
            if let Some(c) = claim.take() {
                audio::pool_stop(c);
            }
        }
        if let Some((_, c)) = LOOPING.take() {
            audio::pool_stop(c);
        }
    }
}
/// Keep `claim` as the sound now on its pool voice (`stop_all`).
fn remember(claim: Claim) {
    if let Some(slot) = audio::POOL_VOICES.iter().position(|&v| v == claim.voice) {
        unsafe {
            PLAYING[slot] = Some(claim);
        }
    }
}
/// How much `event` matters when every pool voice is sounding: a boss's or an arena's sounds
/// outweigh an enemy's, an enemy's the world's, and an enemy's footsteps go first, so a rustle
/// or a step never cuts a roar.
const fn weight(event: u8) -> Weight {
    match event {
        ZOMBIE_GUARD_FOOTSTEP => Weight::Ambient,
        FALSE_KNIGHT_STRIKE_GROUND
        | FALSE_KNIGHT_RAGE
        | BOSS_FINAL_HIT
        | FALSE_KNIGHT_JUMP
        | FALSE_KNIGHT_LAND_1ST_TIME
        | FALSE_KNIGHT_DAMAGE_ARMOUR_FINAL
        | FALSE_KNIGHT_ROLL
        | FALSE_KNIGHT_DEATH
        | FALSE_KNIGHT_CEILING_BREAK
        | GATE_SLAM
        | GATE_OPEN
        | MAWLEK_JUMP
        | MAWLEK_SPIT
        | MAWLEK_WHIP
        | MAWLEK_CALL
        | MAWLEK_BIG_SPIT
        | MAWLEK_SCREAM
        | BOSS_EXPLODE
        | BOSS_GUSHING
        | MAWLEK_JUMP_OFFSCREEN
        | MAWLEK_SPIT_B
        | BIG_FLY_WALL_HIT
        | BIG_FLY_SNORE_STARTLE
        | GRUZ_FINAL_HIT
        | GRUZ_EXPLODE
        | GRUZ_GUSHING
        | BIG_FLY_STOMACHE_PROBLEMS_1
        | BIG_FLY_STOMACHE_PROBLEMS_2
        | BIG_FLY_STOMACHE_PROBLEMS_FINAL_AND_EXPLODE => Weight::Boss,
        ZOMBIE_SHIELD_RAISE
        | ZOMBIE_SHIELD_MOVE
        | ZOMBIE_GUARD_CLUB
        | HERO_PARRY
        | ASPID_SPIT
        | ZOMBIE_GUARD_AWAKE
        | ZOMBIE_GUARD_CLUB_PREPARE
        | ZOMBIE_GUARD_CLUB_PREPARE_2
        | ZOMBIE_GUARD_CLUB_PREPARE_3
        | ZOMBIE_GUARD_DEATH
        | ZOMBIE_GUARD_RUN_LOOP
        | SPLAT_1
        | SPLAT_2
        | SPLAT_3 => Weight::Enemy,
        GRASS_CUT_4
        | GRASS_CUT_2
        | GRASS_CUT_3
        | GRASS_CUT_5
        | WATER_DRIP_1
        | WATER_DRIP_2
        | WATER_DRIP_3
        | WATER_DRIP_4 => Weight::Ambient,
        _ => Weight::World,
    }
}
/// `GrassBehaviour.cutSounds`, `Corpse.splatAudioClipTable` and
/// `WaterDrip.impactAudioClipTable`, in the source's order. A table plays one of the clips the
/// scene's bank holds, at random with equal weights (`PlayRandomSound`, `RandomAudioClipTable`).
const GRASS_CUT: [u8; 4] = [GRASS_CUT_4, GRASS_CUT_2, GRASS_CUT_3, GRASS_CUT_5];
const SPLAT: [u8; 3] = [SPLAT_1, SPLAT_2, SPLAT_3];
const WATER_DRIP: [u8; 4] = [WATER_DRIP_1, WATER_DRIP_2, WATER_DRIP_3, WATER_DRIP_4];
/// The tables' own generator: audio never moves gameplay's random state.
static mut TABLE_RANDOM: u32 = 0x5446_424c;
/// One clip of `table` the resident bank holds, at random, at its gain scaled by what `rolloff`
/// leaves of a source at `at` (Q16 world) from the camera. Silent beyond the rolloff, and when the
/// bank holds none of them (the scene had no room: refused at cook time).
fn play_one_of(table: &[u8], at: [i32; 2], rolloff: hk_sim::fixed::Rolloff) {
    let current = unsafe { CURRENT };
    if current == NONE {
        return;
    }
    let (dx, dy) = unsafe { (at[0] - crate::camera::HK_CAMERA_X, at[1] - crate::camera::HK_CAMERA_Y) };
    let level = hk_sim::fixed::rolloff_at(dx, dy, rolloff);
    if level == 0 {
        return;
    }
    let held = |e: &&u8| entries(current).iter().any(|x| x.0 == **e);
    let count = table.iter().filter(held).count() as u32;
    if count == 0 {
        unsafe {
            HK_SCENE_SFX_MISSED = HK_SCENE_SFX_MISSED.saturating_add(1);
        }
        return;
    }
    let pick = unsafe {
        TABLE_RANDOM = NrLcg::step(TABLE_RANDOM);
        psx_math::WideU::mul(TABLE_RANDOM, count).high() as usize
    };
    let Some(&event) = table.iter().filter(held).nth(pick) else {
        return;
    };
    if let Some((address, rate, gain, pitch, bytes)) = lookup(event) {
        let gain = (i32::from(gain) * level / 4096) as i16;
        if let Some(claim) = audio::pool_play(weight(event), (address, rate, gain), Some(pitch), bytes) {
            remember(claim);
        }
    }
}
/// A grass patch at `at` was cut (`GrassBehaviour.CutReact`).
pub fn grass_cut(at: [i32; 2]) {
    play_one_of(&GRASS_CUT, at, hk_sim::fixed::GRASS_ROLLOFF);
}
/// A breaker corpse at `at` smashed on landing (`Corpse.Smash`).
pub fn corpse_smash(at: [i32; 2]) {
    play_one_of(&SPLAT, at, hk_sim::fixed::ACTOR_ROLLOFF);
}
/// A water drip at `at` hit the ground (`WaterDrip`'s impact).
pub fn drip(at: [i32; 2]) {
    play_one_of(&WATER_DRIP, at, hk_sim::fixed::ACTOR_ROLLOFF);
}
/// The nail struck terrain (`HeroController.CheckForTerrainThunk`): the impact effect's
/// AudioSource is 2D (no rolloff) and `AudioSourcePitchRandomizer` picks its pitch in 0.75 to 1.25.
pub fn nail_terrain() {
    play_random_pitch(NAIL_TERRAIN, 3072, 2049);
}
/// `event` at a pitch scale drawn from `low` to `low + span - 1` (/ 4096): the terrain tink's and
/// the TinkEffect's pitch randomisers share one body.
#[inline(never)]
fn play_random_pitch(event: u8, low: u32, span: u32) {
    let scale = unsafe {
        TABLE_RANDOM = NrLcg::step(TABLE_RANDOM);
        low + psx_math::WideU::mul(TABLE_RANDOM, span).high()
    };
    play_pitched(event, scale);
}
/// A gate switch at `at` was struck (`Switch Control`'s `Hit`: AudioPlayerOneShotSingle through
/// the audio actor at the switch). The gate it opens plays its own sounds on its way open
/// (`SWITCH_SOUNDS`), timed from now.
pub fn gate_switch(at: [i32; 2]) {
    play_one_of(&[SWITCH_GATE_SWITCH], at, hk_sim::fixed::ACTOR_ROLLOFF);
    unsafe {
        SWITCH_CLOCK = 0;
    }
}
/// A vine platform's `Vine` at `at` was cut (`VinePlatformCut.Cut`: PlayOneShot of `cutSound` on
/// the platform's AudioSource, whose rolloff is the actor curve).
pub fn vine_cut(at: [i32; 2]) {
    play_one_of(&[VINE_PLAT_CUT], at, hk_sim::fixed::ACTOR_ROLLOFF);
}
/// Ticks left before a TinkEffect answers again (`repeatDelay`, 0.25 s), one for the port's
/// TinkEffect surfaces together rather than one per object.
static mut TINK_WAIT: u8 = 0;
/// Ticks since the last gate switch was struck, while a `SWITCH_SOUNDS` row may still be due;
/// `SWITCH_IDLE` otherwise.
static mut SWITCH_CLOCK: u16 = SWITCH_IDLE;
const SWITCH_IDLE: u16 = 600;
/// One simulation tick of the TinkEffect delay and of the opening gate's sounds.
pub fn tick() {
    unsafe {
        TINK_WAIT = TINK_WAIT.saturating_sub(1);
        if SWITCH_CLOCK < SWITCH_IDLE {
            SWITCH_CLOCK += 1;
            for &(scene, event, tick, at) in SWITCH_SOUNDS {
                if tick == SWITCH_CLOCK && scene as usize == CURRENT {
                    play_one_of(&[event], at, hk_sim::fixed::ACTOR_ROLLOFF);
                }
            }
        }
    }
}
/// Armour turned the nail at `at`: the `Block Hit` prefab's AudioSource (`sword_hit_reject`,
/// the actor curve). `HealthManager.Invincible` spawns it on every blocked hit.
pub fn nail_reject(at: [i32; 2]) {
    play_one_of(&[NAIL_REJECT], at, hk_sim::fixed::ACTOR_ROLLOFF);
}
/// A TinkEffect answered the nail (`Block Hit v2`, the same clip), at most once per 15 ticks.
/// `TinkEffect.OnTriggerEnter2D` sets the spawned effect's pitch in 0.85 to 1.15. It spawns the
/// effect within two units of the Knight, always inside the curve's full-volume 39 units of the
/// camera, so it plays at full level.
#[inline(never)]
pub fn tink() {
    unsafe {
        if TINK_WAIT != 0 {
            return;
        }
        TINK_WAIT = 15;
    }
    play_random_pitch(NAIL_REJECT, 3482, 1229);
}
/// Where the resident bank holds `event`, with the rate it was cooked at, its
/// gain, its pitch register and its bytes, for a caller that plays it on a voice of its own.
fn lookup(event: u8) -> Option<(u32, u32, i16, u16, u32)> {
    let current = unsafe { CURRENT };
    let found = if current == NONE {
        None
    } else {
        entries(current).iter().find(|e| e.0 == event)
    };
    let Some((_, address, _, bytes, rate, pitch)) = found.map(unpack) else {
        unsafe {
            HK_SCENE_SFX_MISSED = HK_SCENE_SFX_MISSED.saturating_add(1);
        }
        return None;
    };
    // A fitted scene cooks a clip at its own rate (host/scene_sfx.py `fit`),
    // so the rate and pitch come from the entry; the gain is the event's.
    let (_, gain, _) = EVENT_PARAMS[event as usize];
    unsafe {
        HK_SCENE_SFX[event as usize] = HK_SCENE_SFX[event as usize].saturating_add(1);
    }
    Some((address, rate, gain, pitch, bytes))
}
/// A scene sound on the resident banks' shared voice rather than a pool voice: for a long
/// clip (the False Knight's roars), where nothing else on the shared voice sounds at the
/// same time.
pub fn play_shared(event: u8) {
    if let Some((address, rate, gain, pitch, _)) = lookup(event) {
        audio::shared_clip(address, rate, gain, pitch);
    }
}
/// The scene-bank event that is the break clip of the ordinary Breakable `id` (scene * 128 + state),
/// for the families whose clip is not the resident wall hit (barrels, poles, wagons).
pub fn break_sound(id: usize) -> Option<u8> {
    let id = u16::try_from(id).ok()?;
    BREAK_SOUNDS
        .binary_search_by_key(&id, |&(owner, _)| owner)
        .ok()
        .map(|at| BREAK_SOUNDS[at].1)
}
/// Whether the resident bank holds `event`, without counting a miss.
pub fn resident(event: u8) -> bool {
    let current = unsafe { CURRENT };
    current != NONE && entries(current).iter().any(|e| e.0 == event)
}
/// `play` with the clip's pitch scaled by `scale` / 4096. One body for every caller: inlined into
/// each call site it cost more than the sounds it plays.
#[inline(never)]
pub fn play_pitched(event: u8, scale: u32) {
    let Some((address, rate, gain, pitch, bytes)) = lookup(event) else {
        return;
    };
    let pitch = (pitch as u32 * scale / 4096).min(0x3fff) as u16;
    if let Some(claim) = audio::pool_play(weight(event), (address, rate, gain), Some(pitch), bytes) {
        remember(claim);
    }
}
/// One scene sound on a pool voice, if the resident bank holds it.
pub fn play(event: u8) {
    play_pitched(event, 4096);
}
/// A looped scene sound (`make_loop`) at `level`/4096 of its gain, on a pool voice. Call every
/// frame the source can be heard: it starts the loop when it holds no voice (the first call, or
/// after a heavier sound took its voice) and otherwise only moves the volume. A zero level
/// silences it without key off, so it keeps its place in the loop.
pub fn loop_tick(event: u8, level: i32) {
    let Some((_, address, _, _, rate, pitch)) = (unsafe {
        let current = CURRENT;
        if current == NONE {
            None
        } else {
            entries(current).iter().find(|e| e.0 == event)
        }
    })
    .map(unpack) else {
        return;
    };
    let (_, gain, _) = EVENT_PARAMS[event as usize];
    let gain = (i32::from(gain) * level.clamp(0, 4096) / 4096) as i16;
    match unsafe { LOOPING } {
        Some((looping, claim)) if looping == event && audio::pool_holds(claim) => {
            audio::pool_set_gain(claim, gain);
        }
        held => {
            if let Some((_, claim)) = held {
                audio::pool_stop(claim);
            }
            let claim = audio::pool_loop(weight(event), (address, rate, gain), Some(pitch));
            unsafe {
                LOOPING = claim.map(|c| (event, c));
            }
        }
    }
}
/// Stop a looped scene sound that `loop_tick` started (the source went away).
pub fn loop_stop() {
    if let Some((_, claim)) = unsafe { LOOPING.take() } {
        audio::pool_stop(claim);
    }
}
/// Myla, the `Miner` of Crossroads_45. Her AudioSource hums (`Miner_idle_hum`, looped, volume
/// 0.854, rolloff 39 to 56.3 from the camera) from the start of the scene; her `Miner FX` FSM
/// plays one of two pickaxe clips when her sprite reaches frame id 20, the fifth frame of
/// `Idle` (`MYLA_STRIKE_FRAME`), through the same AudioSource and so the same rolloff.
pub const MYLA_STRIKE_FRAME: usize = 4;
const MYLA_VOLUME: i32 = 3498;
const MYLA_ROLLOFF: (i32, i32) = (39, 56);
fn myla_level(camera: (i32, i32), at: [i32; 2]) -> i32 {
    hk_sim::fixed::source_rolloff(at[0] - camera.0, at[1] - camera.1, MYLA_ROLLOFF.0, MYLA_ROLLOFF.1) * MYLA_VOLUME / 4096
}
/// Every frame the scene is resident: the hum, at the camera's distance from her.
pub fn myla_hum(camera: (i32, i32), at: Option<[i32; 2]>) {
    match at {
        Some(at) if resident(MINER_IDLE_HUM) => loop_tick(MINER_IDLE_HUM, myla_level(camera, at)),
        _ => loop_stop(),
    }
}
/// Each frame her `Idle` clip is at `frame` (its own index within the clip): one pickaxe clip
/// on the frame the strike starts, if she is within earshot.
pub fn myla_strike(camera: (i32, i32), at: [i32; 2], frame: usize) {
    static mut WAS: bool = false;
    static mut RANDOM: u32 = 0x6d79_6c61;
    let striking = frame == MYLA_STRIKE_FRAME;
    let rising = striking && unsafe { !WAS };
    unsafe {
        WAS = striking;
    }
    if !rising || !resident(MYLA_PICKAXE_1) {
        return;
    }
    let level = myla_level(camera, at);
    if level == 0 {
        return;
    }
    let event = unsafe {
        RANDOM = NrLcg::step(RANDOM);
        if RANDOM >> 31 == 0 {
            MYLA_PICKAXE_1
        } else {
            MYLA_PICKAXE_2
        }
    };
    if let Some((address, rate, gain, pitch, _)) = lookup(event) {
        let gain = (i32::from(gain) * level / 4096).min(i32::from(i16::MAX)) as i16;
        audio::shared_clip(address, rate, gain, pitch);
    }
}
/// The conversation with her opens: her `Conversation Control` plays `Miner_01_sing`. Call every
/// frame with whether she is speaking; it plays on the rising edge, on the shared voice so the
/// hum on the scene voice carries on under it.
pub fn myla_sing(speaking: bool) {
    static mut WAS: bool = false;
    let rising = speaking && unsafe { !WAS };
    unsafe {
        WAS = speaking;
    }
    if rising && resident(MINER_01_SING) {
        play_shared(MINER_01_SING);
    }
}
