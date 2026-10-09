#[path = "../../../game/src/ambience_state.rs"]
mod state;
// The cooked table comes through the mixer rather than beside it: the stem
// count and the voice budget are all read from it
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
fn an_empty_bank_is_ready_and_starts_nothing_it_does_not_hold() {
    // Every clip arrives with the scene that plays it, so readiness waits for
    // nothing, and a cue keys on only what is resident.
    let mut m = Mixer::new();
    assert!(m.finish());
    assert_eq!(cue(&mut m, 0), 0);
    for _ in 0..60 {
        assert_eq!(m.tick(), 0);
    }
    assert_eq!(m.playing, 0);
    assert_eq!(m.voice_mask(), 0);
    assert!(m.missing > 0);
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
/// one, and none holds area music's.
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
        assert!(
            data::AMBIENCE_POOL_VOICES.contains(&voice) && voice != data::MUSIC_VOICE,
            "stem {stem} is on voice {voice}, which ambience does not pool"
        );
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
    assert_eq!(
        m.voice_mask(),
        mask,
        "the published voice mask is not the one being driven"
    );
    assert_eq!(
        mask & !VOICE_MASK,
        0,
        "a stem is on a voice ambience does not own"
    );
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
            if held[stem] != NO_VOICE {
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
    let mut worst = 0;
    for a in data::AMBIENCE_SCENES.iter() {
        for b in data::AMBIENCE_SCENES.iter() {
            worst = worst.max((a.mask | b.mask).count_ones() as usize);
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
    let (stem, other) = (0, 1);
    assert_eq!(m.cue(0, 1 << stem, [10; STEMS], 0), 1 << stem);
    let voice = m.voice(stem);
    assert_ne!(voice, NO_VOICE);
    assert_eq!(m.tick(), 0);
    assert_eq!(m.cue(1, 1 << other, [10; STEMS], 0), 1 << other);
    let stop = m.tick();
    assert_eq!(stop, 1 << stem);
    assert_eq!(
        m.voice(stem),
        voice,
        "the voice was recycled before key-off"
    );
    assert_ne!(m.voice(other), voice);
    m.release(stop);
    assert_eq!(m.voice(stem), NO_VOICE);
}

/// With every pooled voice out, a cue leaves the stem unplayed and counts it
/// rather than taking a voice another stem has not finished fading out on.
#[test]
fn a_cue_the_pool_cannot_serve_drops_the_stem_and_says_so() {
    let mut m = loaded();
    assert_eq!(m.exhaust_pool(), POOL);
    assert_eq!(m.cue(0, 1, [10; STEMS], 30), 0);
    assert_eq!(m.playing, 0);
    assert_eq!(m.voice(0), NO_VOICE);
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
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    let tail = text
        .split(&format!("const {name}"))
        .nth(1)
        .unwrap_or_else(|| panic!("no {name} in {file}"));
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
fn actual_bank_keeps_every_cue_apart_below_the_tail_and_all_checksums_match() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/ambience");
    let mut end = data::AMBIENCE_SPU_START;
    // The resident channel set is a cook-time budget decision that moves: a
    // swap of 6 for 7 let Greenpath be heard, and this test restated the old
    // set as a literal, so it failed for naming the answer rather than the
    // rule. What holds at any set is that the clips arrive in channel order.
    let mut previous: Option<u8> = None;
    for (i, c) in data::AMBIENCE_CLIPS.iter().enumerate() {
        // Clips share SPU bytes with clips of other areas now, so they are
        // not contiguous; each still sits inside ambience's range.
        assert!(c.spu_address >= data::AMBIENCE_SPU_START && c.spu_address % 16 == 0);
        if let Some(last) = previous {
            assert!(
                c.source_channel > last,
                "clips must arrive in channel order"
            );
        }
        previous = Some(c.source_channel);
        // A pitch of 0 never advances the decoder and anything above 0x1000
        // resamples the clip above its 44.1 kHz source.
        assert!(
            c.pitch > 0 && c.pitch <= 0x1000,
            "clip {i} pitch {} is unplayable",
            c.pitch
        );
        assert_eq!(c.byte_len % 16, 0);
        let bytes = std::fs::read(root.join(format!("clip_{i}.adpcm"))).unwrap();
        assert!(valid_clip(&bytes, c.byte_len, c.checksum));
        // Every loop is held whole; area music has the only ring.
        assert_eq!(c.spu_bytes, c.byte_len);
        end = end.max(c.spu_address + c.spu_bytes as u32);
    }
    assert_eq!(end, data::AMBIENCE_SPU_END);
    // Every cue's stems hold their own bytes: a cue plays them together.
    for cue in data::AMBIENCE_SCENES.iter() {
        for a in 0..STEMS {
            for b in 0..a {
                if cue.mask & (1 << a) == 0 || cue.mask & (1 << b) == 0 {
                    continue;
                }
                let (x, y) = (data::AMBIENCE_CLIPS[a], data::AMBIENCE_CLIPS[b]);
                assert!(
                    x.spu_address + x.spu_bytes as u32 <= y.spu_address
                        || y.spu_address + y.spu_bytes as u32 <= x.spu_address,
                    "stems {a} and {b} of one cue share SPU"
                );
            }
        }
    }
    // Ambience ends at or below where the first bank above it says it begins,
    // and the whole stack ends inside SPU RAM. The gap between them is free.
    // Above ambience: the music ring, the world one-shots, Focus and Runner,
    // each abutting the next.
    let world = bank_const("world-sfx.rs", "SPU_BASE");
    let focus = bank_const("focus-audio.rs", "SPU_BASE");
    let runner = bank_const("runner-audio.rs", "BANK_BASE");
    assert!(
        end <= data::MUSIC_RING_BASE,
        "ambience runs into the music ring"
    );
    assert_eq!(
        data::MUSIC_RING_BASE + data::MUSIC_RING_BYTES as u32,
        world,
        "the music ring no longer sits below the world bank"
    );
    assert_eq!(
        world + bank_const("world-sfx.rs", "BANK_BYTES"),
        focus,
        "the world bank no longer sits below Focus"
    );
    // Focus (and the ability one-shots that ride its range) may stop short of
    // Runner: what is left between them is free for more Knight sounds.
    assert!(
        focus + bank_const("focus-audio.rs", "BANK_BYTES") <= runner,
        "the Focus bank runs into Runner"
    );
    assert!(runner + bank_const("runner-audio.rs", "BANK_BYTES") <= 0x80000);
}

#[test]
fn a_clip_checked_in_pieces_agrees_with_the_whole_clip_check() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/ambience");
    for (i, c) in data::AMBIENCE_CLIPS.iter().enumerate() {
        let bytes = std::fs::read(root.join(format!("clip_{i}.adpcm"))).unwrap();
        for piece in [16, 2048, 8192] {
            let mut check = ClipCheck::new();
            assert!(bytes.chunks(piece).all(|p| check.feed(p, c.byte_len)));
            assert!(check.finish(c.byte_len, c.checksum));
        }
        // One flipped bit anywhere fails, whichever piece carries it.
        let mut bad = bytes.clone();
        let middle = bad.len() / 2;
        bad[middle] ^= 0x10;
        let mut check = ClipCheck::new();
        let fed = bad.chunks(8192).all(|p| check.feed(p, c.byte_len));
        assert!(!fed || !check.finish(c.byte_len, c.checksum));
        // Too many bytes never pass.
        let mut check = ClipCheck::new();
        assert!(!check.feed(&[bytes.as_slice(), &[0u8; 16]].concat(), c.byte_len));
    }
}

/// The byte-wise check `ClipCheck::feed` replaced, kept as its oracle.
fn reference_feed(hash: &mut u32, index: &mut usize, bytes: &[u8], length: usize) -> bool {
    if length == 0 || length % 16 != 0 {
        return false;
    }
    for &byte in bytes {
        let i = *index;
        if i >= length || (i == 0 && byte >> 4 != 0) {
            return false;
        }
        if i % 16 == 0 && (byte >> 4 > 4 || byte & 15 > 12) {
            return false;
        }
        if i % 16 == 1
            && byte != (if i == 1 { 4 } else { 0 } | if i == length - 15 { 3 } else { 0 })
        {
            return false;
        }
        *hash = (*hash ^ byte as u32).wrapping_mul(0x01000193);
        *index += 1;
    }
    true
}

#[test]
fn piecewise_clip_checks_match_the_byte_wise_reference() {
    let mut seed = 0x9e37_79b9u32;
    let mut next = move |n: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed % n
    };
    for round in 0..400 {
        let length = 16 * (1 + next(40) as usize);
        // Mostly well-formed blocks, with an occasional bad header or flag.
        let mut clip: Vec<u8> = (0..length)
            .map(|i| match i % 16 {
                0 => {
                    if i == 0 {
                        next(13) as u8
                    } else {
                        ((next(5) << 4) | next(13)) as u8
                    }
                }
                1 => (if i == 1 { 4 } else { 0 }) | (if i == length - 15 { 3 } else { 0 }),
                _ => next(256) as u8,
            })
            .collect();
        if next(4) == 0 {
            let at = next(length as u32) as usize;
            clip[at] ^= 1 << next(8);
        }
        let extra = if next(8) == 0 { 16 } else { 0 };
        clip.extend(std::iter::repeat(0).take(extra));
        let (mut hash, mut index) = (0x811c9dc5u32, 0usize);
        let mut check = ClipCheck::new();
        let mut at = 0;
        while at < clip.len() {
            let piece = (1 + next(70) as usize).min(clip.len() - at);
            let expected = reference_feed(&mut hash, &mut index, &clip[at..at + piece], length);
            assert_eq!(
                check.feed(&clip[at..at + piece], length),
                expected,
                "round {round} at {at}"
            );
            if !expected {
                break;
            }
            at += piece;
            for checksum in [hash, hash ^ 1] {
                assert_eq!(
                    check.finish(length, checksum),
                    index == length && hash == checksum,
                    "round {round}"
                );
            }
        }
    }
}

#[test]
fn a_cut_stem_stops_gives_up_its_bytes_and_never_fades_back() {
    let mut m = loaded();
    let cave = data::AMBIENCE_SCENES[0];
    assert_eq!(cue(&mut m, 0), cave.mask);
    let victim = (0..STEMS).find(|&s| cave.mask & (1 << s) != 0).unwrap() as u32;
    let audible = m.cut(1 << victim);
    assert_eq!(audible, 1 << victim);
    m.release(audible);
    assert_eq!(m.voice(victim as usize), NO_VOICE);
    assert_eq!(m.loaded & (1 << victim), 0);
    for _ in 0..60 {
        m.tick();
    }
    assert_eq!(m.playing & (1 << victim), 0);
    assert_eq!(m.gains[victim as usize], 0);
    // A missing stem is never keyed on: a later cue counts it instead.
    let before = m.missing;
    m.cue(1, 1 << victim, [10; STEMS], 0);
    assert_eq!(m.playing & (1 << victim), 0);
    assert_eq!(m.missing, before + 1);
}
