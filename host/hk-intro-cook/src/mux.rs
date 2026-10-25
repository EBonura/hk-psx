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

/// The unpaced mux, kept for the tests as the plain reference of where the sound goes.
/// `picture`: the encoder's stream, whose sound sectors are discarded;
/// `sound`: the re-encoded sound sectors. The result has the picture sectors
/// in their original order and a sound sector at every [`AUDIO_PERIOD`]th
/// sector from `delay` on. Sound that would come after the last picture sector
/// is not kept (the player has stopped reading by then; the movie's last
/// moments are silent).
#[cfg(test)]
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

/// Slots a frame has: 150 sectors a second at 15 frames a second.
pub const SLOTS_PER_FRAME: usize = 10;

/// Like [`interleave`], but every frame gets exactly [`SLOTS_PER_FRAME`] slots and its last sector in the
/// last free slot of them, so that the drive delivers a frame's last sector at the same moment of every
/// tenth slot whatever the frame's size, and the player (which shows a frame a fixed time after its last
/// sector arrives) shows 15 frames a second evenly. The sound takes the same slots as in [`interleave`]
/// (every 16th from `delay`); a frame's sectors go into the free slots at the end of its window, the earlier
/// ones are filler (a data sector with no picture in it, which the player skips as foreign). A frame with
/// more sectors than its window has free slots (10 less a sound slot) is an error.
pub fn interleave_paced(picture: &[u8], sound: &[u8], delay: usize) -> Result<Vec<u8>, String> {
    if !picture.len().is_multiple_of(SECTOR) || !sound.len().is_multiple_of(SECTOR) {
        return Err("a stream is not whole sectors".into());
    }
    let video: Vec<&[u8]> = picture
        .chunks_exact(SECTOR)
        .filter(|s| s[2] & SUBMODE_AUDIO == 0)
        .collect();
    let template = video.first().ok_or("the picture has no sectors")?;
    // Frames in order: the chunk header's frame number is at bytes 8 + 4 (frame) of the sector's user data.
    let frame_of = |s: &[u8]| u32::from_le_bytes([s[8 + 8], s[8 + 9], s[8 + 10], s[8 + 11]]);
    let mut frames: Vec<Vec<&[u8]>> = Vec::new();
    let mut last = None;
    for sector in &video {
        let number = frame_of(sector);
        if last != Some(number) {
            frames.push(Vec::new());
            last = Some(number);
        }
        frames.last_mut().expect("a frame was just pushed").push(sector);
    }
    let slots = frames.len() * SLOTS_PER_FRAME;
    let mut audio = sound.chunks_exact(SECTOR);
    let mut table: Vec<Option<&[u8]>> = vec![None; slots];
    let mut is_sound = vec![false; slots];
    for (slot, flag) in is_sound.iter_mut().enumerate() {
        if slot >= delay && (slot - delay).is_multiple_of(AUDIO_PERIOD) {
            if let Some(sector) = audio.next() {
                table[slot] = Some(sector);
                *flag = true;
            }
        }
    }
    for (f, frame) in frames.iter().enumerate() {
        let window = f * SLOTS_PER_FRAME..(f + 1) * SLOTS_PER_FRAME;
        let free: Vec<usize> = window.filter(|&s| !is_sound[s]).collect();
        if frame.len() > free.len() {
            return Err(format!("frame {} has {} sectors, its window has {} free", f + 1, frame.len(), free.len()));
        }
        for (sector, &slot) in frame.iter().zip(&free[free.len() - frame.len()..]) {
            table[slot] = Some(sector);
        }
    }
    // Filler: the first picture sector with the payload zeroed (so that no chunk header parses in it).
    let mut filler = template.to_vec();
    filler[8..8 + 2048].fill(0);
    let mut out = Vec::with_capacity(slots * SECTOR);
    for slot in table {
        out.extend_from_slice(slot.unwrap_or(&filler));
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

    fn picture_frames(sizes: &[usize]) -> Vec<u8> {
        let mut out = Vec::new();
        for (f, &n) in sizes.iter().enumerate() {
            for i in 0..n {
                let mut s = vec![0u8; SECTOR];
                s[2] = 0x08;
                s[8 + 8..8 + 12].copy_from_slice(&(f as u32 + 1).to_le_bytes());
                s[8 + 12] = i as u8;
                out.extend_from_slice(&s);
            }
        }
        out
    }

    #[test]
    fn every_frame_gets_ten_slots_and_ends_in_the_last_free_one() {
        let sound = stream(&[(true, 200), (true, 201)]);
        let out = interleave_paced(&picture_frames(&[9, 7, 9]), &sound, 3).unwrap();
        assert_eq!(out.len(), 30 * SECTOR);
        let at = |i: usize| &out[i * SECTOR..(i + 1) * SECTOR];
        // Frame 1 (slots 0..10): sound at 3, so its 9 sectors take the other nine slots.
        assert_eq!(at(3)[2] & SUBMODE_AUDIO, SUBMODE_AUDIO);
        // Frame 2 (10..20): the second sound sector sits at 19, so its 7 sectors take slots 12 to 18 and 10 and 11 are filler.
        assert_eq!(at(10)[8 + 8..8 + 12], [0, 0, 0, 0]);
        assert_eq!(at(11)[8 + 8..8 + 12], [0, 0, 0, 0]);
        assert_eq!(at(12)[8 + 8..8 + 12], 2u32.to_le_bytes());
        assert_eq!(at(12)[8 + 12], 0);
        assert_eq!(at(18)[8 + 12], 6);
        assert_eq!(at(19)[2] & SUBMODE_AUDIO, SUBMODE_AUDIO);
        assert_eq!(at(19)[8], 201);
    }

    #[test]
    fn a_frame_too_big_for_its_window_is_refused() {
        let sound = stream(&[(true, 200)]);
        assert!(interleave_paced(&picture_frames(&[10]), &sound, 3).is_err());
    }

    #[test]
    fn sound_that_is_not_audio_is_refused() {
        assert!(sparse(&stream(&[(false, 1)]), 8, 2, 0).is_err());
        assert!(sparse(&[], 8, 2, 0).is_err());
    }
}
