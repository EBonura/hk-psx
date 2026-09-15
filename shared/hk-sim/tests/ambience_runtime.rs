#[path = "../../../data/ambience.rs"]
mod data;
#[path = "../../../game/src/ambience_state.rs"]
mod state;
use state::*;

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

#[test]
fn actual_complete_bank_is_contiguous_above_sfx_and_all_checksums_match() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/ambience");
    let mut end = data::AMBIENCE_SPU_START;
    let channels = [0, 1, 3, 4, 5, 6];
    for (i, c) in data::AMBIENCE_CLIPS.iter().enumerate() {
        assert_eq!(c.spu_address, end);
        assert_eq!(c.voice, 6 + i as u8);
        assert_eq!(c.source_channel, channels[i]);
        assert_eq!(c.pitch, 743);
        assert_eq!(c.byte_len % 16, 0);
        let bytes = std::fs::read(root.join(format!("clip_{i}.adpcm"))).unwrap();
        assert!(valid_clip(&bytes, c.byte_len, c.checksum));
        if i == 3 {
            assert_eq!(c.spu_bytes, data::AMBIENCE_STREAM_BYTES);
            assert_eq!(c.byte_len, data::AMBIENCE_STREAM_SOURCE_BYTES);
        } else {
            assert_eq!(c.spu_bytes, c.byte_len);
        }
        end += c.spu_bytes as u32;
    }
    assert_eq!(end, 0x5EA00);
    assert_eq!(data::AMBIENCE_SPU_END-end, 136704);
}
