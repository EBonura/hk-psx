//! Putting the movie's sound and picture back together in one sector stream.
//!
//! The encoder writes a picture sector run with one sound sector in
//! sixteen, the sound sector ahead of the picture it belongs to. The sound
//! plays from the moment its sector arrives, while the player shows a
//! picture a frame's reading time plus a decoder headroom after the
//! picture's last sector arrives, so the picture lags the sound by that much.
//! Moving every sound sector later by the same time puts them together; the
//! cook does that here, since the drive plays what it reads when it reads it.

use crate::xa::{SECTOR, SUBMODE_AUDIO};

/// Sectors the drive keeps reading past the end of an XA file (psx-hw `END_GUARD_SECTORS`), so that a
/// player's poll still finds the head inside the file when the sound has ended.
pub const END_GUARD: usize = 16;
/// Channel of the filler sectors between the sound's (what psx-audio-cook's own files use).
const FILLER_CHANNEL: u8 = 0xFF;
/// Audio, Form 2, real time; the last sector of the file adds end-of-file and end-of-record.
const SUBMODE_SOUND: u8 = 0x04 | 0x20 | 0x40;
const SUBMODE_END: u8 = 0x80 | 0x01;

/// Sectors between two sound sectors of a mono 37.8 kHz stream read at double
/// speed: 150 sectors a second, 9.375 sound sectors a second.
pub const AUDIO_PERIOD: usize = 16;

/// `picture`: the encoder's stream, whose sound sectors are discarded;
/// `sound`: the re-encoded sound sectors. The result has the picture sectors
/// in their original order and a sound sector at every [`AUDIO_PERIOD`]th
/// sector from `delay` on. Sound that would come after the last picture sector
/// is not kept (the player has stopped reading by then; the movie's last
/// moments are silent).
pub fn interleave(picture: &[u8], sound: &[u8], delay: usize) -> Result<Vec<u8>, String> {
    if !picture.len().is_multiple_of(SECTOR) || !sound.len().is_multiple_of(SECTOR) {
        return Err("a stream is not whole sectors".into());
    }
    let video: Vec<&[u8]> = picture
        .chunks_exact(SECTOR)
        .filter(|s| s[2] & SUBMODE_AUDIO == 0)
        .collect();
    let mut audio = sound.chunks_exact(SECTOR);
    let mut out = Vec::with_capacity(picture.len());
    let (mut at, mut shown) = (0usize, 0usize);
    // Every slot takes the next picture sector, except a sound slot while
    // sound lasts. The stream ends with the last picture sector.
    while shown < video.len() {
        let is_sound = at >= delay && (at - delay).is_multiple_of(AUDIO_PERIOD);
        match is_sound.then(|| audio.next()).flatten() {
            Some(sector) => out.extend_from_slice(sector),
            None => {
                out.extend_from_slice(video[shown]);
                shown += 1;
            }
        }
        at += 1;
    }
    Ok(out)
}

/// A sound-only XA file for single-speed playback: the encoder's sound sectors (`sound`, all one file
/// and channel, `file` and `channel` here) on every `stride`th sector, filler on the others, and an end
/// guard of filler. The drive plays the sound at the rate it was encoded for, a sector of it every
/// `stride` sectors, which is the interleave real-time playback needs (mono 37.8 kHz: 8).
pub fn sparse(sound: &[u8], stride: usize, file: u8, channel: u8) -> Result<Vec<u8>, String> {
    if !sound.len().is_multiple_of(SECTOR) || sound.is_empty() {
        return Err("the sound is not whole sectors".into());
    }
    let sectors: Vec<&[u8]> = sound.chunks_exact(SECTOR).collect();
    let coding = sectors[0][3];
    if sectors.iter().any(|s| s[3] != coding || s[2] & SUBMODE_AUDIO == 0) {
        return Err("the sound sectors differ in coding or are not audio".into());
    }
    let total = sectors.len() * stride + END_GUARD;
    let mut out = Vec::with_capacity(total * SECTOR);
    for at in 0..total {
        let end = if at + 1 == total { SUBMODE_END } else { 0 };
        let (channel, data) = match (at % stride == 0).then(|| sectors.get(at / stride)).flatten() {
            Some(sector) => (channel, Some(*sector)),
            None => (FILLER_CHANNEL, None),
        };
        let head = [file, channel, SUBMODE_SOUND | end, coding];
        out.extend_from_slice(&head);
        out.extend_from_slice(&head);
        match data {
            Some(sector) => out.extend_from_slice(&sector[8..]),
            None => out.resize(out.len() + SECTOR - 8, 0),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(kinds: &[(bool, u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (audio, tag) in kinds {
            let mut s = vec![*tag; SECTOR];
            s[2] = if *audio { SUBMODE_AUDIO } else { 0x08 };
            out.extend_from_slice(&s);
        }
        out
    }

    fn tags(raw: &[u8]) -> Vec<(bool, u8)> {
        raw.chunks_exact(SECTOR)
            .map(|s| (s[2] & SUBMODE_AUDIO != 0, s[8]))
            .collect()
    }

    #[test]
    fn sound_moves_later_and_the_picture_keeps_its_order() {
        let picture = stream(&[(true, 0), (false, 1), (false, 2), (false, 3), (false, 4)]);
        let sound = stream(&[(true, 100), (true, 101)]);
        let out = interleave(&picture, &sound, 2).unwrap();
        assert_eq!(
            tags(&out),
            vec![(false, 1), (false, 2), (true, 100), (false, 3), (false, 4)]
        );
    }

    #[test]
    fn a_zero_delay_starts_with_sound_as_the_encoder_does() {
        let picture = stream(&[(false, 1), (false, 2)]);
        let sound = stream(&[(true, 100)]);
        let out = interleave(&picture, &sound, 0).unwrap();
        assert_eq!(tags(&out), vec![(true, 100), (false, 1), (false, 2)]);
    }

    #[test]
    fn sound_after_the_last_picture_is_dropped() {
        let picture = stream(&[(false, 1), (false, 2)]);
        let sound = stream(&[(true, 100)]);
        let out = interleave(&picture, &sound, 2).unwrap();
        assert_eq!(tags(&out), vec![(false, 1), (false, 2)]);
    }

    #[test]
    fn sound_repeats_every_sixteenth_sector() {
        let video: Vec<(bool, u8)> = (0..40).map(|i| (false, i as u8)).collect();
        let sound = stream(&[(true, 200), (true, 201), (true, 202)]);
        let out = interleave(&stream(&video), &sound, 3).unwrap();
        let positions: Vec<usize> = tags(&out)
            .iter()
            .enumerate()
            .filter(|(_, (a, _))| *a)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions, vec![3, 19, 35]);
    }

    #[test]
    fn sound_sits_on_every_stride_and_the_file_ends_in_a_guard() {
        let mut sound = stream(&[(true, 100), (true, 101)]);
        for sector in sound.chunks_exact_mut(SECTOR) {
            sector[3] = 0;
        }
        let out = sparse(&sound, 8, 2, 0).unwrap();
        assert_eq!(out.len(), (2 * 8 + END_GUARD) * SECTOR);
        let at = |i: usize| &out[i * SECTOR..(i + 1) * SECTOR];
        assert_eq!((at(0)[0], at(0)[1], at(0)[2], at(0)[8]), (2, 0, 0x64, 100));
        assert_eq!((at(8)[1], at(8)[8]), (0, 101));
        for i in [1, 7, 9, 15, 16] {
            assert_eq!(at(i)[1], 0xFF, "{i}");
        }
        let last = at(2 * 8 + END_GUARD - 1);
        assert_eq!(last[2], 0x64 | 0x81);
        // The repeated subheader.
        assert_eq!(at(3)[..4], at(3)[4..8]);
    }

    #[test]
    fn sound_that_is_not_audio_is_refused() {
        assert!(sparse(&stream(&[(false, 1)]), 8, 2, 0).is_err());
        assert!(sparse(&[], 8, 2, 0).is_err());
    }
}
