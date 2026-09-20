#[path = "../../../data/ambience.rs"]
mod data;
#[path = "../../../game/src/ambience_state.rs"]
mod state;
use state::*;

// The mixer's stem count is hand-written and the clip table is cooked from
// `cook_music.RESIDENT_ATMOS_CHANNELS`. Widening the resident set has to move
// both, so a set that outgrew the mixer fails here rather than indexing a cue's
// gains off the end of a stem array.
const _: () = assert!(STEMS == data::AMBIENCE_CLIPS.len());

fn loaded() -> Mixer {
    let mut mixer = Mixer::new();
    mixer.loaded = ALL;
    assert!(mixer.finish());
    mixer
}
fn cue(m: &mut Mixer, index: u8) -> u8 {
    let c = data::AMBIENCE_SCENES[index as usize];
    m.cue(index, c.mask, c.gains, c.fade_ticks)
}

#[test]
fn incomplete_bank_cannot_start_even_with_five_good_clips() {
    for missing in 0..STEMS {
        let mut m = Mixer::new();
        m.loaded = ALL & !(1 << missing);
        assert!(!m.finish());
        assert_eq!(cue(&mut m, 0), 0);
        for _ in 0..60 {
            assert_eq!(m.tick(), 0);
        }
        assert_eq!(m.playing, 0);
        assert_eq!(m.gains, [0; STEMS]);
    }
}

#[test]
fn source_cave_surface_transition_keeps_rain_phase_and_delays_stop() {
    let mut m = loaded();
    let cave = data::AMBIENCE_SCENES[0];
    let surface = data::AMBIENCE_SCENES[1];
    assert_eq!(cave.fade_ticks, 30);
    assert_eq!(surface.fade_ticks, 30);
    assert_eq!(cue(&mut m, 0), cave.mask);
    for _ in 0..30 {
        m.tick();
    }
    assert_eq!(m.gains, cave.gains);
    assert_eq!(cue(&mut m, 1), surface.mask & !cave.mask);
    assert_eq!(m.playing, cave.mask | surface.mask);
    for _ in 0..29 {
        assert_eq!(m.tick(), 0);
        assert_eq!(m.gains[4..], cave.gains[4..]);
    }
    assert_eq!(m.tick(), cave.mask & !surface.mask);
    assert_eq!(m.playing, surface.mask);
    assert_eq!(m.gains, surface.gains);
    assert!(!m.transitioning);
    assert_eq!(m.tick(), 0);
}

#[test]
fn immediate_cue_settles_once_including_disabled_voice_stop() {
    let mut m = loaded();
    assert_eq!(m.cue(0, 3, [10; STEMS], 0), 3);
    assert!(m.transitioning);
    assert_eq!(m.tick(), 0);
    assert!(!m.transitioning);
    assert_eq!(m.gains, [10; STEMS]);
    assert_eq!(m.cue(1, 1, [2; STEMS], 0), 0);
    assert_eq!(m.tick(), 2);
    assert_eq!(m.playing, 1);
    assert_eq!(m.tick(), 0);
}

#[test]
fn repeated_grid_cues_do_not_reset_fade_or_key_on() {
    let mut m = loaded();
    cue(&mut m, 0);
    for elapsed in 1..=30 {
        m.tick();
        assert_eq!(cue(&mut m, 0), 0);
        assert_eq!(m.elapsed, elapsed);
    }
    assert_eq!(m.gains, data::AMBIENCE_SCENES[0].gains);
}

#[test]
fn reversal_during_fade_preserves_running_stems_and_current_volume() {
    let mut m = loaded();
    cue(&mut m, 0);
    for _ in 0..30 {
        m.tick();
    }
    cue(&mut m, 1);
    for _ in 0..12 {
        m.tick();
    }
    let halfway = m.gains;
    assert_eq!(cue(&mut m, 0), 0); // Cave stems were never stopped.
    assert_eq!(m.gains, halfway);
    for _ in 0..29 {
        assert_eq!(m.tick(), 0);
    }
    assert_eq!(
        m.tick(),
        data::AMBIENCE_SCENES[1].mask & !data::AMBIENCE_SCENES[0].mask
    );
    assert_eq!(m.gains, data::AMBIENCE_SCENES[0].gains);
}

fn hash(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5u32, |h, b| {
        (h ^ *b as u32).wrapping_mul(0x01000193)
    })
}

#[test]
fn malformed_flags_headers_truncation_and_checksum_cannot_be_admitted() {
    let mut bytes = [0u8; 32];
    bytes[1] = 4;
    bytes[17] = 3;
    assert!(valid_clip(&bytes, 32, hash(&bytes)));
    assert!(!valid_clip(&bytes, 31, hash(&bytes)));
    assert!(!valid_clip(&bytes[..16], 32, hash(&bytes)));
    assert!(!valid_clip(&bytes, 32, hash(&bytes) ^ 1));
    for (offset, value) in [(0, 0x10), (0, 13), (16, 0x50), (1, 0), (17, 1), (17, 7)] {
        let mut bad = bytes;
        bad[offset] = value;
        assert!(!valid_clip(&bad, 32, hash(&bad)));
    }
    let mut single = [0; 16];
    single[1] = 7;
    assert!(valid_clip(&single, 16, hash(&single)));
}

/// One `pub const <name>...=<integer>;` out of a generated audio manifest. The
/// banks stacked above ambience declare their own base, and reading it beats
/// restating it here, which is how this test came to assert an address the cook
/// had already moved.
fn bank_const(file: &str, name: &str) -> u32 {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data").join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    let tail = text.split(&format!("const {name}")).nth(1).unwrap_or_else(|| panic!("no {name} in {file}"));
    let digits: String = tail
        .split('=')
        .nth(1)
        .unwrap()
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap()
}

#[test]
fn actual_complete_bank_is_contiguous_above_sfx_and_all_checksums_match() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/ambience");
    let mut end = data::AMBIENCE_SPU_START;
    // The resident channel set is a cook-time budget decision that moves: a
    // swap of 6 for 7 let Greenpath be heard, and this test restated the old
    // set as a literal, so it failed for naming the answer rather than the
    // rule. What holds at any set is that the clips arrive in channel order.
    let mut previous: Option<u8> = None;
    // Which clip streams is the one whose SPU reservation is a ring rather than
    // its whole payload, which is how the guest finds it; this used to read
    // `i == 3`. Its cook rate is a separate decision from streaming and both
    // used to be asserted as the one pitch pair they happened to produce, so a
    // resident clip cooked below the default rate failed here for being right.
    let mut streamed = None;
    for (i, c) in data::AMBIENCE_CLIPS.iter().enumerate() {
        assert_eq!(c.spu_address, end);
        assert_eq!(c.voice, 6 + i as u8);
        if let Some(last) = previous {
            assert!(c.source_channel > last, "clips must arrive in channel order");
        }
        previous = Some(c.source_channel);
        // A pitch of 0 never advances the decoder and anything above 0x1000
        // resamples the clip above its 44.1 kHz source.
        assert!(c.pitch > 0 && c.pitch <= 0x1000, "clip {i} pitch {} is unplayable", c.pitch);
        assert_eq!(c.byte_len % 16, 0);
        let bytes = std::fs::read(root.join(format!("clip_{i}.adpcm"))).unwrap();
        assert!(valid_clip(&bytes, c.byte_len, c.checksum));
        if c.spu_bytes != c.byte_len {
            assert!(streamed.is_none(), "more than one streamed clip");
            streamed = Some(i);
            assert_eq!(c.spu_bytes, data::AMBIENCE_STREAM_BYTES);
            assert_eq!(c.byte_len, data::AMBIENCE_STREAM_SOURCE_BYTES);
            assert!(c.byte_len > c.spu_bytes, "a ring only pays for itself on a clip too big for SPU");
        }
        end += c.spu_bytes as u32;
    }
    assert!(streamed.is_some(), "no streamed clip");
    // Ambience ends exactly where the first bank above it says it begins, and
    // the whole stack ends inside SPU RAM. Both were literals, and the second
    // of them, 136,704 free, counted the Focus and Runner banks as headroom.
    let focus = bank_const("focus-audio.rs", "SPU_BASE");
    let runner = bank_const("runner-audio.rs", "BANK_BASE");
    assert_eq!(end, focus, "the Focus bank no longer abuts ambience");
    assert_eq!(focus + bank_const("focus-audio.rs", "BANK_BYTES"), runner);
    assert!(runner + bank_const("runner-audio.rs", "BANK_BYTES") <= data::AMBIENCE_SPU_END);
}
