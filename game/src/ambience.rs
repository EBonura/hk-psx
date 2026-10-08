//! Complete source ambience loops in the voices ambience owns. Each loop is
//! loaded whole into SPU at the scene gate that first needs it, at an address
//! it shares with loops of other areas (host/ambience.py `allocate`), and
//! borrows one of the pooled voices for as long as it is audible. Call after
//! audio::init. This module never resets the shared SPU or owns CD IO.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path="volume.rs"]mod volume;
static mut LEVEL:u8=10;
#[path = "ambience_state.rs"]
mod state;
// The cooked table, which is where the stem count and the voice budget come
// from, area music's voice and ring included.
pub use state::data::*;
pub const CLIPS: &[AmbienceClip] = &AMBIENCE_CLIPS;
static mut MIXER: state::Mixer = state::Mixer::new();

/// Background loads of the clips the scenes one gate away play, so an
/// area-change gate finds them resident. `disc::pump` runs one piece at a
/// time through `STAGE` when the drive is idle; a gate load abandons a clip
/// half done, and the gate then loads it itself.
const STAGE_SECTORS: usize = 2;
#[repr(C, align(4))]
struct Stage([u8; STAGE_SECTORS * 2048]);
static mut STAGE: Stage = Stage([0; STAGE_SECTORS * 2048]);
const NO_CLIP: u8 = u8::MAX;
struct Prefetch { want: u8, skip: u8, clip: u8, offset: usize, reading: usize, check: state::ClipCheck }
static mut PREFETCH: Prefetch = Prefetch { want: 0, skip: 0, clip: NO_CLIP, offset: 0, reading: 0, check: state::ClipCheck::new() };
/// Where each clip starts on the disc, filled in once the directory is read.
static mut CLIP_LBA: [u32; AMBIENCE_CLIPS.len()] = [0; AMBIENCE_CLIPS.len()];
pub fn set_clip_lba(clip: usize, lba: u32) { unsafe { CLIP_LBA[clip] = lba; } }
/// Clips loaded in the background, the pieces read for them, and clips
/// abandoned half way (a gate came first, or a check failed).
#[no_mangle]
pub static mut HK_AMBIENCE_PREFETCHED: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_PREFETCH_READS: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_PREFETCH_ABORTS: u32 = 0;
fn prefetch() -> &'static mut Prefetch { unsafe { &mut *(&raw mut PREFETCH) } }
/// The next piece a free drive should read, as (destination, sectors, LBA).
pub fn want_prefetch() -> Option<(*mut u32, usize, u32)> {
    let p = prefetch();
    if !is_ready() || p.reading != 0 {
        return None;
    }
    if p.clip == NO_CLIP {
        let candidates = p.want & !unsafe { MIXER.loaded } & !p.skip & state::ALL;
        if candidates == 0 {
            return None;
        }
        let index = candidates.trailing_zeros() as usize;
        let (start, end) = spu_range(index);
        let mut sharing = 0u8;
        for other in 0..CLIPS.len() {
            let (lo, hi) = spu_range(other);
            if other != index && lo < end && start < hi {
                sharing |= 1 << other;
            }
        }
        // Never take bytes from a stem that can be heard; the cook keeps a
        // scene's own stems apart from its neighbours', so this only skips a
        // clip whose bytes a leftover fade still holds.
        if sharing & unsafe { MIXER.playing } != 0 {
            p.skip |= 1 << index;
            return None;
        }
        cut(sharing & unsafe { MIXER.loaded });
        p.clip = index as u8;
        p.offset = 0;
        p.check = state::ClipCheck::new();
    }
    let clip = CLIPS[p.clip as usize];
    let lba = unsafe { CLIP_LBA[p.clip as usize] };
    if lba == 0 {
        return None;
    }
    let sectors = (clip.byte_len - p.offset).div_ceil(2048).min(STAGE_SECTORS);
    p.reading = sectors;
    Some((unsafe { (&raw mut STAGE.0).cast::<u32>() }, sectors, lba + (p.offset / 2048) as u32))
}
/// The piece `want_prefetch` handed out has landed (or failed).
pub fn prefetch_done(ok: bool) {
    let p = prefetch();
    let sectors = p.reading;
    p.reading = 0;
    if sectors == 0 || p.clip == NO_CLIP {
        return;
    }
    unsafe { HK_AMBIENCE_PREFETCH_READS = HK_AMBIENCE_PREFETCH_READS.saturating_add(1); }
    let index = p.clip as usize;
    let clip = CLIPS[index];
    let bytes = (sectors * 2048).min(clip.byte_len - p.offset);
    let data = unsafe { core::slice::from_raw_parts((&raw const STAGE.0).cast::<u8>(), bytes) };
    if !ok || !p.check.feed(data, clip.byte_len) {
        abort_prefetch();
        return;
    }
    // A sector at a time, with a due pad poll between: this runs from inside a
    // checkpoint (the drive pump), like the music ring's refill.
    let at = clip.spu_address + p.offset as u32;
    crate::scene_sfx::overwritten(at, at + bytes as u32);
    for (i, part) in data.chunks(2048).enumerate() {
        spu::upload_adpcm(SpuAddr::new(clip.spu_address + (p.offset + i * 2048) as u32), part);
        crate::input::poll_only();
    }
    p.offset += bytes;
    if p.offset == clip.byte_len {
        p.clip = NO_CLIP;
        if p.check.finish(clip.byte_len, clip.checksum) {
            unsafe {
                MIXER.loaded |= 1 << index;
                HK_AMBIENCE_PREFETCHED = HK_AMBIENCE_PREFETCHED.saturating_add(1);
            }
            publish();
        } else {
            unsafe { HK_AMBIENCE_PREFETCH_ABORTS = HK_AMBIENCE_PREFETCH_ABORTS.saturating_add(1); }
        }
    }
}
/// Drop a clip half loaded; a piece still in flight lands and is ignored.
pub fn abort_prefetch() {
    let p = prefetch();
    if p.clip != NO_CLIP {
        unsafe { HK_AMBIENCE_PREFETCH_ABORTS = HK_AMBIENCE_PREFETCH_ABORTS.saturating_add(1); }
    }
    p.clip = NO_CLIP;
}

#[no_mangle]
pub static mut HK_AMBIENCE_READY: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_LOADED_MASK: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_PLAYING_MASK: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_START_COUNT: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_TRANSITIONS: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_SCENE: u32 = u32::MAX;
#[no_mangle]
pub static mut HK_AMBIENCE_FADE_TICKS: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_GAINS: [u32; state::STEMS] = [0; state::STEMS];
/// Which SPU voices the bank is driving, and how many stems a cue could not
/// key on because the pool was dry. A replay reads both: the first is the
/// evidence that a stem is on the voice the mixer says it is, the second is
/// zero for every cue pair the catalogue contains.
#[no_mangle]
pub static mut HK_AMBIENCE_VOICE_MASK: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_VOICE_DENIALS: u32 = 0;
/// Clips read from CD at scene gates, stems a cue enabled without their bytes
/// (zero by construction), and stems cut short because an incoming clip's
/// bytes landed on them: only transitions no gate describes, such as a
/// respawn at a distant bench, can cause one.
#[no_mangle]
pub static mut HK_AMBIENCE_CLIP_LOADS: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_MISSING: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_CUTS: u32 = 0;

fn publish() {
    unsafe {
        HK_AMBIENCE_READY = MIXER.ready as u32;
        HK_AMBIENCE_LOADED_MASK = MIXER.loaded as u32;
        HK_AMBIENCE_PLAYING_MASK = MIXER.playing as u32;
        HK_AMBIENCE_SCENE = if MIXER.scene == u8::MAX {
            u32::MAX
        } else {
            MIXER.scene as u32
        };
        HK_AMBIENCE_FADE_TICKS = MIXER.elapsed as u32;
        HK_AMBIENCE_VOICE_MASK = MIXER.voice_mask();
        HK_AMBIENCE_VOICE_DENIALS = MIXER.denied as u32;
        HK_AMBIENCE_MISSING = MIXER.missing as u32;
        for i in 0..state::STEMS {
            HK_AMBIENCE_GAINS[i] = if MIXER.playing & (1 << i) != 0 {
                MIXER.gains[i] as u32
            } else {
                0
            };
        }
    }
}

/// Every voice ambience owns, silenced. Which stem is on which of them is not
/// fixed, so the bank is quietened by voice rather than by clip.
fn silence() {
    for &voice in AMBIENCE_POOL_VOICES.iter() {
        Voice::new(voice).set_volume(Volume::SILENCE, Volume::SILENCE);
    }
}

/// Startup/retry only: silence before replacing any resident bytes.
pub fn begin_load() {
    silence();
    Voice::key_off(state::VOICE_MASK);
    unsafe {
        MIXER = state::Mixer::new();
        HK_AMBIENCE_START_COUNT = 0;
        HK_AMBIENCE_TRANSITIONS = 0;
    }
    publish();
}

/// The clips `scene`'s cue plays that are not resident yet, as a stem mask.
/// The scene gate loads exactly these before the scene takes the arena.
pub fn missing(scene: usize) -> u8 {
    if !is_ready() || scene >= AMBIENCE_SCENES.len() {
        return 0;
    }
    AMBIENCE_SCENES[scene].mask & state::ALL & !unsafe { MIXER.loaded }
}

const UPLOAD_SLICE: usize = 4096;
/// Where a clip lives in SPU.
fn spu_range(index: usize) -> (u32, u32) {
    let clip = CLIPS[index];
    (clip.spu_address, clip.spu_address + clip.spu_bytes as u32)
}

/// Validate a whole clip in the caller's reusable arena, then complete SDK DMA
/// to its cooked SPU address, after every resident clip sharing those bytes has
/// given them up (and been cut, if a cue still had it playing). Failed
/// validation never modifies SPU RAM or marks a clip resident.
pub fn upload(index: usize, bytes: &[u8]) -> bool {
    if index >= CLIPS.len() || unsafe { MIXER.loaded } & (1 << index) != 0 {
        return false;
    }
    let clip = CLIPS[index];
    let (start, end) = spu_range(index);
    if start % 16 != 0
        || start < AMBIENCE_SPU_START
        || end > AMBIENCE_SPU_END
        || clip.spu_bytes != clip.byte_len
        || clip.pitch == 0
        || clip.pitch > 0x3FFF
        || !state::valid_clip_polled(bytes, clip.byte_len, clip.checksum, &mut crate::input::checkpoint)
    {
        return false;
    }
    {
        let mut sharing = 0u8;
        for other in 0..CLIPS.len() {
            let (lo, hi) = spu_range(other);
            let live = unsafe { MIXER.loaded | MIXER.playing } & (1 << other) != 0;
            if other != index && live && lo < end && start < hi {
                sharing |= 1 << other;
            }
        }
        cut(sharing);
        crate::scene_sfx::overwritten(start, end);
        // In slices, servicing the pad between them: a scene gate still polls
        // it every VBlank, and one 49 KiB transfer outlasts one.
        for (slice, part) in bytes.chunks(UPLOAD_SLICE).enumerate() {
            spu::upload_adpcm(SpuAddr::new(start + (slice * UPLOAD_SLICE) as u32), part);
            crate::input::checkpoint();
        }
        unsafe { HK_AMBIENCE_CLIP_LOADS = HK_AMBIENCE_CLIP_LOADS.saturating_add(1); }
    }
    unsafe {
        MIXER.loaded |= 1 << index;
    }
    publish();
    true
}

/// A scene bank (scene_sfx.rs) is about to write `lo..hi`: every clip there
/// gives up its bytes, and is cut if a cue still has it sounding, so it loads
/// again when a cue next wants it. The cook keeps the bank clear of every stem
/// that can sound in its scene, so after a gate this only ever drops clips the
/// new scene does not play.
pub fn forget(lo: u32, hi: u32) {
    let mut stems = 0u8;
    for i in 0..CLIPS.len() {
        let (a, b) = spu_range(i);
        if a < hi && lo < b { stems |= 1 << i; }
    }
    let live = stems & unsafe { MIXER.loaded | MIXER.playing };
    if live != 0 {
        cut(live);
        publish();
    }
}

/// Stop whatever of `stems` is still sounding and give up their bytes.
fn cut(stems: u8) {
    if stems == 0 {
        return;
    }
    let audible = unsafe { MIXER.cut(stems) };
    let mut keyed = 0u32;
    for i in 0..state::STEMS {
        if audible & (1 << i) != 0 {
            let voice = Voice::new(unsafe { MIXER.voice(i) });
            voice.set_volume(Volume::SILENCE, Volume::SILENCE);
            keyed |= voice.mask();
        }
    }
    Voice::key_off(keyed);
    unsafe {
        MIXER.release(audible);
        HK_AMBIENCE_CUTS = HK_AMBIENCE_CUTS.saturating_add(audible.count_ones());
    }
}

pub fn is_ready() -> bool {
    unsafe { MIXER.ready }
}

/// Configure the empty bank; no key-on yet. Every clip arrives with the scene
/// that first plays it, through `upload`.
pub fn finish_load() -> bool {
    if is_ready() {
        return true;
    }
    silence();
    unsafe {
        MIXER.finish();
    }
    publish();
    true
}

/// 0 = Tutorial Cave, 1 = covered Town Surface. Repeated scene IDs are no-ops.
pub fn set_scene(scene: u8) -> bool {
    if !is_ready() || scene as usize >= AMBIENCE_SCENES.len() {
        return false;
    }
    if unsafe { MIXER.scene } == scene {
        return true;
    }
    let cue = AMBIENCE_SCENES[scene as usize];
    let start = unsafe { MIXER.cue(scene, cue.mask, cue.gains, cue.fade_ticks) };
    // The next area's clips, for the drive to load while it is idle.
    let p = prefetch();
    p.want = AMBIENCE_PREFETCH[scene as usize];
    p.skip = 0;
    if p.clip != NO_CLIP && p.want & (1 << p.clip) == 0 {
        abort_prefetch();
    }
    let mut keyed = 0u32;
    for i in 0..state::STEMS {
        if start & (1 << i) == 0 {
            continue;
        }
        // Silence first: a pooled voice was driving another loop the last time
        // it was used, and nothing it is still releasing should be heard at
        // this stem's volume.
        let voice = Voice::new(unsafe { MIXER.voice(i) });
        voice.set_volume(Volume::SILENCE, Volume::SILENCE);
        // Everything that selects this loop rather than the last one, set
        // here because the voice is not this stem's between cues. This preset
        // holds full sustain like sample(), but releases promptly on key-off.
        // END+REPEAT loops do not enter release. The authored mixer controls
        // fades; explicit volume zero guarantees disabled stems mute.
        voice.set_pitch(Pitch::raw(CLIPS[i].pitch));
        voice.set_start_addr(SpuAddr::new(CLIPS[i].spu_address));
        voice.set_adsr(Adsr::sample_one_shot());
        keyed |= voice.mask();
    }
    if start != 0 {
        Voice::key_on(keyed);
        // The loop point and the gain only after key-on: key-on latches the
        // repeat address from the ADPCM loop flag, and a stem that is audible
        // for the register write between the two is audible at silence.
        for i in 0..state::STEMS {
            if start & (1 << i) == 0 {
                continue;
            }
            let voice = Voice::new(unsafe { MIXER.voice(i) });
            voice.set_loop_addr(SpuAddr::new(CLIPS[i].spu_address));
            let gain = Volume(volume::scale(unsafe { MIXER.gains[i] },unsafe {LEVEL}));
            voice.set_volume(gain, gain);
        }
    }
    unsafe {
        HK_AMBIENCE_START_COUNT = HK_AMBIENCE_START_COUNT.saturating_add(start.count_ones());
        HK_AMBIENCE_TRANSITIONS = HK_AMBIENCE_TRANSITIONS.saturating_add(1);
    }
    if cue.fade_ticks == 0 {
        tick();
    } else {
        publish();
    }
    true
}

/// Once per 60 Hz tick, independently of gameplay pause and grid activation.
pub fn tick() {
    // Settled loops run wholly in the SPU: no per-tick divisions or MMIO writes.
    if unsafe { !MIXER.ready || !MIXER.transitioning } {
        return;
    }
    let stop = unsafe { MIXER.tick() };
    for i in 0..state::STEMS {
        // A stem holding no voice has nothing to write to, and the voice it
        // would have written to before now belongs to another stem.
        let voice = unsafe { MIXER.voice(i) };
        if voice == state::NO_VOICE {
            continue;
        }
        let gain = unsafe {
            if MIXER.playing & (1 << i) != 0 {
                MIXER.gains[i]
            } else {
                0
            }
        };
        let gain=Volume(volume::scale(gain,unsafe {LEVEL}));
        Voice::new(voice).set_volume(gain,gain);
    }
    if stop != 0 {
        let mut keyed = 0u32;
        for i in 0..state::STEMS {
            if stop & (1 << i) != 0 {
                keyed |= Voice::new(unsafe { MIXER.voice(i) }).mask();
            }
        }
        Voice::key_off(keyed);
        // Only now that the SPU has been told, so no cue this frame can hand a
        // pooled voice to another stem while the old one is still keying off.
        unsafe { MIXER.release(stop); }
    }
    publish();
}

/// Preserve each source stem's fade/mix while changing the user's level.
pub fn set_volume(level:u8) {
    unsafe {LEVEL=level.min(10);}
    for i in 0..state::STEMS {
        let voice=unsafe {MIXER.voice(i)};
        if voice==state::NO_VOICE {continue;}
        let source=unsafe {if MIXER.playing&(1<<i)!=0 {MIXER.gains[i]}else{0}};
        let gain=Volume(volume::scale(source,level));
        Voice::new(voice).set_volume(gain,gain);
    }
}
