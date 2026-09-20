#[path = "../../../game/src/ambience_state.rs"]
mod state;
// The cooked table comes through the mixer rather than beside it: the stem
// count, the streamed clip's index and the voice budget are all read from it
// there, so this cannot test a set the guest is not compiled against.
use state::{data, *};

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

/// Every stem holds a voice exactly while it is audible, no two hold the same
/// one, and the streamed stem holds its own for the life of the bank because
/// audio_stream.rs owns the single SPU IRQ address register while it runs.
/// Returns which stems hold what, so a caller can check that the pool is being
/// recycled rather than bound.
fn assignment(m: &Mixer) -> [u8; STEMS] {
    let mut held = [NO_VOICE; STEMS];
    for stem in 0..STEMS {
        let voice = m.voice(stem);
        assert_eq!(
            voice != NO_VOICE,
            m.playing & (1 << stem) != 0,
            "stem {stem} holds a voice it is not playing on, or plays on none"
        );
        if voice == NO_VOICE {
            continue;
        }
        if stem == STREAM_INDEX {
            assert_eq!(voice, data::AMBIENCE_STREAM_VOICE);
        } else {
            assert!(
                data::AMBIENCE_POOL_VOICES.contains(&voice),
                "stem {stem} is on voice {voice}, which ambience does not pool"
            );
        }
        assert!(
            !held.contains(&voice),
            "two stems are keyed on to voice {voice}"
        );
        held[stem] = voice;
    }
    let mut mask = 0u32;
    for voice in held.iter().filter(|v| **v != NO_VOICE) {
        mask |= 1 << *voice as u32;
    }
    assert_eq!(m.voice_mask(), mask, "the published voice mask is not the one being driven");
    assert_eq!(mask & !VOICE_MASK, 0, "a stem is on a voice ambience does not own");
    held
}

/// Settle one cue, giving each stopped stem's voice back only once the caller
/// would have keyed it off, which is the order ambience::tick uses.
fn settle(m: &mut Mixer, scene: usize) -> [u8; STEMS] {
    for _ in 0..=data::AMBIENCE_SCENES[scene].fade_ticks {
        let stop = m.tick();
        if stop != 0 {
            for stem in 0..STEMS {
                if stop & (1 << stem) != 0 {
                    assert_ne!(m.voice(stem), NO_VOICE, "a stopped stem was on no voice");
                }
            }
            m.release(stop);
        }
        assignment(m);
    }
    assignment(m)
}

/// The change this whole file exists for: a voice belongs to a stem only while
/// that stem is audible, so walking the cooked cue table hands the same voice
/// to more than one stem. Under the old table it was the same voice for the
/// life of the disc, which is what capped the resident set at the voice count.
#[test]
fn a_pooled_voice_serves_more_than_one_stem_over_the_cooked_cues() {
    let mut m = loaded();
    let mut users = [0u8; STEMS];
    for scene in 0..data::AMBIENCE_SCENES.len() {
        cue(&mut m, scene as u8);
        let held = settle(&mut m, scene);
        assert_eq!(m.playing, data::AMBIENCE_SCENES[scene].mask);
        for stem in 0..STEMS {
            if held[stem] != NO_VOICE && stem != STREAM_INDEX {
                let index = data::AMBIENCE_POOL_VOICES
                    .iter()
                    .position(|v| *v == held[stem])
                    .unwrap();
                users[index] |= 1 << stem;
            }
        }
    }
    // No cue pair in the cooked table can outrun the pool, so nothing is lost.
    assert_eq!(m.denied, 0);
    assert!(
        users.iter().any(|stems| stems.count_ones() > 1),
        "no pooled voice was ever handed to a second stem: {users:?}"
    );
}

/// The pool is sized on how many stems can be audible at the same moment, not
/// on how many are resident. A transition fades the outgoing cue's stems out
/// while the incoming cue's rise, so both sets hold voices at once; this is the
/// guest half of the check host/ambience.py makes before it writes the bank.
#[test]
fn no_pair_of_cooked_cues_can_ask_for_more_voices_than_the_pool_holds() {
    let stream = 1u8 << STREAM_INDEX;
    let mut worst = 0;
    for a in data::AMBIENCE_SCENES.iter() {
        for b in data::AMBIENCE_SCENES.iter() {
            worst = worst.max(((a.mask | b.mask) & !stream).count_ones() as usize);
        }
    }
    assert!(
        worst <= POOL,
        "{worst} stems can be live across a transition and the pool holds {POOL}"
    );
}

/// A stem keeps its voice until the caller has keyed it off, and only then does
/// the pool hand it out again. Keying a second stem on to a voice the SPU has
/// not been told about yet is the one ordering this split exists to prevent.
#[test]
fn a_voice_comes_back_to_the_pool_only_after_its_stem_is_released() {
    let mut m = loaded();
    let stem = (0..STEMS).find(|i| *i != STREAM_INDEX).unwrap();
    let other = (0..STEMS).find(|i| *i != STREAM_INDEX && *i != stem).unwrap();
    assert_eq!(m.cue(0, 1 << stem, [10; STEMS], 0), 1 << stem);
    let voice = m.voice(stem);
    assert_ne!(voice, NO_VOICE);
    assert_eq!(m.tick(), 0);
    assert_eq!(m.cue(1, 1 << other, [10; STEMS], 0), 1 << other);
    let stop = m.tick();
    assert_eq!(stop, 1 << stem);
    assert_eq!(m.voice(stem), voice, "the voice was recycled before key-off");
    assert_ne!(m.voice(other), voice);
    m.release(stop);
    assert_eq!(m.voice(stem), NO_VOICE);
}

/// With every pooled voice out, a cue leaves the stem unplayed and counts it
/// rather than taking a voice another stem has not finished fading out on. The
/// streamed stem is not in the pool, so a dry pool never silences it.
#[test]
fn a_cue_the_pool_cannot_serve_drops_the_stem_and_says_so() {
    let mut m = loaded();
    assert_eq!(m.exhaust_pool(), POOL);
    let stem = (0..STEMS).find(|i| *i != STREAM_INDEX).unwrap();
    assert_eq!(m.cue(0, 1 << stem, [10; STEMS], 30), 0);
    assert_eq!(m.playing, 0);
    assert_eq!(m.voice(stem), NO_VOICE);
    assert_eq!(m.denied, 1);
    assert_eq!(
        m.cue(1, 1 << STREAM_INDEX, [10; STEMS], 30),
        1 << STREAM_INDEX
    );
    assert_eq!(m.voice(STREAM_INDEX), data::AMBIENCE_STREAM_VOICE);
    assert_eq!(m.denied, 1);
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
