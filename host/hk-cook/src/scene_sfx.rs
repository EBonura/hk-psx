//! Per-scene one-shot banks, read at the scene gate like the ambience clips.
//! Ported from host/scene_sfx.py, whose output it reproduces byte for byte.
//!
//! The resident banks (SFX, Geo, world, Focus, Runner) hold what every scene
//! can play and are full. A sound only some scenes need (a bench, a secret, an
//! arena gate, one enemy family, the False Knight's own clips) goes in its
//! scene's bank instead: one WORLD.PAK chunk per scene, first in that scene's
//! disc group, so the gate load that reads the scene reads it too with no extra
//! seek, and the guest uploads it into SPU bytes no stem that can sound in that
//! scene uses.
//!
//! Placement is the ambience allocator's rule seen from the other side. While
//! the Knight is in scene `s` the stems that can be resident are the scene's
//! own cue and the stems of every scene one gate away (the prefetch, which also
//! covers the outgoing cue still fading after a gate). `ambience.json` records
//! both per scene, so each of `s`'s clips is placed first-fit into the gaps
//! those stems leave between ambience's start and the music ring. Banks never
//! conflict with each other: only one scene's bank is resident, and the gate
//! into the next scene replaces it. Stems that are resident but not needed and
//! sit under a bank are forgotten by the guest when the bank lands
//! (`ambience::forget`), so they reload when a later cue wants them.
//!
//! A scene's events are admitted in priority order until its gaps are full; a
//! clip that does not fit is refused and recorded with its size, never trimmed.
//!
//! A scene that refuses a clip at the rows' rates is fitted instead (`fit`):
//! the SDK's rate allocator (psx_audio_cook::rate::allocate, through
//! tools/psx-audio-cook `plan`) picks one rate per clip from the row's rate
//! down the SDK ladder, so the whole set fits the scene's gaps at the least
//! time-weighted predicted loss. Every clip the scene already held at its row
//! rate keeps a floor: its new cook must score no worse than ship6's (the
//! predictor-zero encoder at the row rate) on fwSNRseg against the source and,
//! played through the SPU's Gaussian interpolation, hold the source's level at
//! least as closely, so a lower rate is taken only where the SDK encoder's gain
//! pays for it. Only when even that cannot fit is the lowest-priority clip
//! refused, as before.
//!
//! The catalogue (`EVENTS`) is the whole contract: adding a sound is a row
//! there plus one `scene_sfx::play` call in the guest. The cook writes
//! data/scene-sfx/scene_<id>.adpcm, data/scene_sfx.rs and .hkpsx/scene-sfx.json;
//! it reads ambience.json, battle-gates.json and data/regions.json, so it runs
//! after those cooks.
//!
//! Every clip goes through the same chain the resident banks use: the clip's
//! FSB decoded by FMOD (`crate::fmod`), folded to mono and resampled by ffmpeg,
//! and encoded by the SDK's psx-audio-cook command line, which also runs the
//! rate allocator (`plan`) and the quality score (`score`).

use crate::common::{err, py_round, Result};
use crate::pyjson::{dumps, Json};
use hk_unity::{Obj, Source, Value};
use serde_json::Value as J;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Gain the scene voice plays every clip at: the source's volume 1.0 scaled
/// by a third for mix headroom, as every other bank here (cook_audio volume_q14).
const GAIN: i64 = 5461;
/// An empty scene still owns a chunk, so every scene group has the same shape.
const EMPTY: [u8; 16] = [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// A trimmed clip ends in a linear fade this long, so the cut never clicks.
const TRIM_FADE_SECONDS: f64 = 0.02;
/// The SDK allocator's rate ladder, high to low (psx_audio_cook::rate::LADDER).
const LADDER: [i64; 20] = [22050, 18900, 16000, 13000, 11025, 10000, 9000, 8000, 7000, 6000, 5500, 5000, 4500, 4000, 3600, 3200, 2800, 2400, 2000, 1600];
/// How far a cook's level may stray from its source's before the no-worse
/// check counts it, when ship6's own cook strayed less: fwSNRseg ignores gain.
const LEVEL_TOLERANCE_DB: f64 = 0.5;
/// Rows at this priority or later are fitted only into what the scene's other
/// clips leave after their own fit, which is made against the layout before
/// any stem was unloaded: they never move an earlier clip's rate, so adding one
/// cannot make a scene's existing sounds better or worse. They are fitted
/// together by the same allocator, into that leftover plus any unloaded stem's
/// bytes, and refused lowest priority first when even that does not fit.
const LATE_PRIORITY: i64 = 60;
/// The encoder never resamples: the PCM goes in and out at one nominal rate.
const NOMINAL_RATE: u32 = 22050;
const LIMITATIONS: [&str; 2] = [
    "Every scene clip plays at source gain 1.0 and pitch 1.0 on one voice; a new play retriggers it.",
    "Arena gate sounds are cooked only for the arena the port drives (the False Knight).",
];

/// How a row cuts its clip's tail (`trim_tail`). No catalogue row trims since
/// 2026-10-08: a Hollow Knight sound is never cut short to save SPU RAM. A row
/// whose full tail does not fit its scene's bank gets a lower sample rate from
/// the allocator instead (the rate ladder), as the shared resampler is for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Trim {
    /// Drop everything after the last 20 ms window whose RMS is within this
    /// many dB of the clip's peak.
    Db(i64),
    /// Keep only this many seconds, for a clip the source itself cuts short.
    Seconds(f64),
}

/// One catalogue row: (event, clip file, path id, clip name, rate, where,
/// priority, trim). Lower priority is admitted first. `place` is
/// ["scene", name, ...], ["benches"], ["secrets"], ["totems"],
/// ["secret_walls"], ["secret_floors"], ["secret_breaks"] (either,
/// host/secret_breaks.py), ["chests"], ["shinies"], ["pieces"]
/// (host/pickups.py), ["stalactites"] (scenes with a stalactite hazard),
/// ["arena", name, ...] for scenes whose arena the port drives, or
/// ["family", actor name prefix]. A clip two scenes need is one row
/// naming both. Rates follow the measured share of source energy above the new
/// Nyquist (hk-audit/audio-census/prices.json).
#[derive(Clone, PartialEq, Debug)]
pub struct Event {
    pub name: &'static str,
    pub file: &'static str,
    pub path_id: i64,
    pub clip: &'static str,
    pub rate: i64,
    pub place: &'static [&'static str],
    pub priority: i64,
    pub trim: Option<Trim>,
}

#[allow(clippy::too_many_arguments)]
const fn ev(name: &'static str, file: &'static str, path_id: i64, clip: &'static str, rate: i64, place: &'static [&'static str], priority: i64, trim: Option<Trim>) -> Event {
    Event { name, file, path_id, clip, rate, place, priority, trim }
}

pub const EVENTS: &[Event] = &[
    // The False Knight's clips (cook_audio.BOSS_REFUSED), in the order the
    // fight needs them: the slam's impact (every slam), the rage roar (three
    // rages), the final hit, the jump/landing/stagger/roll cadence, the arena
    // gates, then the dying roar and the entrance. Crossroads_10's gaps hold
    // 56,432 bytes, so each rate is the lowest that keeps nearly all of the
    // clip's energy: `loss` is the share of source energy above the new
    // Nyquist, measured on the source WAV (it matches hk-audit's
    // audio-census/prices.json where both have the clip), and `tail` what a
    // -40 dB trim drops.
    //   strike_ground 3000 Hz: loss -15.0 dB (-15.6 at 4000, -16.4 at 5512);
    //                  tail 4.54 -> 4.22 s.
    //   FKnight_Rage  3000 Hz: loss -26.1 dB (-27.5 at 4000). Played on the
    //                  shared boss voice so the rage's own slams do not cut it.
    //   boss_final_hit 5512 Hz: loss -11.4 dB, the brightest; tail 2.30 -> 1.92 s.
    //   jump, land_1st_time, damage_armour_final 5512 Hz: -18.1, -18.0, -11.4.
    //   roll 4000 Hz: -24.4 dB.
    //   gate_slam 3000 Hz: loss -13.0 dB, and -13.3 even at 5512: it is broadband.
    //   FKnight_death 3000 Hz: loss -18.2 dB, cut to `Steam`'s own 3.0 s: the
    //                  source's `Blow` follows with its own explosion.
    //   ceiling_break 3000 Hz: loss -22.2 dB (-24.9 at 4000); tail 2.42 -> 2.30 s.
    ev("false_knight_strike_ground", "sharedassets48.assets", 28, "false_knight_strike_ground", 3000, &["scene", "Crossroads_10"], 0, None),
    ev("false_knight_rage", "sharedassets48.assets", 30, "FKnight_Rage", 3000, &["scene", "Crossroads_10"], 1, None),
    ev("boss_final_hit", "sharedassets32.assets", 135, "boss_final_hit", 5512, &["scene", "Crossroads_10", "Crossroads_09"], 2, None),
    ev("false_knight_jump", "sharedassets48.assets", 45, "false_knight_jump", 5512, &["scene", "Crossroads_10"], 3, None),
    ev("false_knight_land_1st_time", "sharedassets6.assets", 171, "false_knight_land_1st_time", 5512, &["scene", "Crossroads_10"], 4, None),
    ev("false_knight_damage_armour_final", "sharedassets46.assets", 22, "false_knight_damage_armour_final", 5512, &["scene", "Crossroads_10"], 5, None),
    ev("false_knight_roll", "sharedassets48.assets", 21, "false_knight_roll", 4000, &["scene", "Crossroads_10"], 6, None),
    ev("false_knight_death", "sharedassets48.assets", 38, "FKnight_death", 3000, &["scene", "Crossroads_10"], 8, None),
    ev("false_knight_ceiling_break", "sharedassets19.assets", 31, "false_knight_ceiling_break", 3000, &["scene", "Crossroads_10"], 9, None),
    ev("zombie_shield_raise", "sharedassets32.assets", 143, "zombie_shield_raise", 5512, &["scene", "Crossroads_10"], 13, None),
    ev("zombie_shield_move", "sharedassets32.assets", 87, "zombie_shield_move", 5512, &["scene", "Crossroads_10"], 14, None),
    ev("zombie_guard_footstep", "sharedassets48.assets", 29, "zombie_guard_footstep", 5512, &["scene", "Crossroads_10"], 15, None),
    // `BG Control`'s own clips, only where the port drives the arena.
    ev("gate_slam", "sharedassets27.assets", 35, "gate_slam", 3000, &["arena", "Crossroads_10", "Crossroads_09"], 7, None),
    ev("gate_open", "sharedassets27.assets", 41, "gate_open", 5512, &["arena", "Crossroads_10", "Crossroads_09"], 10, None),
    // Brooding Mawlek's (Crossroads_09), in the order the fight leans on them:
    // every landing's club, the leap, the Head's spit every half second, the
    // arm swipe's whip and call, the super spit, the wake roar, the nail
    // clash, then the corpse's explosion and steam, the offscreen wake leap,
    // and the Head's second spit voice (the Boss Defeat sting is an XA song,
    // host/hk-cook/src/xa_music.rs: 25 s of music no bank holds). Rates are the
    // bank's usual 5512 Hz for the short percussive clips and 3000 to 4000 Hz
    // for the long ones, not measured per clip as the False Knight's were;
    // boss_final_hit and the gates share the False Knight's rows above.
    ev("zombie_guard_club", "sharedassets34.assets", 107, "zombie_guard_club", 5512, &["scene", "Crossroads_09"], 0, None),
    ev("mawlek_jump", "sharedassets34.assets", 79, "mawlek_jump", 5512, &["scene", "Crossroads_09"], 1, None),
    ev("mawlek_spit", "sharedassets33.assets", 73, "mawlek_spit", 5512, &["scene", "Crossroads_09"], 3, None),
    ev("mawlek_whip", "sharedassets34.assets", 80, "mawlek_whip", 5512, &["scene", "Crossroads_09"], 4, None),
    ev("mawlek_call", "sharedassets34.assets", 100, "mawlek_call", 5512, &["scene", "Crossroads_09"], 5, None),
    ev("mawlek_big_spit", "sharedassets34.assets", 58, "mawlek_big_spit", 5512, &["scene", "Crossroads_09"], 6, None),
    ev("mawlek_scream", "sharedassets45.assets", 9, "mawlek_scream", 4000, &["scene", "Crossroads_09"], 8, None),
    ev("hero_parry", "resources.assets", 1154, "hero_parry", 5512, &["scene", "Crossroads_09"], 9, None),
    ev("boss_explode", "sharedassets32.assets", 99, "boss_explode", 3000, &["scene", "Crossroads_09"], 11, None),
    ev("boss_gushing", "sharedassets32.assets", 62, "boss_gushing", 3000, &["scene", "Crossroads_09"], 12, None),
    ev("mawlek_jump_offscreen", "sharedassets34.assets", 74, "mawlek_jump_offscreen", 3000, &["scene", "Crossroads_09"], 13, None),
    ev("mawlek_spit_b", "sharedassets34.assets", 91, "mawlek_spit_b", 5512, &["scene", "Crossroads_09"], 14, None),
    // `Bench Control` `Start Rest`.
    ev("bench_rest", "sharedassets7.assets", 105, "bench_rest", 5512, &["benches"], 20, None),
    // The one-way reveal controllers' sound branch (`unmasker`), and the second
    // clip of a hidden wall's `Break`. 3000 Hz: the chime holds -37.9 dB of its
    // energy above 1.5 kHz, so it loses almost nothing against 5512 Hz and a
    // -40 dB tail (3.68 of 4.03 s) and costs 6,336 bytes instead of 12,720,
    // which is what lets it into King's Pass beside the chest and the shiny.
    // Admitted after them (24): placed first, it took the gap the shiny needs.
    ev("secret_discovered", "sharedassets6.assets", 157, "secret_discovered_temp", 3000, &["secrets"], 24, None),
    // Hidden walls and cracked floors (host/secret_breaks.py), admitted after
    // everything a scene already had so no earlier sound loses its place. A
    // wall's `AudioPlayRandom` picks breakable_wall_hit_1 or _2 at 1:1; _1 is
    // the resident door clip (data/sfx.rs), so only _2 rides the scene bank,
    // and a wall whose scene has no room for it plays _1 every time. `Break`
    // plays breakable_wall_death; a floor's hits and break play barrel_death_1.
    // Energy above the new Nyquist (source WAV): barrel_death_1 -9.6 dB at
    // 8000 Hz, breakable_wall_hit_2 -11.8 dB at 8000, breakable_wall_death
    // -17.5 dB at 4000 (-16.1 at 5512, so the lower rate costs little).
    ev("barrel_death_1", "sharedassets6.assets", 107, "barrel_death_1", 8000, &["secret_floors"], 40, None),
    ev("breakable_wall_death", "sharedassets6.assets", 102, "breakable_wall_death", 4000, &["secret_breaks"], 41, None),
    ev("breakable_wall_hit_2", "sharedassets6.assets", 99, "breakable_wall_hit_2", 8000, &["secret_walls"], 42, None),
    // Soul totem `Hit` (soul_totem, mini_soul_totem), where a totem stands.
    ev("soul_totem_slash", "sharedassets56.assets", 14, "soul_totem_slash", 11025, &["totems"], 25, None),
    // `Chest Control` `Open`: the lid's clip (its second, barrel_death_2, would
    // cut the first on the one scene voice).
    // `Shiny Control`'s Flash/Trink Flash/Big Get Flash all play one pickup
    // clip; heart and vessel pieces play theirs on `Get`. The chest and shiny
    // clips stay at 11025 Hz: at 8000 the chest's loses 3.1 dB more of its
    // energy above Nyquist. The piece clip loses only 0.4 dB more at 8000 Hz
    // (-10.6 to -10.2) and its -40 dB tail is -50 dB of its energy, so it is
    // 8000 Hz and trimmed: 5,424 bytes instead of 11,456, which fits the
    // 6,944-byte gap Brooding Mawlek's clips leave in Crossroads_09 for its
    // arena mask shard. Crossroads_10 has no room left after the False
    // Knight's clips, so its chest and the City Crest stay silent.
    ev("chest_open", "sharedassets6.assets", 158, "chest_open", 11025, &["chests"], 22, None),
    ev("shiny_item_pickup", "resources.assets", 1337, "shiny_item_pickup", 11025, &["shinies"], 23, None),
    ev("heartpiece_collect", "sharedassets10.assets", 31, "heartpiece_collect", 8000, &["pieces"], 23, None),
    // Aspid Hunter `spitter` Fire.
    ev("aspid_spit", "sharedassets32.assets", 118, "spitter_spit", 22050, &["family", "Spitter"], 30, None),
    // Gruz Mother's (Crossroads_04), after its arena's two gate sounds
    // (LATE_IN, priorities 60 and 61). Crossroads_04's own gaps (16,656 bytes)
    // are full with the bench, secret and wall clips this scene had; the room
    // for these is the inaudible Rain Indoor stem the scene no longer keeps
    // resident (host/ambience.py UNLOADED_STEMS). They are still admitted after
    // every earlier clip (priority 62 on) and never displace one: a clip that
    // does not fit is refused and its play is a counted miss. Appended last so
    // every earlier event keeps its index.
    // The boss_* clips are the Crossroads_09/10 rows' clips under their own
    // event names, so those rows keep their priorities in their scenes.
    // The flying, charge and snore loops are AudioSource loops, which the one
    // scene voice does not hold.
    ev("big_fly_wall_hit", "sharedassets32.assets", 66, "big_fly_wall_hit", 5512, &["scene", "Crossroads_04"], 62, None),
    ev("big_fly_snore_startle", "sharedassets32.assets", 78, "big_fly_snore_startle", 5512, &["scene", "Crossroads_04"], 63, None),
    ev("gruz_final_hit", "sharedassets32.assets", 135, "boss_final_hit", 5512, &["scene", "Crossroads_04"], 64, None),
    ev("gruz_explode", "sharedassets32.assets", 99, "boss_explode", 3000, &["scene", "Crossroads_04"], 65, None),
    ev("gruz_gushing", "sharedassets32.assets", 62, "boss_gushing", 3000, &["scene", "Crossroads_04"], 66, None),
    ev("big_fly_stomache_problems_1", "sharedassets40.assets", 25, "big_fly_stomache_problems_1", 4000, &["scene", "Crossroads_04"], 67, None),
    ev("big_fly_stomache_problems_2", "sharedassets40.assets", 26, "big_fly_stomache_problems_2", 4000, &["scene", "Crossroads_04"], 68, None),
    ev("big_fly_stomache_problems_final_and_explode", "sharedassets40.assets", 29, "big_fly_stomache_problems_final_and_explode", 4000, &["scene", "Crossroads_04"], 69, None),
    // StalactiteControl (Tutorial_01 and six Crossroads scenes): the up-slash
    // break (`breakSound`), a side or down hit (`hitSound`) and the fall
    // starting (`startFallSound`), in that order of priority. Late rows, so
    // they only take what every earlier clip leaves and never move one. 5512 Hz
    // like the bank's other short percussive clips, not measured per clip.
    ev("stalactite_death", "sharedassets6.assets", 112, "stalactite_death", 5512, &["stalactites"], 55, None),
    ev("stalactite_impact", "sharedassets6.assets", 95, "stalactite_impact", 5512, &["stalactites"], 56, None),
    ev("stalactite_break", "sharedassets6.assets", 161, "stalactite_break", 5512, &["stalactites"], 57, None),
];

/// Gruz Mother's arena gates sound like every other arena's.
const LATE_IN: [(&str, &str, i64); 2] = [("gate_slam", "Crossroads_04", 60), ("gate_open", "Crossroads_04", 61)];

// ---------------------------------------------------------------- numbers

/// Python's `round(x, digits)`: the correctly rounded decimal, read back.
fn round_to(x: f64, digits: usize) -> f64 {
    format!("{x:.digits$}").parse().unwrap()
}
/// Python's `a / b` for two ints, below 2**53 so both convert exactly.
fn div(a: i64, b: i64) -> f64 {
    a as f64 / b as f64
}
fn oneshot_bytes(samples: usize) -> usize {
    samples.div_ceil(28) * 16 + 16
}
fn candidate_rates(row: i64) -> Vec<i64> {
    std::iter::once(row).chain(LADDER.iter().copied().filter(|&r| r < row)).collect()
}
fn pitch_register(rate: i64) -> i64 {
    py_round(div(rate * 4096, 44100))
}
/// RMS level in dB of full scale.
fn level_db(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return -120.0;
    }
    let sum: i64 = samples.iter().map(|&x| x as i64 * x as i64).sum();
    let power = div(sum, samples.len() as i64);
    10.0 * (power.max(1e-12) / 1073741824.0).log10()
}
/// scene_bank.py / ambience.py `fnv`.
fn fnv(data: &[u8]) -> u32 {
    data.iter().fold(0x811c9dc5u32, |v, &b| (v ^ b as u32).wrapping_mul(0x01000193))
}

/// `trim_tail`: a clip's tail cut by `trim`, ending in a linear fade, with the
/// energy of what was dropped relative to the whole, in dB.
fn trim_tail(pcm: Vec<i16>, rate: i64, trim: Option<Trim>) -> (Vec<i16>, Option<f64>) {
    let n = pcm.len();
    let Some(trim) = trim.filter(|_| n > 0) else { return (pcm, None) };
    let keep = match trim {
        Trim::Db(value) => {
            let peak = pcm.iter().map(|&x| (x as i64).abs()).max().unwrap_or(0);
            let peak = if peak == 0 { 1 } else { peak };
            let threshold = peak as f64 * 10f64.powf(value as f64 / 20.0);
            let window = ((rate / 50) as usize).max(1);
            let mut keep = window;
            for start in (0..n).step_by(window) {
                let part = &pcm[start..(start + window).min(n)];
                let sum: i64 = part.iter().map(|&x| x as i64 * x as i64).sum();
                if div(sum, part.len() as i64).sqrt() >= threshold {
                    keep = start + part.len();
                }
            }
            keep
        }
        Trim::Seconds(s) => py_round(s * rate as f64) as usize,
    };
    let keep = keep.min(n);
    let total: i64 = pcm.iter().map(|&x| x as i64 * x as i64).sum();
    let total = if total == 0 { 1 } else { total };
    let dropped: i64 = pcm[keep..].iter().map(|&x| x as i64 * x as i64).sum();
    let fade = keep.min(py_round(TRIM_FADE_SECONDS * rate as f64) as usize);
    let mut out = pcm[..keep].to_vec();
    for i in 0..fade {
        let at = keep - fade + i;
        out[at] = py_round(div(out[at] as i64 * (fade - i) as i64, fade as i64)) as i16;
    }
    let db = (dropped != 0).then(|| round_to(10.0 * div(dropped, total).log10(), 1));
    (out, db)
}

/// The predictor-zero one-shot encoder every scene bank shipped with up to
/// ship6 (host/cook_audio.py before the SDK encoder), kept only as the
/// baseline `fit`'s no-worse check measures against.
fn ship6_encode(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    for start in (0..samples.len()).step_by(28) {
        let mut block: Vec<i64> = samples[start..(start + 28).min(samples.len())].iter().map(|&x| x as i64).collect();
        block.resize(28, 0);
        let peak = block.iter().map(|x| x.abs()).max().unwrap();
        let mut shift = 0;
        while shift < 12 && peak <= (7 * 4096) >> (shift + 1) {
            shift += 1;
        }
        let unit = 4096 >> shift;
        let n: Vec<u8> = block.iter().map(|&x| (py_round(div(x, unit)).clamp(-8, 7) & 15) as u8).collect();
        out.extend_from_slice(&[shift as u8, 0]);
        out.extend((0..28).step_by(2).map(|i| n[i] | (n[i + 1] << 4)));
    }
    out.extend_from_slice(&[12, 1]);
    out.extend_from_slice(&[0; 14]);
    out
}

/// cook_audio.py `decode_oneshot`'s framing check: no flags but the silent
/// END terminator.
fn check_oneshot(bank: &[u8]) -> Result<()> {
    if bank.is_empty() || !bank.len().is_multiple_of(16) {
        return err("ADPCM block alignment");
    }
    for start in (0..bank.len()).step_by(16) {
        let (header, flags) = (bank[start], bank[start + 1]);
        if header >> 4 > 4 || header & 15 > 12 || flags != if start + 16 == bank.len() { 1 } else { 0 } {
            return err("unsupported predictor/shift or unsafe loop flags");
        }
    }
    if bank[bank.len() - 14..].iter().any(|&b| b != 0) {
        return err("one-shot terminator must be silent");
    }
    Ok(())
}

// ---------------------------------------------------------------- WAV

struct Wav {
    channels: u16,
    rate: u32,
    width: u16,
    data: Vec<u8>,
}
/// What Python's `wave` module reads: the fmt chunk and the data chunk.
fn read_wav(bytes: &[u8]) -> Result<Wav> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return err("not a RIFF WAVE");
    }
    let (mut at, mut fmt, mut data) = (12, None, None);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
        match id {
            b"fmt " => fmt = Some(body.to_vec()),
            b"data" => {
                data = Some(body.to_vec());
                break;
            }
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let (fmt, data) = (fmt.ok_or("WAV without fmt")?, data.ok_or("WAV without data")?);
    let u16_at = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
    Ok(Wav { channels: u16_at(2), rate: u32::from_le_bytes(fmt[4..8].try_into().unwrap()), width: u16_at(14) / 8, data })
}
/// Python `wave` writing 16-bit mono at `rate`.
fn mono_wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}
fn samples_of(data: &[u8]) -> Vec<i16> {
    data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

// ---------------------------------------------------------------- clips

#[derive(Clone)]
struct Meta {
    seconds: f64,
    kept_seconds: f64,
    trim: Option<Trim>,
    dropped_energy_db: Option<f64>,
}

/// The clip pipeline and its caches: source WAVs, resampled PCM and encoded
/// payloads, plus the psx-audio-cook command line.
struct Clips<'s> {
    root: PathBuf,
    source: &'s Source,
    scratch: PathBuf,
    binary: PathBuf,
    wavs: HashMap<(String, i64), Vec<u8>>,
    pcm: HashMap<String, (Vec<i16>, Meta)>,
    encoded: HashMap<String, Vec<u8>>,
    checked: HashMap<(String, i64), Check>,
}

#[derive(Clone)]
struct Check {
    pass: bool,
    ship6_fwsnrseg: f64,
    fwsnrseg: f64,
    ship6_level_off_db: f64,
    level_off_db: f64,
}

fn run(cmd: &mut Command, input: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut child = cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).spawn().map_err(|e| format!("{cmd:?}: {e}"))?;
    let writer = input.map(|data| {
        let mut stdin = child.stdin.take().unwrap();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(&data);
        })
    });
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if let Some(w) = writer {
        let _ = w.join();
    }
    if !out.status.success() {
        return err(format!("{cmd:?} failed with {}", out.status));
    }
    Ok(out.stdout)
}

/// What `assemble` and `fit` need of the clip pipeline: payloads and sizes at
/// any rate, and, when a scene may be fitted, the allocator and the no-worse
/// check (Python's `clip_of` and `Measure`).
trait Audio {
    fn payload(&mut self, e: &Event, rate: i64) -> Result<Vec<u8>>;
    fn size(&mut self, e: &Event, rate: i64) -> Result<usize>;
    fn allocate(&mut self, rows: &[(Event, Vec<i64>, Vec<usize>, usize)], budget: i64) -> Result<Option<Vec<usize>>>;
    fn check(&mut self, e: &Event, rate: i64) -> Result<Check>;
    /// False for a pipeline without a `Measure`: refusals stand.
    fn can_fit(&self) -> bool {
        true
    }
}

impl Audio for Clips<'_> {
    fn payload(&mut self, e: &Event, rate: i64) -> Result<Vec<u8>> {
        Clips::payload(self, e, rate)
    }
    fn size(&mut self, e: &Event, rate: i64) -> Result<usize> {
        Clips::size(self, e, rate)
    }
    fn allocate(&mut self, rows: &[(Event, Vec<i64>, Vec<usize>, usize)], budget: i64) -> Result<Option<Vec<usize>>> {
        Clips::allocate(self, rows, budget)
    }
    fn check(&mut self, e: &Event, rate: i64) -> Result<Check> {
        Clips::check(self, e, rate)
    }
}

impl<'s> Clips<'s> {
    fn new(root: &Path, source: &'s Source) -> Result<Self> {
        // spu_cook.py `binary`: the SDK's cooker built from the pinned tree.
        let crate_dir = root.join("tools/psx-audio-cook");
        let target = crate_dir.join("target");
        run(
            Command::new("cargo").args(["build", "-q", "--release", "--manifest-path"]).arg(crate_dir.join("Cargo.toml")).arg("--target-dir").arg(&target).current_dir(root),
            None,
        )?;
        let scratch = std::env::temp_dir().join(format!("hk-scene-sfx-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        Ok(Self {
            root: root.to_path_buf(),
            source,
            scratch,
            binary: target.join("release/psx-audio-cook"),
            wavs: HashMap::new(),
            pcm: HashMap::new(),
            encoded: HashMap::new(),
            checked: HashMap::new(),
        })
    }

    /// `source_wav`: a row's source clip as WAV bytes, checked against its name.
    fn source_wav(&mut self, e: &Event) -> Result<Vec<u8>> {
        let key = (e.file.to_string(), e.path_id);
        if let Some(w) = self.wavs.get(&key) {
            return Ok(w.clone());
        }
        let file = self.source.file(e.file).map_err(|x| x.to_string())?;
        let obj: Obj = self.source.object(&file, e.path_id).map_err(|x| x.to_string())?;
        let t = self.source.read(&obj).map_err(|x| x.to_string())?;
        let name = t.get("m_Name").and_then(Value::str).unwrap_or_default();
        if name != e.clip {
            return err(format!("changed scene sound mapping: {}:{} is {name}, not {}", e.file, e.path_id, e.clip));
        }
        let data = match t.get("m_AudioData") {
            Some(Value::Bytes(b)) if !b.is_empty() => b.clone(),
            _ => {
                let r = t.get("m_Resource").ok_or("AudioClip with neither m_AudioData nor m_Resource")?;
                let path = r.get("m_Source").and_then(Value::str).unwrap_or_default();
                let base = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                let offset = r.get("m_Offset").and_then(Value::int).unwrap_or(0) as usize;
                let size = r.get("m_Size").and_then(Value::int).unwrap_or(0) as usize;
                let bytes = self.source.resource(&self.source.directory.join(&base)).map_err(|x| x.to_string())?;
                bytes.get(offset..offset + size).ok_or("audio resource out of range")?.to_vec()
            }
        };
        let wav = if data.starts_with(b"RIFF") {
            data
        } else {
            let channels = t.get("m_Channels").and_then(Value::int).filter(|&c| c != 0).unwrap_or(2) as i32;
            let frequency = t.get("m_Frequency").and_then(Value::int).filter(|&c| c != 0).unwrap_or(44100) as i32;
            crate::fmod::raw_to_wav(&self.root, &data, channels, frequency)?
        };
        self.wavs.insert(key, wav.clone());
        Ok(wav)
    }

    /// `resampled`: a clip at `rate`, folded to mono by ffmpeg's resampler
    /// (cook_audio.py `convert_wav`) and trimmed, with its meta.
    fn resampled(&mut self, e: &Event, rate: i64) -> Result<(Vec<i16>, Meta)> {
        let key = format!("{}:{}:{rate}:{:?}", e.file, e.path_id, e.trim);
        if let Some(x) = self.pcm.get(&key) {
            return Ok(x.clone());
        }
        let data = self.source_wav(e)?;
        let wav = read_wav(&data)?;
        if wav.width != 2 || !(wav.channels == 1 || wav.channels == 2) {
            return err("unvalidated source PCM format or rate conversion");
        }
        let frames = wav.data.len() / (2 * wav.channels as usize);
        let pcm = if frames == 0 {
            Vec::new()
        } else {
            let out = run(
                Command::new("ffmpeg").args(["-v", "error", "-i", "pipe:0", "-ar", &rate.to_string(), "-ac", "1", "-f", "s16le", "pipe:1"]),
                Some(data.clone()),
            )?;
            let pcm = samples_of(&out);
            let expected = py_round(div(frames as i64 * rate, wav.rate as i64));
            if (pcm.len() as i64 - expected).abs() > 1 {
                return err(format!("resampled length {} is not the expected {expected}", pcm.len()));
            }
            pcm
        };
        let full = pcm.len();
        let (pcm, dropped) = trim_tail(pcm, rate, e.trim);
        let meta = Meta {
            seconds: round_to(div(full as i64, rate), 2),
            kept_seconds: round_to(div(pcm.len() as i64, rate), 2),
            trim: e.trim,
            dropped_energy_db: dropped,
        };
        self.pcm.insert(key, (pcm.clone(), meta.clone()));
        Ok((pcm, meta))
    }

    /// `encoded`: one clip through the SDK encoder, as a one-shot.
    fn payload(&mut self, e: &Event, rate: i64) -> Result<Vec<u8>> {
        let key = format!("{}:{}:{rate}:{:?}", e.file, e.path_id, e.trim);
        if let Some(x) = self.encoded.get(&key) {
            return Ok(x.clone());
        }
        let (pcm, _) = self.resampled(e, rate)?;
        let mut out = if pcm.is_empty() {
            Vec::new()
        } else {
            let (input, output) = (self.scratch.join("in.wav"), self.scratch.join("out.adpcm"));
            std::fs::write(&input, mono_wav(NOMINAL_RATE, &pcm)).map_err(|x| x.to_string())?;
            run(
                Command::new(&self.binary)
                    .arg("encode")
                    .arg(&input)
                    .arg(&output)
                    .args(["--rate", &NOMINAL_RATE.to_string(), "--format", "raw", "--no-normalize", "--no-flags", "--loop", "none"]),
                None,
            )?;
            let data = std::fs::read(&output).map_err(|x| x.to_string())?;
            if data.len() != pcm.len().div_ceil(28) * 16 {
                return err(format!("encoded {} bytes for {} samples", data.len(), pcm.len()));
            }
            data
        };
        out.extend_from_slice(&[12, 1]);
        out.extend_from_slice(&[0; 14]);
        check_oneshot(&out)?;
        self.encoded.insert(key, out.clone());
        Ok(out)
    }

    fn size(&mut self, e: &Event, rate: i64) -> Result<usize> {
        Ok(oneshot_bytes(self.resampled(e, rate)?.0.len()))
    }

    /// `Measure.wav`: the row's source WAV in the scratch directory.
    fn wav_path(&mut self, e: &Event) -> Result<PathBuf> {
        let path = self.scratch.join(format!("{}.wav", e.name));
        if !path.exists() {
            let data = self.source_wav(e)?;
            std::fs::write(&path, data).map_err(|x| x.to_string())?;
        }
        Ok(path)
    }

    /// `Measure.score`: fwSNRseg of `payload` played at `rate` against the
    /// source, and the level offset of that playback from the band-limited source.
    fn score(&mut self, e: &Event, payload: &[u8], rate: i64) -> Result<(f64, f64)> {
        let (path, play, original) = (self.scratch.join("score.adpcm"), self.scratch.join("play.wav"), self.scratch.join("original.wav"));
        std::fs::write(&path, payload).map_err(|x| x.to_string())?;
        let wav = self.wav_path(e)?;
        let out = run(
            Command::new(&self.binary).arg("score").arg(&wav).arg(&path).args(["--rate", &rate.to_string(), "--play"]).arg(&play).arg("--original").arg(&original),
            None,
        )?;
        let parsed: J = serde_json::from_slice(&out).map_err(|x| x.to_string())?;
        let fw = parsed["fwsnrseg"].as_f64().ok_or("score without fwsnrseg")?;
        let level = |p: &Path| -> Result<f64> {
            let w = read_wav(&std::fs::read(p).map_err(|x| x.to_string())?)?;
            Ok(level_db(&samples_of(&w.data)))
        };
        Ok((fw, level(&play)? - level(&original)?))
    }

    /// `Measure.allocate`: steps from the SDK allocator, or None when the
    /// rows' floors do not fit `budget`.
    fn allocate(&mut self, rows: &[(Event, Vec<i64>, Vec<usize>, usize)], budget: i64) -> Result<Option<Vec<usize>>> {
        let mut lines = vec![format!("budget\t{budget}")];
        for (e, rates, sizes, max_step) in rows {
            let len = self.resampled(e, e.rate)?.0.len();
            let weight = div(len as i64, e.rate).max(1e-3);
            let wav = self.wav_path(e)?;
            lines.push(format!(
                "{weight:.4}\t{max_step}\t{}\t{}\t{}",
                wav.display(),
                rates.iter().map(i64::to_string).collect::<Vec<_>>().join(","),
                sizes.iter().map(usize::to_string).collect::<Vec<_>>().join(",")
            ));
        }
        let request = self.scratch.join("plan.txt");
        std::fs::write(&request, lines.join("\n") + "\n").map_err(|x| x.to_string())?;
        let out = String::from_utf8(run(Command::new(&self.binary).arg("plan").arg(&request), None)?).map_err(|x| x.to_string())?;
        let out: Vec<&str> = out.split('\n').collect();
        if out[0].trim() == "none" {
            return Ok(None);
        }
        Ok(Some(out.iter().filter(|l| !l.trim().is_empty()).map(|l| l.split('\t').next().unwrap().parse().unwrap()).collect()))
    }

    /// `Measure.check`: whether this cook at `rate` is no worse than ship6's
    /// at the row rate.
    fn check(&mut self, e: &Event, rate: i64) -> Result<Check> {
        let key = (e.name.to_string(), rate);
        if let Some(c) = self.checked.get(&key) {
            return Ok(c.clone());
        }
        let row = e.rate;
        let base_pcm = self.resampled(e, row)?.0;
        let (base_score, base_off) = self.score(e, &ship6_encode(&base_pcm), row)?;
        let payload = self.payload(e, rate)?;
        let (new_score, new_off) = self.score(e, &payload, rate)?;
        let c = Check {
            pass: new_score >= base_score && new_off.abs() <= base_off.abs().max(LEVEL_TOLERANCE_DB),
            ship6_fwsnrseg: base_score,
            fwsnrseg: new_score,
            ship6_level_off_db: round_to(base_off, 2),
            level_off_db: round_to(new_off, 2),
        };
        self.checked.insert(key, c.clone());
        Ok(c)
    }
}

impl Drop for Clips<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

// ---------------------------------------------------------------- planning

fn jstr(v: &J, k: &str) -> String {
    v.get(k).and_then(J::as_str).unwrap_or_default().to_string()
}
fn read_json(path: &Path) -> Result<J> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| e.to_string())
}

/// `scene_needs`: per scene id, the events its content asks for.
fn scene_needs(root: &Path, report: &J, gates: &[J], scenes: &[J]) -> Result<BTreeMap<i64, Vec<Event>>> {
    let by_name: HashMap<String, i64> = scenes.iter().map(|s| (jstr(s, "scene_name"), s["scene_id"].as_i64().unwrap_or(0))).collect();
    let regions = report["regions"].as_array().ok_or("no regions")?;
    let scene_of = |r: &J| r["scene_id"].as_i64().unwrap_or(0);
    let secrets_of = |r: &J| r["secrets"].as_array().cloned().unwrap_or_default();
    let benches: HashSet<i64> = regions.iter().filter(|r| r["benches"].as_array().is_some_and(|b| !b.is_empty())).map(scene_of).collect();
    let family_in = |r: &J, set: &[i64]| secrets_of(r).iter().any(|x| set.contains(&x["family"].as_i64().unwrap_or(0)));
    let walls: HashSet<i64> = regions.iter().filter(|r| family_in(r, &[1, 2])).map(scene_of).collect();
    let floors: HashSet<i64> = regions.iter().filter(|r| family_in(r, &[3, 4])).map(scene_of).collect();
    // A hidden wall's `Break` plays the reveal chime too.
    let mut secrets: HashSet<i64> = walls.clone();
    for (k, v) in report["reveal_mask_scenes"].as_object().ok_or("no reveal_mask_scenes")? {
        if v["controllers"].as_array().into_iter().flatten().any(|c| c.get("plays_sound").is_some_and(|p| p.as_bool().unwrap_or(false) || p.as_i64().unwrap_or(0) != 0)) {
            secrets.insert(k.parse().map_err(|_| "reveal mask scene id")?);
        }
    }
    let mut families: HashMap<String, HashSet<i64>> = HashMap::new();
    for region in regions {
        for actor in region["actors"].as_array().into_iter().flatten() {
            if actor.get("movement_supported").is_some_and(|m| m.as_bool().unwrap_or(false)) {
                let name = jstr(actor, "name");
                families.entry(name.split(' ').next().unwrap_or("").to_string()).or_default().insert(scene_of(region));
            }
        }
    }
    let stalactites: HashSet<i64> = regions
        .iter()
        .filter(|r| r["hazards"].as_array().into_iter().flatten().any(|h| jstr(h, "name").starts_with("Stalactite")))
        .map(scene_of)
        .collect();
    let arena_scenes: HashSet<i64> = gates.iter().filter_map(|g| g["scene"].as_i64()).collect();
    let totems_path = root.join(".hkpsx/soul-totems.json");
    let totems: HashSet<i64> = if totems_path.is_file() {
        read_json(&totems_path)?["totems"].as_array().into_iter().flatten().filter_map(|t| t["scene"].as_i64()).collect()
    } else {
        HashSet::new()
    };
    // host/pickups.py runs first and says which scenes hold what.
    let pickups_path = root.join(".hkpsx/pickups.json");
    let placed = if pickups_path.is_file() { read_json(&pickups_path)? } else { J::Object(Default::default()) };
    let (mut chests, mut shinies, mut pieces) = (HashSet::new(), HashSet::new(), HashSet::new());
    for (k, v) in placed.as_object().into_iter().flatten() {
        let Some(&s) = by_name.get(k) else { continue };
        if v["chests"].as_array().is_some_and(|c| !c.is_empty()) {
            chests.insert(s);
        }
        let pickups = v["pickups"].as_array().cloned().unwrap_or_default();
        if pickups.iter().any(|p| !p["touch"].as_bool().unwrap_or(false)) {
            shinies.insert(s);
        }
        if pickups.iter().any(|p| p["touch"].as_bool().unwrap_or(false)) {
            pieces.insert(s);
        }
    }
    let mut needs: BTreeMap<i64, Vec<Event>> = scenes.iter().map(|s| (s["scene_id"].as_i64().unwrap_or(0), Vec::new())).collect();
    for event in EVENTS {
        let kind = event.place;
        let named = || -> Result<HashSet<i64>> { kind[1..].iter().map(|n| by_name.get(*n).copied().ok_or_else(|| format!("no scene {n}"))).collect() };
        let ids: HashSet<i64> = match kind[0] {
            "scene" => named()?,
            "arena" => named()?.intersection(&arena_scenes).copied().collect(),
            "benches" => benches.clone(),
            "secrets" => secrets.clone(),
            "secret_walls" => walls.clone(),
            "secret_breaks" => walls.union(&floors).copied().collect(),
            "secret_floors" => floors.clone(),
            "totems" => totems.clone(),
            "chests" => chests.clone(),
            "shinies" => shinies.clone(),
            "pieces" => pieces.clone(),
            "stalactites" => stalactites.clone(),
            "family" => families.get(kind[1]).cloned().unwrap_or_default(),
            other => return err(format!("unknown scene sound selector {other}")),
        };
        for scene in ids {
            if let Some(list) = needs.get_mut(&scene) {
                list.push(event.clone());
            }
        }
    }
    // A shared row a later arena needs as well, at a late priority there so
    // that scene's existing clips keep their fit (see LATE_PRIORITY).
    for event in EVENTS {
        for &(name, scene_name, priority) in &LATE_IN {
            if name != event.name {
                continue;
            }
            let scene = *by_name.get(scene_name).ok_or("late scene")?;
            if let Some(list) = needs.get_mut(&scene) {
                if !list.contains(event) {
                    list.push(Event { priority, ..event.clone() });
                }
            }
        }
    }
    Ok(needs)
}

/// `free_gaps`: SPU gaps between ambience's start and the ring that no stem
/// able to be resident in `scene` occupies. With `unloaded` false, the bytes of
/// a stem the scene does not keep resident stay taken.
fn free_gaps(ambience: &J, scene: i64, unloaded: bool) -> Result<Vec<(i64, i64)>> {
    let cue = ambience["cues"].as_array().into_iter().flatten().find(|c| c["scene"].as_i64() == Some(scene)).ok_or("no cue")?;
    let g = |k: &str| cue.get(k).and_then(J::as_i64).unwrap_or(0);
    let mask = g("mask") | g("prefetch") | if unloaded { 0 } else { g("unloaded") };
    let clips = ambience["clips"].as_array().ok_or("no clips")?;
    let mut taken: Vec<(i64, i64)> = clips
        .iter()
        .enumerate()
        .filter(|(i, _)| mask >> i & 1 != 0)
        .map(|(_, c)| {
            let a = c["spu_address"].as_i64().unwrap_or(0);
            (a, a + c["spu_bytes"].as_i64().unwrap_or(0))
        })
        .collect();
    taken.sort();
    let (mut gaps, mut at) = (Vec::new(), ambience["spu_start"].as_i64().ok_or("spu_start")?);
    for (lo, hi) in taken {
        if lo > at {
            gaps.push((at, lo));
        }
        at = at.max(hi);
    }
    let ceiling = ambience["spu_ceiling"].as_i64().ok_or("spu_ceiling")?;
    if ceiling > at {
        gaps.push((at, ceiling));
    }
    Ok(gaps)
}

/// `pack`: first-fit `events` in the order given into a copy of `gaps`.
type Placed = Vec<(Event, i64)>;
type Gaps = Vec<(i64, i64)>;
/// (rate per event name, refused events, floored clips that moved).
type Fitted = (Vec<(String, i64)>, Vec<Event>, Moved);
fn pack(events: &[Event], gaps: &[(i64, i64)], size_of: &mut dyn FnMut(&Event) -> Result<i64>) -> Result<(Placed, Vec<Event>, Gaps)> {
    let mut gaps = gaps.to_vec();
    let (mut placed, mut refused) = (Vec::new(), Vec::new());
    for e in events {
        let size = size_of(e)?;
        match gaps.iter().position(|(lo, hi)| hi - lo >= size) {
            Some(n) => {
                placed.push((e.clone(), gaps[n].0));
                gaps[n].0 += size;
            }
            None => refused.push(e.clone()),
        }
    }
    Ok((placed, refused, gaps))
}

/// `subtract`: `gaps` less the (lo, hi) ranges in `taken`.
fn subtract(gaps: &[(i64, i64)], taken: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for &(lo, hi) in gaps {
        let mut cuts: Vec<(i64, i64)> = taken.iter().filter(|&&(a, b)| a < hi && lo < b).map(|&(a, b)| (lo.max(a), hi.min(b))).collect();
        cuts.sort();
        let mut at = lo;
        for (a, b) in cuts {
            if a > at {
                out.push((at, a));
            }
            at = at.max(b);
        }
        if hi > at {
            out.push((at, hi));
        }
    }
    out
}

type Moved = Vec<(Event, i64, Check)>;

/// `fit`: rates for a scene whose row rates refuse something, the SDK
/// allocator's choice under the no-worse floors, placed first-fit by priority
/// or else largest first.
fn fit(events: &[Event], gaps: &[(i64, i64)], refused_names: &HashSet<&str>, clips: &mut dyn Audio) -> Result<Fitted> {
    let budget: i64 = gaps.iter().map(|(lo, hi)| hi - lo).sum();
    let mut events = events.to_vec();
    events.sort_by_key(|e| e.priority);
    let mut dropped = Vec::new();
    let mut floors: HashMap<&'static str, Option<usize>> = events.iter().map(|e| (e.name, None)).collect();
    while !events.is_empty() {
        let mut options: HashMap<&str, Vec<i64>> = HashMap::new();
        let mut sizes: HashMap<&str, Vec<usize>> = HashMap::new();
        for e in &events {
            let rates = candidate_rates(e.rate);
            let s = rates.iter().map(|&r| clips.size(e, r)).collect::<Result<Vec<_>>>()?;
            options.insert(e.name, rates);
            sizes.insert(e.name, s);
        }
        let mut limit = budget;
        while limit > 0 {
            let rows: Vec<(Event, Vec<i64>, Vec<usize>, usize)> = events
                .iter()
                .map(|e| (e.clone(), options[e.name].clone(), sizes[e.name].clone(), floors[e.name].unwrap_or(options[e.name].len() - 1)))
                .collect();
            let Some(steps) = clips.allocate(&rows, limit)? else { break };
            let chosen: Vec<(String, i64)> = events.iter().zip(&steps).map(|(e, &s)| (e.name.to_string(), options[e.name][s])).collect();
            let rate_of = |name: &str| chosen.iter().find(|c| c.0 == name).unwrap().1;
            // A clip the scene held at its row rate may only move down as far
            // as it still beats ship6's cook; tighten and ask again.
            let mut failed = false;
            for (e, &s) in events.iter().zip(&steps) {
                if s != 0 && !refused_names.contains(e.name) && !clips.check(e, rate_of(e.name))?.pass {
                    floors.insert(e.name, Some(s - 1));
                    failed = true;
                }
            }
            if failed {
                continue;
            }
            let size_of = |e: &Event| -> i64 {
                let o = &options[e.name];
                sizes[e.name][o.iter().position(|&r| r == rate_of(e.name)).unwrap()] as i64
            };
            let mut largest = events.clone();
            largest.sort_by_key(|e| -size_of(e));
            for order in [events.clone(), largest] {
                let (_, left, _) = pack(&order, gaps, &mut |e| Ok(size_of(e)))?;
                if left.is_empty() {
                    let mut moved = Vec::new();
                    for e in &events {
                        if !refused_names.contains(e.name) && rate_of(e.name) != e.rate {
                            moved.push((e.clone(), rate_of(e.name), clips.check(e, rate_of(e.name))?));
                        }
                    }
                    return Ok((chosen, dropped, moved));
                }
            }
            // The total fits but the gaps' shapes do not: ask for less.
            limit -= 256;
        }
        // Even the floors do not fit: refuse the lowest-priority clip whole.
        dropped.push(events.pop().unwrap());
    }
    Ok((Vec::new(), dropped, Vec::new()))
}

struct Entry {
    event: &'static str,
    spu_address: i64,
    offset: usize,
    bytes: usize,
    rate: i64,
}
struct Bank {
    chunk: Vec<u8>,
    entries: Vec<Entry>,
    refused: Vec<(&'static str, usize, i64)>,
    free_before: i64,
    free_after: i64,
    fitted: Option<(Vec<(String, i64)>, Moved)>,
}

fn set_rate(rates: &mut Vec<(String, i64)>, name: &str, rate: i64) {
    match rates.iter_mut().find(|r| r.0 == name) {
        Some(slot) => slot.1 = rate,
        None => rates.push((name.to_string(), rate)),
    }
}
fn rate_in(rates: &[(String, i64)], name: &str) -> Option<i64> {
    rates.iter().find(|r| r.0 == name).map(|r| r.1)
}

/// `assemble`: per scene, its chunk bytes, admitted entries and refusals.
fn assemble(needs: &BTreeMap<i64, Vec<Event>>, ambience: &J, clips: &mut dyn Audio) -> Result<BTreeMap<i64, Bank>> {
    let mut banks = BTreeMap::new();
    for (&scene, events) in needs {
        let mut late: Vec<Event> = events.iter().filter(|e| e.priority >= LATE_PRIORITY).cloned().collect();
        late.sort_by_key(|e| e.priority);
        let events: Vec<Event> = events.iter().filter(|e| e.priority < LATE_PRIORITY).cloned().collect();
        let gaps = free_gaps(ambience, scene, false)?;
        let all_gaps = free_gaps(ambience, scene, true)?;
        let free: i64 = gaps.iter().map(|(lo, hi)| hi - lo).sum();
        // Largest-first inside one priority would pack better, but priority is
        // the contract: a lower-priority clip never displaces a higher one.
        let mut ordered = events.clone();
        ordered.sort_by_key(|e| e.priority);
        let mut rates: Vec<(String, i64)> = Vec::new();
        for e in &ordered {
            set_rate(&mut rates, e.name, e.rate);
        }
        let size_at = |clips: &mut dyn Audio, rates: &[(String, i64)], e: &Event| -> Result<i64> { Ok(clips.payload(e, rate_in(rates, e.name).unwrap())?.len() as i64) };
        let (mut placed, mut refused, mut left) = pack(&ordered, &gaps, &mut |e| size_at(clips, &rates, e))?;
        let mut fitted = None;
        if !refused.is_empty() && clips.can_fit() {
            let names: HashSet<&str> = refused.iter().map(|e| e.name).collect();
            let (chosen, dropped, moved) = fit(&ordered, &gaps, &names, clips)?;
            let moved = if !chosen.is_empty() {
                for (n, r) in &chosen {
                    set_rate(&mut rates, n, *r);
                }
                let kept: Vec<Event> = ordered.iter().filter(|e| chosen.iter().any(|c| c.0 == e.name)).cloned().collect();
                let (p, _, l) = pack(&kept, &gaps, &mut |e| size_at(clips, &rates, e))?;
                (placed, left) = (p, l);
                if placed.len() != kept.len() {
                    let mut sized: Vec<(i64, Event)> = Vec::new();
                    for e in &kept {
                        sized.push((size_at(clips, &rates, e)?, e.clone()));
                    }
                    sized.sort_by_key(|(s, _)| -s);
                    let largest: Vec<Event> = sized.into_iter().map(|(_, e)| e).collect();
                    let (p, _, l) = pack(&largest, &gaps, &mut |e| size_at(clips, &rates, e))?;
                    (placed, left) = (p, l);
                }
                if placed.len() != kept.len() {
                    return err("fit placed what pack cannot");
                }
                refused = dropped;
                moved
            } else {
                // Nothing fits even alone: keep the plain priority packing.
                Vec::new()
            };
            let fitted_rates: Vec<(String, i64)> =
                ordered.iter().filter(|e| rate_in(&rates, e.name).is_some() && !refused.contains(e)).map(|e| (e.name.to_string(), rate_in(&rates, e.name).unwrap())).collect();
            fitted = Some((fitted_rates, moved));
        }
        if !late.is_empty() {
            let mut taken = Vec::new();
            for (e, a) in &placed {
                taken.push((*a, a + size_at(clips, &rates, e)?));
            }
            let late_gaps = subtract(&all_gaps, &taken);
            for e in &late {
                set_rate(&mut rates, e.name, e.rate);
            }
            let (mut got, mut late_refused, _) = pack(&late, &late_gaps, &mut |e| size_at(clips, &rates, e))?;
            if !late_refused.is_empty() && clips.can_fit() {
                let names: HashSet<&str> = late.iter().map(|e| e.name).collect();
                let (chosen, dropped, _) = fit(&late, &late_gaps, &names, clips)?;
                if !chosen.is_empty() {
                    for (n, r) in &chosen {
                        set_rate(&mut rates, n, *r);
                    }
                    let kept: Vec<Event> = late.iter().filter(|e| chosen.iter().any(|c| c.0 == e.name)).cloned().collect();
                    got = pack(&kept, &late_gaps, &mut |e| size_at(clips, &rates, e))?.0;
                    if got.len() != kept.len() {
                        let mut sized: Vec<(i64, Event)> = Vec::new();
                        for e in &kept {
                            sized.push((size_at(clips, &rates, e)?, e.clone()));
                        }
                        sized.sort_by_key(|(s, _)| -s);
                        let largest: Vec<Event> = sized.into_iter().map(|(_, e)| e).collect();
                        got = pack(&largest, &late_gaps, &mut |e| size_at(clips, &rates, e))?.0;
                    }
                    if got.len() != kept.len() {
                        return err("fit placed what pack cannot");
                    }
                    late_refused = dropped;
                }
            }
            placed.extend(got);
            refused.extend(late_refused);
            let mut taken = Vec::new();
            for (e, a) in &placed {
                taken.push((*a, a + size_at(clips, &rates, e)?));
            }
            left = subtract(&all_gaps, &taken);
        }
        let (mut chunk, mut entries) = (Vec::new(), Vec::new());
        placed.sort_by_key(|(e, _)| e.priority);
        for (e, address) in &placed {
            let rate = rate_in(&rates, e.name).unwrap();
            let payload = clips.payload(e, rate)?;
            entries.push(Entry { event: e.name, spu_address: *address, offset: chunk.len(), bytes: payload.len(), rate });
            chunk.extend_from_slice(&payload);
        }
        let mut refused_rows = Vec::new();
        for e in &refused {
            refused_rows.push((e.name, clips.payload(e, e.rate)?.len(), e.rate));
        }
        banks.insert(
            scene,
            Bank {
                chunk: if chunk.is_empty() { EMPTY.to_vec() } else { chunk },
                entries,
                refused: refused_rows,
                free_before: free,
                free_after: left.iter().map(|(lo, hi)| hi - lo).sum(),
                fitted,
            },
        );
    }
    Ok(banks)
}

// ---------------------------------------------------------------- output

fn rust(banks: &BTreeMap<i64, Bank>) -> String {
    let mut lines = vec![
        "// Generated by host/scene_sfx.py: per-scene one-shot banks, read at the scene gate.".to_string(),
        format!("pub const EVENTS:usize={};", EVENTS.len()),
    ];
    for (index, e) in EVENTS.iter().enumerate() {
        lines.push(format!("pub const {}:u8={index};", e.name.to_uppercase()));
    }
    lines.push("/// (row rate, gain, row pitch register) per event; a fitted scene's entry".into());
    lines.push("/// carries the rate it was cooked at (ENTRIES).".into());
    lines.push(format!(
        "pub static EVENT_PARAMS:[(u32,i16,u16);{}]=[{}];",
        EVENTS.len(),
        EVENTS.iter().map(|e| format!("({},{GAIN},{}),", e.rate, pitch_register(e.rate))).collect::<String>()
    ));
    let (mut entries, mut rows) = (Vec::new(), Vec::new());
    for bank in banks.values() {
        rows.push(format!("({},{},{},{}),", bank.chunk.len(), fnv(&bank.chunk), entries.len(), bank.entries.len()));
        for e in &bank.entries {
            let index = EVENTS.iter().position(|x| x.name == e.event).unwrap();
            entries.push(format!("({index},{},{},{},{},{}),", e.spu_address, e.offset, e.bytes, e.rate, pitch_register(e.rate)));
        }
    }
    lines.push("/// Per guest scene id: (chunk bytes, FNV-1a, first entry, entry count).".into());
    lines.push(format!("pub static BANKS:[(u32,u32,u16,u8);{}]=[{}];", banks.len(), rows.concat()));
    lines.push("/// (event, SPU address, offset in the scene chunk, bytes, rate, pitch register).".into());
    lines.push(format!("pub static ENTRIES:&[(u8,u32,u32,u32,u32,u16)]=&[{}];", entries.concat()));
    lines.join("\n") + "\n"
}

fn float_or_null(v: Option<f64>) -> Json {
    v.map_or(Json::Null, Json::Float)
}
fn trim_json(t: Option<Trim>) -> Json {
    match t {
        None => Json::Null,
        Some(Trim::Db(v)) => Json::List(vec![Json::Str("db".into()), Json::Int(v)]),
        Some(Trim::Seconds(v)) => Json::List(vec![Json::Str("seconds".into()), Json::Float(v)]),
    }
}
fn check_fields(c: &Check) -> Vec<(String, Json)> {
    vec![
        ("pass".into(), Json::Bool(c.pass)),
        ("ship6_fwsnrseg".into(), Json::Float(c.ship6_fwsnrseg)),
        ("fwsnrseg".into(), Json::Float(c.fwsnrseg)),
        ("ship6_level_off_db".into(), Json::Float(c.ship6_level_off_db)),
        ("level_off_db".into(), Json::Float(c.level_off_db)),
    ]
}

/// The cook: writes data/scene-sfx/, data/scene_sfx.rs and .hkpsx/scene-sfx.json
/// under `root`, and returns the summary it prints.
pub fn cook(root: &Path, source: &Source) -> Result<String> {
    let ambience = read_json(&root.join(".hkpsx/ambience.json"))?;
    let gates = read_json(&root.join(".hkpsx/battle-gates.json"))?["gates"].as_array().cloned().unwrap_or_default();
    let report = read_json(&root.join("data/regions.json"))?;
    let scenes = report["scenes"].as_array().cloned().ok_or("no scenes")?;
    let needs = scene_needs(root, &report, &gates, &scenes)?;
    let mut clips = Clips::new(root, source)?;
    let banks = assemble(&needs, &ambience, &mut clips)?;
    let out = root.join("data/scene-sfx");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for (scene, bank) in &banks {
        std::fs::write(out.join(format!("scene_{scene}.adpcm")), &bank.chunk).map_err(|e| e.to_string())?;
    }
    std::fs::write(root.join("data/scene_sfx.rs"), rust(&banks)).map_err(|e| e.to_string())?;
    let mut events = Vec::new();
    for e in EVENTS {
        let bytes = clips.payload(e, e.rate)?.len();
        let (_, meta) = clips.resampled(e, e.rate)?;
        events.push(Json::Obj(vec![
            ("event".into(), Json::Str(e.name.into())),
            ("clip".into(), Json::Str(format!("{}:{}", e.file, e.path_id))),
            ("name".into(), Json::Str(e.clip.into())),
            ("rate".into(), Json::Int(e.rate)),
            ("where".into(), Json::List(e.place.iter().map(|w| Json::Str(w.to_string())).collect())),
            ("priority".into(), Json::Int(e.priority)),
            ("bytes".into(), Json::Int(bytes as i64)),
            ("seconds".into(), Json::Float(meta.seconds)),
            ("kept_seconds".into(), Json::Float(meta.kept_seconds)),
            ("trim".into(), trim_json(meta.trim)),
            ("dropped_energy_db".into(), float_or_null(meta.dropped_energy_db)),
        ]));
    }
    let mut scene_rows = Vec::new();
    for (scene, b) in &banks {
        let mut fields = vec![
            (
                "entries".to_string(),
                Json::List(
                    b.entries
                        .iter()
                        .map(|e| {
                            Json::Obj(vec![
                                ("event".into(), Json::Str(e.event.into())),
                                ("spu_address".into(), Json::Int(e.spu_address)),
                                ("offset".into(), Json::Int(e.offset as i64)),
                                ("bytes".into(), Json::Int(e.bytes as i64)),
                                ("rate".into(), Json::Int(e.rate)),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "refused".into(),
                Json::List(
                    b.refused
                        .iter()
                        .map(|(n, bytes, rate)| Json::Obj(vec![("event".into(), Json::Str(n.to_string())), ("bytes".into(), Json::Int(*bytes as i64)), ("rate".into(), Json::Int(*rate))]))
                        .collect(),
                ),
            ),
            ("chunk_bytes".into(), Json::Int(b.chunk.len() as i64)),
            ("chunk_sha256".into(), Json::Str(Sha256::digest(&b.chunk).iter().map(|x| format!("{x:02x}")).collect())),
            ("free_before".into(), Json::Int(b.free_before)),
            ("free_after".into(), Json::Int(b.free_after)),
        ];
        if let Some((rates, moved)) = &b.fitted {
            fields.push((
                "fitted".into(),
                Json::Obj(vec![
                    ("rates".into(), Json::Obj(rates.iter().map(|(n, r)| (n.clone(), Json::Int(*r))).collect())),
                    (
                        "moved".into(),
                        Json::List(
                            moved
                                .iter()
                                .map(|(e, rate, c)| {
                                    let mut f = vec![("event".to_string(), Json::Str(e.name.into())), ("row_rate".into(), Json::Int(e.rate)), ("rate".into(), Json::Int(*rate))];
                                    f.extend(check_fields(c));
                                    Json::Obj(f)
                                })
                                .collect(),
                        ),
                    ),
                ]),
            ));
        }
        scene_rows.push((scene.to_string(), Json::Obj(fields)));
    }
    let report_out = Json::Obj(vec![
        ("voice".into(), Json::Str("ambience SCENE_SFX_VOICE".into())),
        ("events".into(), Json::List(events)),
        ("scenes".into(), Json::Obj(scene_rows)),
        ("limitations".into(), Json::List(LIMITATIONS.iter().map(|s| Json::Str(s.to_string())).collect())),
    ]);
    std::fs::create_dir_all(root.join(".hkpsx")).map_err(|e| e.to_string())?;
    std::fs::write(root.join(".hkpsx/scene-sfx.json"), dumps(&report_out) + "\n").map_err(|e| e.to_string())?;
    let used: Vec<(&i64, &Bank)> = banks.iter().filter(|(_, b)| !b.entries.is_empty() || !b.refused.is_empty()).collect();
    let mut text = format!("Scene SFX: {} scenes ask for sounds, largest chunk {} bytes", used.len(), banks.values().map(|b| b.chunk.len()).max().unwrap_or(0));
    for (scene, bank) in used {
        let refused: Vec<String> = bank.refused.iter().map(|(n, bytes, _)| format!("{n} {bytes}B")).collect();
        let admitted: Vec<String> = bank
            .entries
            .iter()
            .map(|e| format!("{} {}B{}", e.event, e.bytes, if bank.fitted.is_some() { format!(" @{}", e.rate) } else { String::new() }))
            .collect();
        text += &format!(
            "\n  scene {scene}: {}{}; {} of {} left",
            admitted.join(", "),
            if refused.is_empty() { String::new() } else { format!("; refused {}", refused.join(", ")) },
            bank.free_after,
            bank.free_before
        );
    }
    Ok(text)
}

pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => Source::new(d).map_err(|e| e.to_string())?,
        None => Source::from_doctor(root).map_err(|e| e.to_string())?,
    };
    println!("{}", cook(root, &source)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ambience(ceiling: i64) -> J {
        // Stem 0 at 100..200, stem 1 at 300..400; the ring (ceiling) at 500.
        serde_json::json!({"spu_start": 0, "spu_ceiling": ceiling,
            "clips": [{"spu_address": 100, "spu_bytes": 100}, {"spu_address": 300, "spu_bytes": 100}],
            "cues": [{"scene": 0, "mask": 1, "prefetch": 2}, {"scene": 1, "mask": 0, "prefetch": 0}]})
    }
    fn rated(name: &'static str, rate: i64, priority: i64) -> Event {
        ev(name, "f", 1, name, rate, &["scene", "x"], priority, None)
    }
    fn check(pass: bool) -> Check {
        Check { pass, ship6_fwsnrseg: 0.0, fwsnrseg: 0.0, ship6_level_off_db: 0.0, level_off_db: 0.0 }
    }

    /// Fixed payload sizes and no measure: refusals stand.
    struct Fixed(HashMap<&'static str, usize>);
    impl Audio for Fixed {
        fn payload(&mut self, e: &Event, _: i64) -> Result<Vec<u8>> {
            Ok(vec![0; self.0[e.name]])
        }
        fn size(&mut self, e: &Event, _: i64) -> Result<usize> {
            Ok(self.0[e.name])
        }
        fn allocate(&mut self, _: &[(Event, Vec<i64>, Vec<usize>, usize)], _: i64) -> Result<Option<Vec<usize>>> {
            unreachable!()
        }
        fn check(&mut self, _: &Event, _: i64) -> Result<Check> {
            unreachable!()
        }
        fn can_fit(&self) -> bool {
            false
        }
    }

    /// Sizes proportional to rate, a greedy allocator stand-in that lowers the
    /// largest clip one step at a time, and a no-worse check that holds each
    /// clip in `floor` at or above that rate.
    struct Fake {
        seconds: HashMap<&'static str, f64>,
        floor: HashMap<&'static str, i64>,
        asked: Vec<i64>,
    }
    impl Audio for Fake {
        fn payload(&mut self, e: &Event, rate: i64) -> Result<Vec<u8>> {
            Ok(vec![0; self.size(e, rate)?])
        }
        fn size(&mut self, e: &Event, rate: i64) -> Result<usize> {
            Ok(oneshot_bytes(py_round(self.seconds[e.name] * rate as f64) as usize))
        }
        fn allocate(&mut self, rows: &[(Event, Vec<i64>, Vec<usize>, usize)], budget: i64) -> Result<Option<Vec<usize>>> {
            self.asked.push(budget);
            let mut steps = vec![0usize; rows.len()];
            if rows.iter().map(|r| r.2[r.3] as i64).sum::<i64>() > budget {
                return Ok(None);
            }
            let total = |steps: &[usize]| rows.iter().zip(steps).map(|(r, &s)| r.2[s] as i64).sum::<i64>();
            while total(&steps) > budget {
                let i = (0..rows.len()).filter(|&i| steps[i] < rows[i].3).max_by_key(|&i| (rows[i].2[steps[i]], std::cmp::Reverse(i))).unwrap();
                steps[i] += 1;
            }
            Ok(Some(steps))
        }
        fn check(&mut self, e: &Event, rate: i64) -> Result<Check> {
            Ok(check(rate >= self.floor.get(e.name).copied().unwrap_or(0)))
        }
    }
    fn fake(seconds: &[(&'static str, f64)], floor: &[(&'static str, i64)]) -> Fake {
        Fake { seconds: seconds.iter().copied().collect(), floor: floor.iter().copied().collect(), asked: Vec::new() }
    }
    fn needs(scene: i64, events: Vec<Event>) -> BTreeMap<i64, Vec<Event>> {
        [(scene, events)].into_iter().collect()
    }

    #[test]
    fn gaps_skip_the_scenes_own_and_neighbour_stems() {
        assert_eq!(free_gaps(&ambience(500), 0, true).unwrap(), [(0, 100), (200, 300), (400, 500)]);
        assert_eq!(free_gaps(&ambience(500), 1, true).unwrap(), [(0, 500)]);
    }

    #[test]
    fn priority_decides_and_a_clip_that_does_not_fit_is_refused_whole() {
        let mut audio = Fixed([("a", 96), ("b", 96), ("c", 96), ("d", 96)].into_iter().collect());
        let n = needs(0, vec![rated("d", 8000, 3), rated("a", 8000, 0), rated("c", 8000, 2), rated("b", 8000, 1)]);
        let banks = assemble(&n, &ambience(500), &mut audio).unwrap();
        let bank = &banks[&0];
        assert_eq!(bank.entries.iter().map(|e| e.event).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(bank.entries.iter().map(|e| e.spu_address).collect::<Vec<_>>(), [0, 200, 400]);
        assert_eq!(bank.refused, [("d", 96, 8000)]);
        assert_eq!(bank.chunk.len(), 288);
    }

    #[test]
    fn a_scene_without_sounds_still_owns_a_chunk() {
        let banks = assemble(&needs(1, Vec::new()), &ambience(500), &mut Fixed(HashMap::new())).unwrap();
        assert_eq!(banks[&1].chunk, EMPTY);
        assert!(banks[&1].entries.is_empty());
    }

    #[test]
    fn a_scene_that_fits_at_row_rates_is_not_refitted() {
        let mut m = fake(&[("a", 0.01)], &[]);
        let banks = assemble(&needs(1, vec![rated("a", 8000, 0)]), &ambience(500), &mut m).unwrap();
        assert!(banks[&1].fitted.is_none());
        assert_eq!(banks[&1].entries[0].rate, 8000);
        assert!(m.asked.is_empty());
    }

    #[test]
    fn a_refused_clip_is_fitted_by_lowering_rates() {
        // 500 bytes of gap; each clip is 256 bytes at 8000 Hz.
        let mut m = fake(&[("a", 0.05), ("b", 0.05), ("c", 0.05)], &[]);
        let banks = assemble(&needs(1, vec![rated("a", 8000, 0), rated("b", 8000, 1), rated("c", 8000, 2)]), &ambience(500), &mut m).unwrap();
        let bank = &banks[&1];
        assert!(bank.refused.is_empty());
        assert_eq!(bank.entries.len(), 3);
        assert!(bank.entries.iter().map(|e| e.bytes).sum::<usize>() <= 500);
        assert!(bank.entries.iter().any(|e| e.rate < 8000));
        let (rates, _) = bank.fitted.as_ref().unwrap();
        let mut a: Vec<(String, i64)> = rates.clone();
        let mut b: Vec<(String, i64)> = bank.entries.iter().map(|e| (e.event.to_string(), e.rate)).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn a_held_clip_never_drops_below_its_no_worse_floor() {
        // a fits alone at its row rate, b is refused beside it; b must give.
        let mut m = fake(&[("a", 0.05), ("b", 0.05)], &[("a", 8000)]);
        let banks = assemble(&needs(1, vec![rated("a", 8000, 0), rated("b", 8000, 1)]), &ambience(490), &mut m).unwrap();
        let rate = |n: &str| banks[&1].entries.iter().find(|e| e.event == n).unwrap().rate;
        assert_eq!(rate("a"), 8000);
        assert!(rate("b") < 8000);
    }

    #[test]
    fn the_lowest_priority_clip_is_refused_when_floors_cannot_fit() {
        // a is 480 bytes at its 8000 Hz floor; b is 112 even at 1600 Hz.
        let mut m = fake(&[("a", 0.1), ("b", 0.1)], &[("a", 8000)]);
        let banks = assemble(&needs(1, vec![rated("a", 8000, 0), rated("b", 8000, 1)]), &ambience(500), &mut m).unwrap();
        assert_eq!(banks[&1].entries.iter().map(|e| (e.event, e.rate)).collect::<Vec<_>>(), [("a", 8000)]);
        assert_eq!(banks[&1].refused.iter().map(|r| r.0).collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn the_rust_table_carries_each_entrys_rate_and_pitch() {
        let bank = Bank {
            chunk: vec![0; 32],
            entries: vec![Entry { event: "bench_rest", spu_address: 64, offset: 0, bytes: 32, rate: 4000 }],
            refused: Vec::new(),
            free_before: 0,
            free_after: 0,
            fitted: None,
        };
        let text = rust(&[(0, bank)].into_iter().collect());
        let index = EVENTS.iter().position(|e| e.name == "bench_rest").unwrap();
        assert!(text.contains(&format!("({index},64,0,32,4000,{}),", pitch_register(4000))));
    }

    #[test]
    fn a_db_trim_keeps_the_body_and_fades_the_cut() {
        // 100 ms at full level, then 100 ms 60 dB down, at 1000 Hz.
        let pcm: Vec<i16> = std::iter::repeat_n(10000, 100).chain(std::iter::repeat_n(10, 100)).collect();
        let (kept, dropped) = trim_tail(pcm, 1000, Some(Trim::Db(-40)));
        assert_eq!(kept.len(), 100);
        assert_eq!(kept[0], 10000);
        assert_eq!(kept[99], 500); // the last of a 20-sample linear fade
        assert!(dropped.unwrap() < -50.0);
    }

    #[test]
    fn a_seconds_trim_cuts_where_it_is_told_and_none_keeps_all() {
        let pcm = vec![1000i16; 300];
        let (kept, dropped) = trim_tail(pcm.clone(), 100, Some(Trim::Seconds(2.0)));
        assert_eq!(kept.len(), 200);
        assert!((dropped.unwrap() - -4.8).abs() < 0.05);
        assert_eq!(trim_tail(pcm.clone(), 100, None), (pcm, None));
    }

    #[test]
    fn ship6_blocks_end_in_a_silent_terminator() {
        let out = ship6_encode(&[1000; 30]);
        assert_eq!(out.len(), 3 * 16);
        assert_eq!(&out[32..34], &[12, 1]);
        assert!(check_oneshot(&out).is_ok());
    }
}
