//! Encodes the two opening cinematics of Hollow Knight for the intro program.
//!
//! The original's `Opening_Sequence` plays two Unity VideoClips, `Prologue`
//! and then `Intro`, H.264 + AAC MP4s that sit in the install's
//! `hollow_knight_Data/sharedassets5.resource`. The cook finds both by class
//! (VideoClip) and name through the repository's Unity reader, checks that
//! they are the clips this cook was written against (size, frame count, frame
//! rate, picture size), cuts each MP4 out of the resource file and encodes it
//! with `psxavenc` as a 320x192 (the 16:9 picture, 320x180, letterboxed on a 320x240 screen), 15
//! frames a second, bitstream version 2 STR with interleaved XA-ADPCM audio.
//! It then decodes every frame once with the SDK's bitstream decoder and
//! writes what the guest has to size its buffers for. Both clips of the
//! install this was written against carry a digital-silent audio track: the
//! original plays their sound from separate AudioClips (`hk-cook intro-audio`
//! decodes them, `--audio-dir`), mastered as the Half-Life cook does (see
//! [`encode`]). Without `--audio-dir` the streams hold silent XA sectors.
//! Nothing it writes
//! belongs in the repository: the movies and everything derived from them
//! stay under `.hkpsx/` beside the other build output.
//!
//! ```text
//! hk-intro-cook (--data-dir DIR | --from-doctor REPO) --out DIR [--psxavenc PATH]
//! ```
//!
//! `DIR` is the install's `hollow_knight_Data`; `--from-doctor` takes the
//! one the repository's `.hkpsx/doctor.json` selected. Writes `HKINTRO1.STR`
//! (Prologue) and `HKINTRO2.STR` (Intro) as raw 2336-byte sectors for
//! `mkisopsx --xa-file`, and `movie.rs` (constants the intro program includes:
//! `MOVIES` and one module per movie, `prologue` and `intro`). Both movies file
//! their sound under the same XA file and channel (file 1, channel 0): the
//! drive is only ever playing one of them, and the player starts each movie
//! with that filter set. `psxavenc` comes from `--psxavenc`, then `PSXAVENC`,
//! then the `PATH`. Exit status 3 means the install or the encoder is missing
//! (or the install has no such clips), which is not an error for the caller:
//! the disc is then built without an intro.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use hk_unity::Value;

mod limiter;
mod loudness;
mod mux;
mod xa;

use psx_fmv::bitstream as bs;
use psx_fmv::stream::{Chunk, FrameAssembler, CHUNK_PAYLOAD_BYTES, MAX_CHUNKS};

/// Raw XA sector record: 8-byte subheader and 2328 bytes.
const XA_SECTOR: usize = 2336;
const SUBMODE_AUDIO: u8 = 0x04;
const SUBMODE_FORM2: u8 = 0x20;

/// Picture and rate the intro plays at: 320x180 in a coded 320x192 (whole 16x16 macroblocks), centred on the
/// 320x240 screen.
const WIDTH: u16 = 320;
/// The picture's own height: the 16:9 clips fill the 320 columns of a 4:3 screen with 180 lines.
const PICTURE_HEIGHT: u32 = 180;
/// The coded height: a whole number of 16 line macroblocks (12), with the 6 black lines above and below
/// the 180 the encoder pads, so that the picture is not squashed to 176 (2%) the way a coded height of 11
/// macroblocks would make it.
const HEIGHT: u16 = 192;
const FPS: u32 = 15;
/// XA file and channel the sound of both movies is filed under (psxavenc
/// `-F`, `-C`).
const XA_FILE: u8 = 1;
const XA_CHANNEL: u8 = 0;
/// Exit status for "no intro on this machine".
const NO_INTRO: u8 = 3;

/// Sample rate of the movie's sound on the disc.
const SOUND_HZ: f64 = 37_800.0;
/// Words of memory the SDK's player asks for one of these movies at 15 bits (`psx_fmv::player::memory_words`,
/// which only builds for the console): three frame slots of the most sectors a frame spans, two run-length
/// buffers of the most words a frame decodes to (whole DMA blocks), and two whole decoded frames.
fn player_words(m: &Movie) -> usize {
    const SLOTS: usize = 3;
    let slot = usize::from(m.max_chunks) * (CHUNK_PAYLOAD_BYTES / 4);
    let rle = m.rle_words.div_ceil(psx_fmv::mdec::DMA_BLOCK_WORDS) * psx_fmv::mdec::DMA_BLOCK_WORDS;
    let frame = usize::from(m.width) / 16 * (4 * 2 * usize::from(m.height));
    SLOTS * slot + 2 * rle + 2 * frame
}

/// What the intro program may use of the game's scene arena for the player's buffers (game/src/disc.rs
/// `arena()`, 415,052 bytes in the shipping build; it moves with the scenes). The dither makes the frames the
/// encoder fills its 10 sectors with decode to about 13,300 run-length words (12,000 undithered) and, with
/// the 192 line frame buffers, the player needs 413 KB of it: hardly any margin, so the cook refuses a
/// movie that needs more and the guest build asserts the same against the arena (`INTRO_PLAYER_BYTES`).
const PLAYER_MEMORY_LIMIT: usize = 415_000;
/// VBlanks from a picture's last sector to its flip, as the intro asks the
/// player (`Config::latency_vblanks`); written to `movie.rs`.
const LATENCY_VBLANKS: u32 = 5;
/// How much quieter the player's sound path is than the stream it is fed: the
/// 37.8 to 44.1 kHz conversion of the CD sound costs about 0.9 dB of
/// loudness (the SPU's own mix is unity at the main volume 0x3FFF the intro
/// sets). Measured for the Half-Life intro on the SPU output of the headless
/// emulator against that install's movie: with the stream raised by this much
/// the output read the same loudness as the original played to both speakers.
/// It is a property of the player and the stream format (mono 37.8 kHz XA at
/// double speed), which are the same here, so it carries over; it has not been
/// re-measured on these two movies. The gain goes through the limiter, which
/// holds the stream under its ceiling whatever the movie peaks at.
const PLAYBACK_LOSS_DB: f64 = 0.9;
/// How far the install's sound clip may differ in length from the picture it
/// plays with: the encoder ends a picture about 0.2 s early, and the clips are
/// authored to within a frame or two of the video.
const MAX_SOUND_DRIFT_S: f64 = 0.6;
/// The piano under the poem, a sound-only XA file of its own (`HKPOEM.XA`): file number (the movies'
/// sound is file 1), channel, and the interleave a mono 37.8 kHz stream needs at single speed.
const POEM_FILE: &str = "HKPOEM.XA";
const POEM_XA_FILE: u8 = 2;
const POEM_CHANNEL: u8 = 0;
const POEM_STRIDE: usize = 8;
/// Silence ahead of the piano in the file. The drive's seek and start take a variable while, and
/// the poem starts the file early enough that the piano's first note lands where the original's
/// does (3.53 s into the sequence, `Music` activated by the clip); the intro program measures the
/// start and shows the text against the sound's own clock.
const POEM_LEAD_IN_S: f64 = 2.0;
/// Sectors the sound is moved later behind the encoder's placement, so that
/// it plays together with the picture. The picture shows its frame the
/// headroom of [`LATENCY_VBLANKS`] after the frame's last sector arrives, and
/// a frame is read in 10 sectors (150 a second, 15 frames a second), so the
/// sound has to wait 10 sectors plus 5 / 59.94 s (12.5 sectors) behind where
/// the encoder starts it; 1.5 more sectors (10 ms) are what the first frame's
/// decode and the flip to the next VBlank add, measured on the emulator.
const AUDIO_DELAY_SECTORS: usize = 24;

#[derive(Debug, Default, PartialEq, Eq)]
struct Movie {
    frames: u32,
    width: u16,
    height: u16,
    max_chunks: u16,
    rle_words: usize,
    xa: Option<(u8, u8)>,
    video_sectors: u32,
    audio_sectors: u32,
    other_sectors: u32,
    versions: [u32; 4],
}

/// Decodes every frame of `raw` (2336-byte sectors) and returns the figures
/// the guest needs, or the first reason the player could not show it.
fn measure(raw: &[u8]) -> Result<Movie, String> {
    if raw.is_empty() || !raw.len().is_multiple_of(XA_SECTOR) {
        return Err(format!(
            "{} bytes is not a whole number of {XA_SECTOR}-byte sectors",
            raw.len()
        ));
    }
    let mut m = Movie::default();
    let mut asm = FrameAssembler::new();
    let mut frame = vec![0u8; MAX_CHUNKS as usize * CHUNK_PAYLOAD_BYTES];
    let mut rle = vec![0u16; 1 << 17];
    for (i, sector) in raw.chunks_exact(XA_SECTOR).enumerate() {
        let (sub, data) = sector.split_at(8);
        let submode = sub[2];
        if submode & SUBMODE_AUDIO != 0 {
            if submode & SUBMODE_FORM2 == 0 {
                return Err(format!("sector {i}: an audio sector outside Form 2"));
            }
            let at = (sub[0], sub[1]);
            match m.xa {
                None => m.xa = Some(at),
                Some(xa) if xa != at => {
                    return Err(format!("sector {i}: audio from {at:?}, earlier {xa:?}"))
                }
                Some(_) => {}
            }
            m.audio_sectors += 1;
            continue;
        }
        let user = &data[..2048];
        let Some(chunk) = Chunk::parse(user) else {
            m.other_sectors += 1;
            continue;
        };
        if submode & SUBMODE_FORM2 != 0 {
            return Err(format!("sector {i}: a video sector in Form 2"));
        }
        m.video_sectors += 1;
        m.max_chunks = m.max_chunks.max(chunk.count);
        let Some(done) = asm.add(user, &mut frame) else {
            continue;
        };
        if done.number != m.frames + 1 {
            return Err(format!(
                "sector {i}: frame {} follows frame {}",
                done.number, m.frames
            ));
        }
        if m.frames == 0 {
            (m.width, m.height) = (done.width, done.height);
        } else if (done.width, done.height) != (m.width, m.height) {
            return Err(format!(
                "frame {}: {}x{}, earlier frames {}x{}",
                done.number, done.width, done.height, m.width, m.height
            ));
        }
        m.frames = done.number;
        let bytes = &frame[..(done.size as usize).min(frame.len())];
        let header =
            bs::Header::parse(bytes).map_err(|e| format!("frame {}: {e:?}", done.number))?;
        m.versions[(header.version as usize).min(3)] += 1;
        let macroblocks = (done.width as u32 / 16) * (done.height as u32 / 16);
        let words = bs::decode_frame(bytes, &mut rle, macroblocks, macroblocks, &mut || {})
            .map_err(|e| format!("frame {}: {e:?}", done.number))?;
        m.rle_words = m.rle_words.max(words);
    }
    if asm.dropped != 0 {
        return Err(format!("{} frames have sectors missing", asm.dropped));
    }
    if m.frames == 0 || (m.width, m.height) != (WIDTH, HEIGHT) {
        return Err(format!(
            "{} frames of {}x{} is not a {WIDTH}x{HEIGHT} movie",
            m.frames, m.width, m.height
        ));
    }
    if m.xa.is_none() {
        return Err("the encoder wrote no audio".into());
    }
    Ok(m)
}

/// Where the data of a sector record ends and what the disc builder rebuilds
/// begins: after the 2048 bytes of a Form 1 sector's user data, and after the
/// 18 sound groups of 128 bytes of a Form 2 audio sector.
const FORM1_TAIL: std::ops::Range<usize> = 8 + 2048..XA_SECTOR;
const AUDIO_TAIL: std::ops::Range<usize> = 8 + 18 * 128..XA_SECTOR;

/// Zero what follows each sector's data: the encoder leaves it as whatever its
/// memory held, which makes the disc differ from one cook of the same movie to
/// the next, and the disc builder rebuilds it (checksums, and for audio 20
/// unused bytes the drive never plays) at the sector's final address.
fn zero_sector_tails(raw: &mut [u8]) {
    for sector in raw.chunks_exact_mut(XA_SECTOR) {
        let tail = if sector[2] & SUBMODE_AUDIO != 0 {
            AUDIO_TAIL
        } else {
            FORM1_TAIL
        };
        sector[tail].fill(0);
    }
}

/// What this cook was written against for one of the two movies: the clip's
/// name in the install, what the install's file says about it, and where the
/// cook files the result.
struct Spec {
    /// `m_Name` of the VideoClip.
    name: &'static str,
    /// Module of `movie.rs` and stem of the scratch directory.
    module: &'static str,
    /// Raw sector stream written to the output directory.
    file: &'static str,
    /// Size of the MP4 in the resource file, in bytes.
    size: u64,
    /// Frames and frame rate of the source video.
    frames: u64,
    fps: f64,
}

/// The two movies in the order the original's `Opening_Sequence` plays them.
const SPECS: [Spec; 2] = [
    Spec {
        name: "Prologue",
        module: "prologue",
        file: "HKINTRO1.STR",
        size: 4_678_720,
        frames: 906,
        fps: 30.0,
    },
    Spec {
        name: "Intro",
        module: "intro",
        file: "HKINTRO2.STR",
        size: 4_128_070,
        frames: 953,
        fps: 25.0,
    },
];

/// Picture size both clips are authored at.
const SOURCE_SIZE: (u32, u32) = (1920, 1080);

/// One VideoClip of the install as its Unity object describes it.
#[derive(Debug, Clone, PartialEq)]
struct Clip {
    name: String,
    /// `m_ExternalResources.m_Source`: the resource file the MP4 sits in.
    source: String,
    offset: u64,
    size: u64,
    frames: u64,
    fps: f64,
    size_px: (u32, u32),
}

/// The fields of a VideoClip value, or `None` when one is missing.
fn clip_from_value(v: &Value) -> Option<Clip> {
    let ext = v.get("m_ExternalResources")?;
    let num = |value: &Value| u64::try_from(value.int()?).ok();
    Some(Clip {
        name: v.get("m_Name")?.str()?,
        source: ext.get("m_Source")?.str()?,
        offset: num(ext.get("m_Offset")?)?,
        size: num(ext.get("m_Size")?)?,
        frames: num(v.get("m_FrameCount")?)?,
        fps: v.get("m_FrameRate")?.float()?,
        size_px: (
            u32::try_from(num(v.get("Width")?)?).ok()?,
            u32::try_from(num(v.get("Height")?)?).ok()?,
        ),
    })
}

/// The clip called `name`: `Ok(None)` when the install has none, an error
/// when it has several (the cook would have to guess).
fn pick<'a>(clips: &'a [Clip], name: &str) -> Result<Option<&'a Clip>, String> {
    let mut named = clips.iter().filter(|c| c.name == name);
    let first = named.next();
    if named.next().is_some() {
        return Err(format!("the install has more than one VideoClip {name}"));
    }
    Ok(first)
}

/// Refuses a clip that is not the one the cook was written against, since the
/// intro's timing and the owner's size and rate decisions were made on it.
fn check_clip(spec: &Spec, clip: &Clip) -> Result<(), String> {
    let mut wrong = Vec::new();
    if clip.size != spec.size {
        wrong.push(format!("{} bytes, expected {}", clip.size, spec.size));
    }
    if clip.frames != spec.frames {
        wrong.push(format!("{} frames, expected {}", clip.frames, spec.frames));
    }
    if (clip.fps - spec.fps).abs() > 1e-6 {
        wrong.push(format!("{} fps, expected {}", clip.fps, spec.fps));
    }
    if clip.size_px != SOURCE_SIZE {
        wrong.push(format!(
            "{}x{}, expected {}x{}",
            clip.size_px.0, clip.size_px.1, SOURCE_SIZE.0, SOURCE_SIZE.1
        ));
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "VideoClip {} is not the one this cook was written for: {}",
            spec.name,
            wrong.join(", ")
        ))
    }
}

/// Frames the 15 fps movie should have when it covers the source's duration.
fn expected_frames(spec: &Spec) -> u32 {
    (spec.frames as f64 / spec.fps * f64::from(FPS)).round() as u32
}

/// How many frames short of [`expected_frames`] the encoder may stop. Measured
/// on this install's two clips it ends them 3 frames (0.2 s) early whatever
/// the frame rate asked for (4,500 sectors for the 30.2 s Prologue, 5,690 for
/// the 38.12 s Intro), so a few frames are the encoder's, and a larger gap is
/// a cut or a different clip.
const MAX_SHORT_FRAMES: u32 = 5;

/// Whether `frames` is a plausible cook of `spec`: never longer than the
/// source by more than a frame, never shorter by more than [`MAX_SHORT_FRAMES`].
fn length_is_plausible(spec: &Spec, frames: u32) -> bool {
    let want = expected_frames(spec);
    frames <= want + 1 && frames + MAX_SHORT_FRAMES >= want
}

/// Every VideoClip (class 329) of `sharedassets5.assets`.
fn find_clips(source: &hk_unity::Source) -> Result<Vec<Clip>, String> {
    const VIDEO_CLIP: i32 = 329;
    let file = source
        .file("sharedassets5.assets")
        .map_err(|e| format!("sharedassets5.assets: {e}"))?;
    let mut clips = Vec::new();
    for info in file.objects.iter().filter(|o| o.class_id == VIDEO_CLIP) {
        let obj = source
            .object(&file, info.path_id)
            .map_err(|e| e.to_string())?;
        let value = source
            .read(&obj)
            .map_err(|e| format!("{}: {e}", obj.sid()))?;
        clips.push(
            clip_from_value(&value).ok_or_else(|| format!("{}: not a VideoClip", obj.sid()))?,
        );
    }
    Ok(clips)
}

/// The bytes of a clip inside its resource file.
fn clip_slice<'a>(resource: &'a [u8], clip: &Clip) -> Result<&'a [u8], String> {
    let end = clip
        .offset
        .checked_add(clip.size)
        .filter(|end| *end <= resource.len() as u64)
        .ok_or_else(|| {
            format!(
                "{}: {}+{} is past the end of {} ({} bytes)",
                clip.name,
                clip.offset,
                clip.size,
                clip.source,
                resource.len()
            )
        })?;
    let bytes = &resource[clip.offset as usize..end as usize];
    // An MP4 starts with a box whose type is `ftyp`.
    if bytes.get(4..8) != Some(b"ftyp") {
        return Err(format!("{}: the bytes are not an MP4", clip.name));
    }
    Ok(bytes)
}

/// The generated constants the intro program includes: how many movies, and
/// for each (in play order) the figures its buffers are sized by.
fn constants(movies: &[(&Spec, &Movie)], poem: &Poem) -> String {
    let player_bytes = movies.iter().map(|(_, m)| player_words(m) * 4).max().unwrap_or(0);
    let mut text = format!(
        "// Generated by host/hk-intro-cook from the user's Hollow Knight install.\n\
         pub const MOVIES: usize = {};\n\
         /// The most memory the SDK's player asks for any movie: what the scene arena has to lend.\n\
         pub const PLAYER_BYTES: usize = {player_bytes};\n",
        movies.len()
    );
    for (spec, m) in movies {
        text += &format!(
            "pub mod {} {{\n\
             \x20   // VideoClip {}, stream {}.\n\
             \x20   pub const FRAMES: u32 = {};\n\
             \x20   pub const WIDTH: u16 = {};\n\
             \x20   pub const HEIGHT: u16 = {};\n\
             \x20   pub const MAX_CHUNKS: u16 = {};\n\
             \x20   pub const RLE_WORDS: usize = {};\n\
             \x20   pub const XA: Option<(u8, u8)> = {:?};\n\
             \x20   pub const LATENCY_VBLANKS: u32 = {};\n\
             }}\n",
            spec.module,
            spec.name,
            spec.file,
            m.frames,
            m.width,
            m.height,
            m.max_chunks,
            m.rle_words,
            m.xa,
            LATENCY_VBLANKS
        );
    }
    text += &format!(
        "pub mod poem {{\n\
         \x20   // The piano under the poem, stream {POEM_FILE}.\n\
         \x20   pub const SECTORS: u32 = {};\n\
         \x20   pub const XA_FILE: u8 = {POEM_XA_FILE};\n\
         \x20   pub const XA_CHANNEL: u8 = {POEM_CHANNEL};\n\
         \x20   /// Milliseconds of silence ahead of the first note in the file.\n\
         \x20   pub const LEAD_IN_MS: u32 = {};\n\
         \x20   /// Milliseconds of piano after the lead-in.\n\
         \x20   pub const PIANO_MS: u32 = {};\n\
         }}\n",
        poem.sectors, poem.lead_in_ms, poem.piano_ms
    );
    text
}

/// What the intro program needs to know about the poem's sound file.
struct Poem {
    sectors: u32,
    lead_in_ms: u32,
    piano_ms: u32,
}

/// The piano of the poem as the sound-only XA file the drive plays at single speed: the install's
/// clip mastered as the movies' sound is (the same player path), [`POEM_LEAD_IN_S`] of silence in
/// front, one sector in [`POEM_STRIDE`].
fn encode_poem(psxavenc: &Path, clip: &Path, work: &Path) -> Result<(Vec<u8>, Mastering, Poem), String> {
    std::fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let (wide, mastered, coded) = (work.join("wide.xa"), work.join("sound.wav"), work.join("sound.xa"));
    pass(psxavenc, &["-t", "xa"], &[clip, &wide], "8")?;
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    let decoded = xa::decode_mono_37800(&read(&wide)?)?;
    if is_silent(&decoded) {
        return Err("the poem's piano is digital silence".into());
    }
    let (pcm, mastering) = master(&decoded)?;
    let lead = (POEM_LEAD_IN_S * SOUND_HZ) as usize;
    let mut padded = vec![0i16; lead];
    padded.extend_from_slice(&pcm);
    std::fs::write(&mastered, wav(&padded, SOUND_HZ as u32))
        .map_err(|e| format!("{}: {e}", mastered.display()))?;
    pass(psxavenc, &["-t", "xa"], &[&mastered, &coded], "4")?;
    let file = mux::sparse(&read(&coded)?, POEM_STRIDE, POEM_XA_FILE, POEM_CHANNEL)?;
    let poem = Poem {
        sectors: (file.len() / XA_SECTOR) as u32,
        lead_in_ms: (POEM_LEAD_IN_S * 1000.0) as u32,
        piano_ms: (pcm.len() as f64 / SOUND_HZ * 1000.0) as u32,
    };
    std::fs::remove_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    Ok((file, mastering, poem))
}

/// `name` on the command line: `explicit`, else the environment variable `variable`, else the `PATH`.
fn find_tool(name: &str, variable: &str, explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = explicit.or_else(|| std::env::var_os(variable).map(PathBuf::from)) {
        return path.is_file().then_some(path);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// The clip as the encoder should see it: 15 frames a second, 320x180 by an area average, reduced to the
/// 5 bits per channel the MDEC's 15-bit output keeps with an ordered dither, and padded with black to the
/// coded 320x192. The MDEC truncates to 5 bits without any dither of its own, so a smooth gradient decodes
/// as a few flat bands with contour rings (the lamp glows) and its channels' steps fall in different
/// places (olive and maroon patches in a dark blue sky); with the dither in the picture itself the
/// quantisation noise is a fixed fine pattern and the gradient reads as one. The clip's own (silent)
/// sound rides along as the encoder needs a sound track to leave its sound sectors in place.
fn prepare_picture(ffmpeg: &Path, movie: &Path, work: &Path) -> Result<PathBuf, String> {
    let (dithered, prepared) = (work.join("dithered.mkv"), work.join("prepared.mkv"));
    let run = |args: &[&std::ffi::OsStr]| -> Result<(), String> {
        let status = Command::new(ffmpeg)
            .args(["-loglevel", "error", "-y"])
            .args(args)
            .status()
            .map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
        status.success().then_some(()).ok_or_else(|| format!("{} failed: {status}", ffmpeg.display()))
    };
    let os = std::ffi::OsStr::new;
    let scale = format!(
        "fps={FPS},scale={WIDTH}:{PICTURE_HEIGHT}:flags=area,format=rgb555le"
    );
    run(&[
        os("-i"), movie.as_os_str(), os("-vf"), os(&scale), os("-sws_dither"), os("bayer"),
        os("-c:v"), os("ffv1"), os("-c:a"), os("pcm_s16le"), dithered.as_os_str(),
    ])?;
    let pad = format!(
        "format=rgb24,pad={WIDTH}:{HEIGHT}:0:{}:black",
        (u32::from(HEIGHT) - PICTURE_HEIGHT) / 2
    );
    run(&[
        os("-i"), dithered.as_os_str(), os("-vf"), os(&pad), os("-c:v"), os("ffv1"), os("-c:a"), os("copy"),
        prepared.as_os_str(),
    ])?;
    std::fs::remove_file(&dithered).map_err(|e| format!("{}: {e}", dithered.display()))?;
    Ok(prepared)
}

/// Runs the encoder with `args`.
fn run_psxavenc(psxavenc: &Path, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let status = Command::new(psxavenc)
        .arg("-q")
        .args(args)
        .status()
        .map_err(|e| format!("{}: {e}", psxavenc.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed: {status}", psxavenc.display()))
    }
}

/// The encoder as the XA options of one audio pass: mono 37.8 kHz, `bits`
/// per sample, filed under the movie's file and channel.
fn xa_args(bits: &'static str) -> Vec<std::ffi::OsString> {
    ["-f", "37800", "-c", "1", "-b", bits, "-F"]
        .iter()
        .map(std::ffi::OsString::from)
        .chain([
            XA_FILE.to_string().into(),
            "-C".into(),
            XA_CHANNEL.to_string().into(),
        ])
        .collect()
}

fn pass(psxavenc: &Path, head: &[&str], tail: &[&Path], bits: &'static str) -> Result<(), String> {
    let xa = xa_args(bits);
    let mut args: Vec<&std::ffi::OsStr> = head.iter().map(std::ffi::OsStr::new).collect();
    args.extend(xa.iter().map(|a| a.as_os_str()));
    args.extend(tail.iter().map(|p| p.as_os_str()));
    run_psxavenc(psxavenc, &args)
}

/// What the gain stage did to the sound, for the log.
#[derive(Debug)]
struct Mastering {
    source_lufs: f64,
    target_lufs: f64,
    gain_db: f64,
    result_lufs: f64,
    peak_dbfs: f64,
    limiter: limiter::Report,
}

/// Raises the sound by [`PLAYBACK_LOSS_DB`] over the install's own loudness,
/// through a look-ahead limiter so that it cannot clip, and returns the new
/// samples. The movie keeps about 17 dB between its loudness and its peak, so
/// the 0.9 dB the player's path loses fits under the ceiling untouched.
fn master(pcm: &[i16]) -> Result<(Vec<i16>, Mastering), String> {
    let x: Vec<f64> = pcm.iter().map(|s| f64::from(*s) / 32768.0).collect();
    let source = loudness::integrated(&x, SOUND_HZ).ok_or("the movie's sound is silent")?;
    let target = source + PLAYBACK_LOSS_DB;
    let gain = limiter::gain_for(&x, SOUND_HZ, target)
        .ok_or_else(|| format!("{target:.1} LUFS is out of reach for the movie's sound"))?;
    let (y, report) = limiter::apply(&x, gain, SOUND_HZ);
    let out: Vec<i16> = y
        .iter()
        .map(|v| (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect();
    let peak = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let result = loudness::integrated(&y, SOUND_HZ).ok_or("the mastered sound is silent")?;
    Ok((
        out,
        Mastering {
            source_lufs: source,
            target_lufs: target,
            gain_db: 20.0 * gain.log10(),
            result_lufs: result,
            peak_dbfs: 20.0 * peak.log10(),
            limiter: report,
        },
    ))
}

/// A 16-bit mono PCM WAV, the form the encoder reads without guessing.
fn wav(pcm: &[i16], hz: u32) -> Vec<u8> {
    let data = pcm.len() as u32 * 2;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&hz.to_le_bytes());
    w.extend_from_slice(&(hz * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for s in pcm {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

/// True when the decoded sound is exactly zero throughout: a clip whose audio
/// track is digital silence, which the encoder turns into all-zero samples.
fn is_silent(pcm: &[i16]) -> bool {
    pcm.iter().all(|s| *s == 0)
}

/// The movie as an STR with its sound beside the picture.
///
/// The encoder makes the picture (320x192, 15 frames a second, bitstream
/// version 2) and the sound twice. First the sound at 8 bits, which the cook
/// decodes to samples, loudens and limits ([`master`]); then those samples at
/// 4 bits, the form the drive plays. [`mux::interleave`] puts the two streams
/// back together with the sound [`AUDIO_DELAY_SECTORS`] later than the encoder
/// would place it.
///
/// A movie with no sound of its own (`sound` is None and the clip's track is
/// silent, as both of this install's are) has nothing to master. The encoder's
/// own sound sectors, which are silent, then stay where it put them, so that the stream keeps its 10 sectors a frame and the same
/// XA file and channel; the result carries no [`Mastering`].
///
/// Mono 37.8 kHz XA: the sound is stereo in the install and the encoder
/// mixes it down to one channel, which the drive plays to both speakers; the
/// sound then takes half the sectors a stereo stream would, which go to the
/// picture instead. The loudness the cook matches is that of the mono mix.
///
/// `work` holds the MP4 and, while the encoder runs, its intermediate files;
/// it is removed once the stream is built.
fn encode(
    psxavenc: &Path,
    ffmpeg: &Path,
    movie: &Path,
    sound: Option<&Path>,
    work: &Path,
) -> Result<(Vec<u8>, Option<Mastering>), String> {
    std::fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let size = format!("{WIDTH}x{HEIGHT}");
    let fps = FPS.to_string();
    let (picture, wide, mastered, coded) = (
        work.join("picture.str"),
        work.join("wide.xa"),
        work.join("sound.wav"),
        work.join("sound.xa"),
    );
    let prepared = prepare_picture(ffmpeg, movie, work)?;
    pass(
        psxavenc,
        &["-t", "str", "-v", "v2", "-s", &size, "-r", &fps, "-x", "2"],
        &[&prepared, &picture],
        "4",
    )?;
    // The sound comes from the install's separate AudioClip when the caller
    // decoded one (hk-cook intro-audio), else from the movie's own track.
    pass(psxavenc, &["-t", "xa"], &[sound.unwrap_or(movie), &wide], "8")?;
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    let decoded = xa::decode_mono_37800(&read(&wide)?)?;
    let (stream, mastering) = if is_silent(&decoded) {
        (read(&picture)?, None)
    } else {
        let (pcm, mastering) = master(&decoded)?;
        std::fs::write(&mastered, wav(&pcm, SOUND_HZ as u32))
            .map_err(|e| format!("{}: {e}", mastered.display()))?;
        pass(psxavenc, &["-t", "xa"], &[&mastered, &coded], "4")?;
        let stream = mux::interleave(&read(&picture)?, &read(&coded)?, AUDIO_DELAY_SECTORS)?;
        (stream, Some(mastering))
    };
    std::fs::remove_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    Ok((stream, mastering))
}

/// Where the install comes from.
enum Install {
    DataDir(PathBuf),
    Doctor(PathBuf),
}

struct Args {
    install: Install,
    out: PathBuf,
    psxavenc: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
    /// Folder of `<movie>.wav` files, the install's separate AudioClips
    /// (hk-cook intro-audio). Without it a movie's sound is its own track.
    audio_dir: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let (mut data_dir, mut doctor, mut out, mut psxavenc, mut ffmpeg, mut audio_dir) =
        (None, None, None, None, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--data-dir" => data_dir = Some(value()?),
            "--from-doctor" => doctor = Some(value()?),
            "--out" => out = Some(value()?),
            "--psxavenc" => psxavenc = Some(value()?),
            "--ffmpeg" => ffmpeg = Some(value()?),
            "--audio-dir" => audio_dir = Some(value()?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let usage = "usage: hk-intro-cook (--data-dir DIR | --from-doctor REPO) --out DIR \
                 [--psxavenc PATH] [--ffmpeg PATH] [--audio-dir DIR]";
    let install = match (data_dir, doctor) {
        (Some(dir), None) => Install::DataDir(dir),
        (None, Some(repo)) => Install::Doctor(repo),
        _ => return Err(usage.into()),
    };
    Ok(Args {
        install,
        out: out.ok_or(usage)?,
        psxavenc,
        ffmpeg,
        audio_dir,
    })
}

/// Opens the install; `Ok(None)` when there is none to open (which is not an
/// error for the caller).
fn open_install(install: &Install) -> Result<Option<hk_unity::Source>, String> {
    match install {
        Install::DataDir(dir) => {
            let has_exe = dir
                .parent()
                .is_some_and(|p| p.join("hollow_knight.exe").is_file());
            if !dir.is_dir() || !has_exe {
                eprintln!(
                    "hk-intro-cook: {} is not a hollow_knight_Data folder of a Windows install",
                    dir.display()
                );
                return Ok(None);
            }
            hk_unity::Source::new(dir)
                .map(Some)
                .map_err(|e| format!("{}: {e}", dir.display()))
        }
        Install::Doctor(repo) => match hk_unity::Source::from_doctor(repo) {
            Ok(source) => Ok(Some(source)),
            Err(hk_unity::Error::Io(path, e)) => {
                eprintln!("hk-intro-cook: no install selected: {path}: {e}");
                Ok(None)
            }
            Err(e) => Err(format!("{}: {e}", repo.display())),
        },
    }
}

/// Seconds of picture and of sound a cooked stream holds. A sector of 4-bit
/// mono 37.8 kHz XA carries 18 groups of 8 units of 28 samples.
fn durations(m: &Movie) -> (f64, f64) {
    let samples = f64::from(m.audio_sectors) * (18 * 8 * 28) as f64;
    (f64::from(m.frames) / f64::from(FPS), samples / SOUND_HZ)
}

fn run() -> Result<ExitCode, String> {
    let args = parse_args()?;
    let Some(source) = open_install(&args.install)? else {
        return Ok(ExitCode::from(NO_INTRO));
    };
    let clips = find_clips(&source)?;
    let mut chosen = Vec::new();
    for spec in &SPECS {
        match pick(&clips, spec.name)? {
            Some(clip) => {
                check_clip(spec, clip)?;
                chosen.push(clip);
            }
            None => {
                eprintln!(
                    "hk-intro-cook: no intro: the install has no VideoClip {}",
                    spec.name
                );
                return Ok(ExitCode::from(NO_INTRO));
            }
        }
    }
    let Some(psxavenc) = find_tool("psxavenc", "PSXAVENC", args.psxavenc) else {
        eprintln!("hk-intro-cook: psxavenc not found (--psxavenc, PSXAVENC or PATH)");
        return Ok(ExitCode::from(NO_INTRO));
    };
    let Some(ffmpeg) = find_tool("ffmpeg", "FFMPEG", args.ffmpeg) else {
        eprintln!("hk-intro-cook: ffmpeg not found (--ffmpeg, FFMPEG or PATH)");
        return Ok(ExitCode::from(NO_INTRO));
    };
    std::fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let mut cooked: Vec<(&Spec, Movie)> = Vec::new();
    let mut total_bytes = 0usize;
    for (spec, clip) in SPECS.iter().zip(chosen) {
        let (path, _) = source.resolve(&clip.source).map_err(|e| e.to_string())?;
        let resource = source.resource(&path).map_err(|e| e.to_string())?;
        let bytes = clip_slice(&resource, clip)?;
        let work = args.out.join(format!("work-{}", spec.module));
        std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
        let mp4 = work.join(format!("{}.mp4", spec.module));
        std::fs::write(&mp4, bytes).map_err(|e| format!("{}: {e}", mp4.display()))?;
        let sound = args.audio_dir.as_ref().map(|d| d.join(format!("{}.wav", spec.module)));
        if let Some(wav) = sound.as_deref().filter(|w| !w.is_file()) {
            return Err(format!("{}: no decoded sound at {}", spec.name, wav.display()));
        }
        let (mut raw, mastering) = encode(&psxavenc, &ffmpeg, &mp4, sound.as_deref(), &work)?;
        // Same movie, same bytes: the encoder leaves the unused tail of each
        // audio sector as whatever its memory held, which would make the disc
        // differ from one cook to the next.
        zero_sector_tails(&mut raw);
        let target = args.out.join(spec.file);
        std::fs::write(&target, &raw).map_err(|e| format!("{}: {e}", target.display()))?;
        let m = measure(&raw).map_err(|e| format!("{}: {e}", spec.name))?;
        let want = expected_frames(spec);
        if !length_is_plausible(spec, m.frames) {
            return Err(format!(
                "{}: {} frames at {FPS} fps, expected about {want} for the source's length",
                spec.name, m.frames
            ));
        }
        let words = player_words(&m);
        if words * 4 > PLAYER_MEMORY_LIMIT {
            return Err(format!(
                "{}: the player needs {} bytes, the scene arena lends {PLAYER_MEMORY_LIMIT}",
                spec.name,
                words * 4
            ));
        }
        eprintln!("hk-intro-cook: {}: the player needs {} bytes of the scene arena", spec.name, words * 4);
        let (video_s, audio_s) = durations(&m);
        if sound.is_some() {
            // The sound is the install's own clip, so it must really be there, and about as long
            // as the picture it plays with (the original starts both together).
            if mastering.is_none() {
                return Err(format!("{}: the decoded sound is digital silence", spec.name));
            }
            if (video_s - audio_s).abs() > MAX_SOUND_DRIFT_S {
                return Err(format!(
                    "{}: {audio_s:.2} s of sound against {video_s:.2} s of picture",
                    spec.name
                ));
            }
        }
        match &mastering {
            Some(mastering) => eprintln!(
                "hk-intro-cook: {}: sound {:.1} LUFS in the install's mono mix, {:+.1} dB gain to reach {:.1}, {:.1} LUFS and \
                 {:.1} dBFS peak after the limiter (at most {:.1} dB, {:.1}% of the time)",
                spec.name,
                mastering.source_lufs,
                mastering.gain_db,
                mastering.target_lufs,
                mastering.result_lufs,
                mastering.peak_dbfs,
                mastering.limiter.max_reduction_db,
                mastering.limiter.active * 100.0
            ),
            None => eprintln!(
                "hk-intro-cook: {}: NOTE the clip's audio track is digital silence; \
                 the stream carries the encoder's silent XA sectors and nothing was mastered",
                spec.name
            ),
        }
        eprintln!(
            "hk-intro-cook: {} -> {}: {} frames {}x{} ({video_s:.2} s of picture, {audio_s:.2} s of sound), \
             {} video + {} audio + {} other sectors ({} bytes), max {} chunks, max {} RLE words, \
             bitstream versions {:?}, XA {:?}",
            spec.name,
            spec.file,
            m.frames,
            m.width,
            m.height,
            m.video_sectors,
            m.audio_sectors,
            m.other_sectors,
            raw.len(),
            m.max_chunks,
            m.rle_words,
            m.versions,
            m.xa
        );
        total_bytes += raw.len();
        cooked.push((spec, m));
    }
    let audio_dir = args
        .audio_dir
        .as_ref()
        .ok_or("the poem's piano needs --audio-dir (hk-cook intro-audio)")?;
    let piano = audio_dir.join("excerpt.wav");
    if !piano.is_file() {
        return Err(format!("no decoded piano at {}", piano.display()));
    }
    let (poem_bytes, poem_mastering, poem) =
        encode_poem(&psxavenc, &piano, &args.out.join("work-poem"))?;
    let poem_target = args.out.join(POEM_FILE);
    std::fs::write(&poem_target, &poem_bytes).map_err(|e| format!("{}: {e}", poem_target.display()))?;
    eprintln!(
        "hk-intro-cook: the poem's piano: {:.1} LUFS in the install's mono mix, {:+.1} dB gain to reach {:.1}, {:.1} LUFS and \
         {:.1} dBFS peak after the limiter; {:.2} s of piano after {:.1} s of lead-in, {} sectors ({} bytes) in {POEM_FILE}",
        poem_mastering.source_lufs,
        poem_mastering.gain_db,
        poem_mastering.target_lufs,
        poem_mastering.result_lufs,
        poem_mastering.peak_dbfs,
        f64::from(poem.piano_ms) / 1000.0,
        POEM_LEAD_IN_S,
        poem.sectors,
        poem_bytes.len()
    );
    total_bytes += poem_bytes.len();
    let movies: Vec<(&Spec, &Movie)> = cooked.iter().map(|(s, m)| (*s, m)).collect();
    std::fs::write(args.out.join("movie.rs"), constants(&movies, &poem))
        .map_err(|e| format!("movie.rs: {e}"))?;
    // What the build checks to know the cook is current: the sound's origin.
    let origin = if args.audio_dir.is_some() { "install AudioClips" } else { "movie track" };
    std::fs::write(args.out.join("sound.json"), format!("{{\"sound\": \"{origin}\"}}\n"))
        .map_err(|e| format!("sound.json: {e}"))?;
    let sectors: u32 = cooked
        .iter()
        .map(|(_, m)| m.video_sectors + m.audio_sectors + m.other_sectors)
        .sum();
    eprintln!(
        "hk-intro-cook: both movies and the poem's piano: {sectors} movie sectors, {total_bytes} bytes ({:.1} MiB)",
        total_bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("hk-intro-cook: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn the_player_memory_is_what_the_sdk_asks_for() {
        // The 320x176 Prologue as cooked before the picture grew: 95,504 words.
        let m = Movie { width: 320, height: 176, max_chunks: 10, rle_words: 12_032, ..Movie::default() };
        assert_eq!(player_words(&m), 95_504);
    }

    #[test]
    fn a_stream_that_is_not_whole_sectors_is_refused() {
        assert!(measure(&[0u8; XA_SECTOR + 1]).is_err());
        assert!(measure(&[]).is_err());
    }

    #[test]
    fn only_what_follows_each_sectors_data_is_zeroed() {
        let mut raw = vec![0xAAu8; XA_SECTOR * 2];
        raw[2] = SUBMODE_AUDIO | SUBMODE_FORM2;
        raw[XA_SECTOR + 2] = 0;
        zero_sector_tails(&mut raw);
        // Audio sector: the sound groups stay, the tail goes.
        assert!(raw[AUDIO_TAIL].iter().all(|b| *b == 0));
        assert_eq!(raw[AUDIO_TAIL.start - 1], 0xAA);
        // Data sector: the user data stays, the tail goes.
        let second = &raw[XA_SECTOR..];
        assert!(second[FORM1_TAIL].iter().all(|b| *b == 0));
        assert_eq!(second[FORM1_TAIL.start - 1], 0xAA);
    }

    #[test]
    fn a_stream_without_video_is_refused() {
        // All-zero sectors are neither audio nor an STR chunk.
        assert!(measure(&vec![0u8; XA_SECTOR * 4]).is_err());
    }

    fn movie(frames: u32, audio_sectors: u32) -> Movie {
        Movie {
            frames,
            width: 320,
            height: HEIGHT,
            max_chunks: 9,
            rle_words: 20_000,
            xa: Some((1, 0)),
            audio_sectors,
            ..Movie::default()
        }
    }

    #[test]
    fn the_constants_name_every_figure_the_guest_sizes_by_for_each_movie() {
        let (first, second) = (movie(453, 0), movie(572, 0));
        let poem = Poem { sectors: 1186, lead_in_ms: 2000, piano_ms: 15_600 };
        let text = constants(&[(&SPECS[0], &first), (&SPECS[1], &second)], &poem);
        for want in [
            "pub const MOVIES: usize = 2;",
            "pub mod prologue {",
            "pub mod intro {",
            "VideoClip Prologue, stream HKINTRO1.STR",
            "VideoClip Intro, stream HKINTRO2.STR",
            "FRAMES: u32 = 453",
            "FRAMES: u32 = 572",
            "WIDTH: u16 = 320",
            "HEIGHT: u16 = 192",
            "MAX_CHUNKS: u16 = 9",
            "RLE_WORDS: usize = 20000",
            "XA: Option<(u8, u8)> = Some((1, 0))",
            "LATENCY_VBLANKS: u32 = 5",
            "pub mod poem {",
            "SECTORS: u32 = 1186",
            "XA_FILE: u8 = 2",
            "LEAD_IN_MS: u32 = 2000",
        ] {
            assert!(text.contains(want), "{want}\n{text}");
        }
        // The modules come in play order, and the generated file closes them.
        assert!(text.find("mod prologue").unwrap() < text.find("mod intro").unwrap());
        assert_eq!(text.matches('{').count(), text.matches('}').count());
    }

    fn map(fields: Vec<(&str, Value)>) -> Value {
        Value::Map(
            fields
                .into_iter()
                .map(|(k, v)| (Arc::<str>::from(k), v))
                .collect(),
        )
    }

    fn video_clip(name: &str, spec: &Spec) -> Value {
        map(vec![
            ("m_Name", Value::Str(name.as_bytes().to_vec())),
            ("Width", Value::UInt(1920)),
            ("Height", Value::UInt(1080)),
            ("m_FrameRate", Value::F64(spec.fps)),
            ("m_FrameCount", Value::UInt(spec.frames)),
            (
                "m_ExternalResources",
                map(vec![
                    ("m_Source", Value::Str(b"sharedassets5.resource".to_vec())),
                    ("m_Offset", Value::UInt(100)),
                    ("m_Size", Value::UInt(spec.size)),
                ]),
            ),
        ])
    }

    #[test]
    fn the_two_clips_are_found_by_name_whatever_order_or_company_they_come_in() {
        let clips: Vec<Clip> = [
            video_clip("Other", &SPECS[0]),
            video_clip("Intro", &SPECS[1]),
            video_clip("Prologue", &SPECS[0]),
        ]
        .iter()
        .map(|v| clip_from_value(v).unwrap())
        .collect();
        for spec in &SPECS {
            let clip = pick(&clips, spec.name).unwrap().unwrap();
            assert_eq!(clip.name, spec.name);
            assert_eq!(check_clip(spec, clip), Ok(()));
        }
        assert_eq!(pick(&clips, "Missing"), Ok(None));
        // Two clips of one name cannot be told apart.
        let mut twice = clips.clone();
        twice.push(clips[0].clone());
        assert!(pick(&twice, "Other").is_err());
    }

    #[test]
    fn a_value_without_the_clip_fields_is_not_a_clip() {
        assert!(clip_from_value(&map(vec![("m_Name", Value::Str(b"x".to_vec()))])).is_none());
        assert!(clip_from_value(&Value::Int(1)).is_none());
    }

    #[test]
    fn a_clip_that_differs_from_the_validated_one_is_refused_with_the_difference() {
        let mut clip = clip_from_value(&video_clip("Intro", &SPECS[1])).unwrap();
        assert_eq!(check_clip(&SPECS[1], &clip), Ok(()));
        // The same bytes under the wrong spec: size, frames and rate all differ.
        let e = check_clip(&SPECS[0], &clip).unwrap_err();
        assert!(
            e.contains("bytes") && e.contains("frames") && e.contains("fps"),
            "{e}"
        );
        clip.size_px = (1280, 720);
        assert!(check_clip(&SPECS[1], &clip)
            .unwrap_err()
            .contains("1280x720"));
    }

    #[test]
    fn the_cooked_length_is_the_sources_at_fifteen_frames_a_second() {
        // 30.2 s and 38.12 s.
        assert_eq!(expected_frames(&SPECS[0]), 453);
        assert_eq!(expected_frames(&SPECS[1]), 572);
    }

    #[test]
    fn the_encoders_few_frames_short_are_accepted_and_a_cut_or_overrun_is_not() {
        // What the encoder gave for the two clips, and the figures around it.
        assert!(length_is_plausible(&SPECS[0], 450));
        assert!(length_is_plausible(&SPECS[1], 569));
        assert!(length_is_plausible(&SPECS[0], 454));
        assert!(!length_is_plausible(&SPECS[0], 455));
        assert!(!length_is_plausible(&SPECS[0], 447));
        assert!(!length_is_plausible(&SPECS[1], 300));
    }

    #[test]
    fn a_clip_is_cut_out_of_its_resource_file_and_must_be_an_mp4() {
        let mut resource = vec![0u8; 64];
        resource[24..28].copy_from_slice(&[0, 0, 0, 0x18]);
        resource[28..32].copy_from_slice(b"ftyp");
        let mut clip = clip_from_value(&video_clip("Intro", &SPECS[1])).unwrap();
        (clip.offset, clip.size) = (24, 16);
        assert_eq!(clip_slice(&resource, &clip).unwrap(), &resource[24..40]);
        // Past the end, overflowing, and not an MP4 are all refused.
        clip.size = 41;
        assert!(clip_slice(&resource, &clip).is_err());
        clip.size = u64::MAX;
        assert!(clip_slice(&resource, &clip).is_err());
        (clip.offset, clip.size) = (0, 16);
        assert!(clip_slice(&resource, &clip).is_err());
    }

    #[test]
    fn only_exact_zeros_are_a_silent_track() {
        assert!(is_silent(&[0; 1000]));
        assert!(is_silent(&[]));
        let mut quiet = vec![0i16; 1000];
        quiet[500] = 1;
        assert!(!is_silent(&quiet));
    }
    #[test]
    fn picture_and_sound_lengths_follow_from_the_sector_counts() {
        // 150 frames at 15 fps is 10 s; 94 sound sectors of 4032 samples at
        // 37.8 kHz is 10.03 s.
        let (video, audio) = durations(&movie(150, 94));
        assert!((video - 10.0).abs() < 1e-9);
        assert!((audio - 10.03).abs() < 0.01, "{audio}");
    }
}
