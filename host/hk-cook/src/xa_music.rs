//! The Title, Boss Battle 1, Enemy Battle and Boss Defeat tracks as one
//! XA-ADPCM file, replacing the Red Book tracks host/title_music.py and the
//! fight half of host/area_music.py cooked.
//!
//! The four songs are the four channels of one 37.8 kHz stereo file at single
//! speed (`psx-audio-cook xa-encode`): the drive plays one sector in four, so
//! the file holds four songs for the disc a single CD-DA song would take. The
//! drive is busy while XA plays, which is how CD-DA was already used here: the
//! title and the fights read nothing while they play. Area music, which has to
//! survive room loads, stays on the SPU ring (docs/MUSIC.md).
//!
//! Each song loops at its own end, not at the file's: the guest hands the SDK
//! player a sector count of the song's span, so a short song never plays out
//! the silence the encoder pads it with. The longest song is followed by
//! `TAIL_SECONDS` of silence so the head is still inside the file when the
//! guest's once-a-second poll sees the song end.
//!
//! Writes data/music.xa (the raw 2336-byte sectors mkisopsx takes), data/xa_music.rs
//! (the guest's descriptors) and .hkpsx/xa-music.json (provenance and the
//! reference decoder's SNR per song).

use crate::common::{err, Result};
use hk_unity::{Source, Value};
use serde_json::{json, Value as J};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One song: where the source clip is and what it must be called.
pub struct Song {
    pub key: &'static str,
    pub file: &'static str,
    pub path_id: i64,
    pub clip: &'static str,
    /// The `music.rs` constant that names its channel.
    pub constant: &'static str,
}

/// Channel order is the order here: the guest's song numbers.
pub const SONGS: [Song; 4] = [
    Song {
        key: "title",
        file: "sharedassets1.assets",
        path_id: 94,
        clip: "Title",
        constant: "TITLE_TRACK",
    },
    Song {
        key: "boss",
        file: "sharedassets48.assets",
        path_id: 42,
        clip: "Boss Battle 1",
        constant: "BOSS_TRACK",
    },
    Song {
        key: "mawlek",
        file: "sharedassets40.assets",
        path_id: 28,
        clip: "S18 Enemy Battle-02 LOOP",
        constant: "MAWLEK_TRACK",
    },
    Song {
        key: "defeat",
        file: "sharedassets40.assets",
        path_id: 30,
        clip: "Boss Defeat",
        constant: "BOSS_DEFEAT_TRACK",
    },
];

/// Silence after the longest song, so a late poll still finds the head in the file.
pub const TAIL_SECONDS: u32 = 3;
/// The CD input gain the Red Book tracks played at: one third of full scale.
pub const GAIN: i32 = 10922;
/// The title's own start delay (`SceneManager.musicDelayTime` 1 s) in VBlanks.
pub const TITLE_DELAY_TICKS: u32 = 60;
/// XA-ADPCM 37.8 kHz stereo: samples per channel in one sector (18 groups of 4 blocks of 28).
const SECTOR_SAMPLES: usize = 2016;
const RATE: u64 = 37_800;
const STRIDE: u64 = 4;
const GUARD_SECTORS: u64 = 16;

/// A clip's decoded samples: interleaved 16-bit little endian.
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub data: Vec<u8>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.data.len() / (2 * self.channels as usize)
    }

    /// The samples of a canonical 16-bit PCM WAV (the decoder's output).
    pub fn parse(wav: &[u8]) -> Result<Pcm> {
        if wav.len() < 44
            || &wav[0..4] != b"RIFF"
            || &wav[8..12] != b"WAVE"
            || &wav[12..16] != b"fmt "
            || &wav[36..40] != b"data"
        {
            return err("not a canonical WAV");
        }
        let u16_at = |i: usize| u16::from_le_bytes([wav[i], wav[i + 1]]);
        if u16_at(20) != 1 || u16_at(34) != 16 {
            return err("not 16-bit PCM");
        }
        let size = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        let data = wav
            .get(44..44 + size)
            .ok_or("WAV data chunk runs past the file")?
            .to_vec();
        Ok(Pcm {
            rate: u32::from_le_bytes(wav[24..28].try_into().unwrap()),
            channels: u16_at(22),
            data,
        })
    }

    /// `seconds` of silence at the end.
    pub fn pad(&mut self, seconds: u32) {
        let frames = seconds as usize * self.rate as usize;
        self.data
            .resize(self.data.len() + frames * 2 * self.channels as usize, 0);
    }

    pub fn wav(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(44 + self.data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + self.data.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&(self.rate * self.channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(self.channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

/// An AudioClip's samples as the other cookers read them: FSB bank bytes
/// (inline or in a `.resS`), decoded by FMOD. Returns the WAV and the sha256
/// of the compressed bank, which is what identifies the source.
fn clip_wav(root: &Path, source: &Source, song: &Song) -> Result<(Vec<u8>, String)> {
    let file = source.file(song.file).map_err(|e| e.to_string())?;
    let obj = source
        .object(&file, song.path_id)
        .map_err(|e| e.to_string())?;
    let tree = source.read(&obj).map_err(|e| e.to_string())?;
    let name = tree.get("m_Name").and_then(Value::str).unwrap_or_default();
    if name != song.clip {
        return err(format!(
            "changed music mapping: {}:{} is {name}, not {}",
            song.file, song.path_id, song.clip
        ));
    }
    let data = match tree.get("m_AudioData") {
        Some(Value::Bytes(b)) if !b.is_empty() => b.clone(),
        _ => {
            let r = tree
                .get("m_Resource")
                .ok_or("AudioClip with neither m_AudioData nor m_Resource")?;
            let path = r.get("m_Source").and_then(Value::str).unwrap_or_default();
            let base = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
            let offset = r.get("m_Offset").and_then(Value::int).unwrap_or(0) as usize;
            let size = r.get("m_Size").and_then(Value::int).unwrap_or(0) as usize;
            let bytes = source
                .resource(&source.directory.join(&base))
                .map_err(|e| e.to_string())?;
            bytes
                .get(offset..offset + size)
                .ok_or("audio resource out of range")?
                .to_vec()
        }
    };
    let sha = hex(&Sha256::digest(&data));
    let channels = tree
        .get("m_Channels")
        .and_then(Value::int)
        .filter(|&c| c != 0)
        .unwrap_or(2) as i32;
    let frequency = tree
        .get("m_Frequency")
        .and_then(Value::int)
        .filter(|&c| c != 0)
        .unwrap_or(44100) as i32;
    Ok((
        crate::fmod::raw_to_wav(root, &data, channels, frequency)?,
        sha,
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Samples per channel the encoder keeps of a clip of `frames` frames at
/// `rate`: the length its resampler returns (psx_audio_cook::resample).
pub fn encoded_samples(frames: usize, rate: u32) -> u64 {
    (frames as u64 * RATE + rate as u64 / 2) / rate as u64
}

fn sectors_of(samples: u64) -> u64 {
    samples.div_ceil(SECTOR_SAMPLES as u64).max(1)
}

/// Where the songs sit in the file the encoder will write.
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    /// Sectors of the whole file, the end guard included.
    pub file_sectors: u64,
    /// Sectors from the file start to the end of each song's own audio.
    pub spans: Vec<u64>,
    /// The longest song, which the tail follows.
    pub longest: usize,
}

/// The layout for songs of `(frames, source rate)`: `tail_seconds` of silence
/// goes after the longest, and the encoder pads every other song to match.
pub fn layout(clips: &[(usize, u32)], tail_seconds: u32) -> Layout {
    let samples: Vec<u64> = clips.iter().map(|&(f, r)| encoded_samples(f, r)).collect();
    let longest = (0..samples.len())
        .max_by_key(|&i| (sectors_of(samples[i]), std::cmp::Reverse(i)))
        .unwrap_or(0);
    let total = sectors_of(samples[longest] + tail_seconds as u64 * RATE);
    Layout {
        file_sectors: total * STRIDE + GUARD_SECTORS,
        spans: samples.iter().map(|&s| sectors_of(s) * STRIDE).collect(),
        longest,
    }
}

/// data/xa_music.rs: what the guest needs to find and play each song.
pub fn rust_manifest(layout: &Layout, file_number: u8) -> String {
    let mut lines = vec![
        "// Generated by hk-cook xa-music; descriptors only, the songs are channels of MUSIC.XA on the disc.".to_string(),
        "/// One song of the XA file: its channel and the sectors from the file start to the end of its audio.".to_string(),
        "#[derive(Clone,Copy)] pub struct XaSong {pub channel:u8,pub span:u32}".to_string(),
        "pub const XA_NAME:&str=\"MUSIC.XA\";".to_string(),
        format!("pub const XA_FILE_NUMBER:u8={file_number};"),
        format!("pub const XA_SECTORS:u32={};", layout.file_sectors),
        format!("pub const TITLE_DELAY:u32={TITLE_DELAY_TICKS};"),
        format!("pub const TITLE_GAIN:i16={GAIN};"),
        format!("pub const BOSS_GAIN:i16={GAIN};"),
    ];
    for (channel, song) in SONGS.iter().enumerate() {
        lines.push(format!("pub const {}:u8={channel};", song.constant));
    }
    lines.push(format!("pub const XA_SONGS:[XaSong;{}]=[", SONGS.len()));
    for (channel, span) in layout.spans.iter().enumerate() {
        lines.push(format!("    XaSong{{channel:{channel},span:{span}}},"));
    }
    lines.push("];".to_string());
    lines.join("\n") + "\n"
}

fn sha_file(path: &Path) -> Result<String> {
    Ok(hex(&Sha256::digest(
        std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
    )))
}

/// Where the SDK is exported from: `HK_SDK_SOURCE`, else the PSoXide checkout beside this one.
fn sdk_source(root: &Path) -> PathBuf {
    std::env::var_os("HK_SDK_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.parent().unwrap_or(root).join("PSoXide"))
}

/// The SDK crates the XA encoder builds from, at the revision xa-encoder.lock.json
/// names. The guest's own pin (sdk.lock.json) predates
/// the XA encoder and cannot move until the build tools that read the SDK's
/// linker script and hazard patcher are ported to its new layout, so the
/// encoder comes from a second, host-only export under .hkpsx/xa-encoder,
/// made once per revision and never edited.
fn encoder_tree(root: &Path) -> Result<PathBuf> {
    let pin: J = serde_json::from_str(
        &std::fs::read_to_string(root.join("xa-encoder.lock.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let revision = pin["revision"]
        .as_str()
        .ok_or("xa-encoder.lock.json lacks revision")?;
    let tree = root.join(".hkpsx/xa-encoder").join(revision);
    if tree.join("crates/psx-audio-cook/Cargo.toml").is_file() {
        return Ok(tree);
    }
    let partial = root
        .join(".hkpsx/xa-encoder")
        .join(format!("{revision}.partial"));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    let tar = partial.join("sdk.tar");
    let source = sdk_source(root);
    let status = Command::new("git")
        .arg("-C")
        .arg(&source)
        .args(["archive", revision, "-o"])
        .arg(&tar)
        .args(["Cargo.toml", "Cargo.lock", "crates", "tools"])
        .status()
        .map_err(|e| format!("git: {e}"))?;
    if !status.success() {
        return err(format!(
            "git archive {revision} failed in {}: pass HK_SDK_SOURCE",
            source.display()
        ));
    }
    if !Command::new("tar")
        .arg("-xf")
        .arg(&tar)
        .arg("-C")
        .arg(&partial)
        .status()
        .map_err(|e| format!("tar: {e}"))?
        .success()
    {
        return err("tar failed");
    }
    std::fs::remove_file(&tar).map_err(|e| e.to_string())?;
    // The crate inherits the SDK workspace's package fields, so the wrapper
    // that gives it a command line is its own workspace beside it.
    std::fs::create_dir_all(partial.join("cli")).map_err(|e| e.to_string())?;
    std::fs::write(
        partial.join("cli/Cargo.toml"),
        "[package]\nname = \"hk-xa-encode\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n\n[[bin]]\nname = \"xa-encode\"\npath = \"main.rs\"\n\n[dependencies]\npsx-audio-cook = { path = \"../crates/psx-audio-cook\" }\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(partial.join("cli/main.rs"), "fn main() -> std::process::ExitCode {\n    let args: Vec<String> = std::env::args().skip(1).collect();\n    psx_audio_cook::cli::run(&args)\n}\n").map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &tree).map_err(|e| e.to_string())?;
    Ok(tree)
}

/// The SDK's `psx-audio-cook` command line, built from the encoder tree.
fn encoder(root: &Path) -> Result<PathBuf> {
    let tree = encoder_tree(root)?;
    let target = tree.join("target");
    let status = Command::new("cargo")
        .args(["build", "-q", "--release", "--manifest-path"])
        .arg(tree.join("cli/Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return err("building the XA encoder failed");
    }
    Ok(target.join("release/xa-encode"))
}

fn run(binary: &Path, args: &[&std::ffi::OsStr]) -> Result<String> {
    let out = Command::new(binary)
        .args(args)
        .output()
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    if !out.status.success() {
        return err(format!(
            "psx-audio-cook {:?} failed: {}",
            args.first(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The reference decoder's SNR in dB, left and right, of one channel against its source WAV.
fn score(binary: &Path, wav: &Path, xa: &Path, channel: usize) -> Result<Vec<f64>> {
    let channel = channel.to_string();
    let text = run(
        binary,
        &[
            "xa-score".as_ref(),
            wav.as_os_str(),
            xa.as_os_str(),
            channel.as_ref(),
        ],
    )?;
    let value: J =
        serde_json::from_str(text.trim()).map_err(|e| format!("xa-score output: {e}"))?;
    value["snr_db"]
        .as_array()
        .ok_or("xa-score without snr_db")?
        .iter()
        .map(|v| v.as_f64().ok_or_else(|| "bad snr".to_string()))
        .collect()
}

/// Cook the XA file and its descriptors, unless the inputs and outputs of the
/// last cook are intact.
pub fn cook(root: &Path, source: &Source) -> Result<String> {
    let (data, report_path) = (root.join("data"), root.join(".hkpsx/xa-music.json"));
    let (xa_path, manifest_path) = (data.join("music.xa"), data.join("xa_music.rs"));
    let binary = encoder(root)?;
    let mut clips = Vec::new();
    let mut sources = Vec::new();
    for song in &SONGS {
        let (wav, sha) = clip_wav(root, source, song)?;
        let pcm = Pcm::parse(&wav)?;
        if pcm.channels != 2 {
            return err(format!("{} is not stereo", song.clip));
        }
        sources.push((sha, pcm.rate, pcm.frames()));
        clips.push(pcm);
    }
    let layout = layout(
        &clips
            .iter()
            .map(|c| (c.frames(), c.rate))
            .collect::<Vec<_>>(),
        TAIL_SECONDS,
    );
    let key = hex(&Sha256::digest(
        json!({"sources": sources.iter().map(|s| &s.0).collect::<Vec<_>>(), "encoder": sha_file(&binary)?, "code": hex(&Sha256::digest(include_bytes!("xa_music.rs")))})
            .to_string(),
    ));
    if let Ok(old) = std::fs::read_to_string(&report_path) {
        let old: J = serde_json::from_str(&old).map_err(|e| e.to_string())?;
        if old["key"] == key.as_str()
            && xa_path.is_file()
            && manifest_path.is_file()
            && sha_file(&xa_path)? == old["sha256"].as_str().unwrap_or("")
            && sha_file(&manifest_path)? == old["manifest_sha256"].as_str().unwrap_or("")
        {
            return Ok("XA music cache verified".into());
        }
    }
    let work = root.join(".hkpsx/xa-music");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    clips[layout.longest].pad(TAIL_SECONDS);
    let mut wavs = Vec::new();
    for (song, clip) in SONGS.iter().zip(&clips) {
        let path = work.join(format!("{}.wav", song.key));
        std::fs::write(&path, clip.wav()).map_err(|e| e.to_string())?;
        wavs.push(path);
    }
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let manifest = work.join("encoder.json");
    let mut args: Vec<&std::ffi::OsStr> = vec!["xa-encode".as_ref(), xa_path.as_os_str()];
    args.extend(wavs.iter().map(|w| w.as_os_str()));
    args.extend([
        "--file".as_ref(),
        "1".as_ref(),
        "--manifest".as_ref(),
        manifest.as_os_str(),
    ]);
    run(&binary, &args)?;
    check_encoder(
        &std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?,
        &layout,
    )?;
    let mut rows = Vec::new();
    for (channel, (song, wav)) in SONGS.iter().zip(&wavs).enumerate() {
        let snr = score(&binary, wav, &xa_path, channel)?;
        let (sha, rate, frames) = &sources[channel];
        rows.push(json!({"key": song.key, "clip": song.clip, "source": format!("{}:{}", song.file, song.path_id), "source_sha256": sha,
            "source_rate": rate, "source_seconds": *frames as f64 / *rate as f64, "channel": channel,
            "span_sectors": layout.spans[channel], "seconds": layout.spans[channel] as f64 / 75.0, "snr_db": snr}));
    }
    std::fs::write(&manifest_path, rust_manifest(&layout, 1)).map_err(|e| e.to_string())?;
    let report = json!({"format": "hk-xa-music-v1", "key": key, "encoder_sha256": sha_file(&binary)?, "file": "data/music.xa",
        "sha256": sha_file(&xa_path)?, "bytes": std::fs::metadata(&xa_path).map_err(|e| e.to_string())?.len(), "sectors": layout.file_sectors,
        "manifest_sha256": sha_file(&manifest_path)?, "sample_rate": RATE, "stereo": true, "speed": 1, "stride": STRIDE,
        "tail_seconds": TAIL_SECONDS, "songs": rows});
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "XA music: {} songs, {} sectors ({} bytes of raw sectors)",
        SONGS.len(),
        layout.file_sectors,
        layout.file_sectors * 2336
    ))
}

/// The encoder's own manifest must agree with the layout the guest is given.
fn check_encoder(text: &str, layout: &Layout) -> Result<()> {
    let m: J = serde_json::from_str(text).map_err(|e| format!("encoder manifest: {e}"))?;
    let ok = m["sectors"].as_u64() == Some(layout.file_sectors)
        && m["stride"].as_u64() == Some(STRIDE)
        && m["sample_rate"].as_u64() == Some(RATE)
        && m["songs"]
            .as_array()
            .is_some_and(|s| s.len() == layout.spans.len());
    if ok {
        Ok(())
    } else {
        err(format!(
            "the encoder's file ({text}) is not the layout {layout:?}"
        ))
    }
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

    #[test]
    fn the_resampler_length_rounds_half_up() {
        assert_eq!(encoded_samples(44_100, 44_100), 37_800);
        assert_eq!(encoded_samples(7, 44_100), 6);
        // 48 kHz: 81.8 s is 3,926,400 frames, and 37,800 / 48,000 is 63 / 80 (0.7875).
        assert_eq!(encoded_samples(3_926_400, 48_000), 3_092_040);
    }

    #[test]
    fn a_song_is_whole_sectors_and_one_stride_apart() {
        // 2016 samples fill one song sector; one more takes a second.
        let l = layout(&[(2016 * 100 * 7 / 6, 44_100)], 0);
        assert_eq!(l.spans, [100 * STRIDE]);
        assert_eq!(l.file_sectors, 100 * STRIDE + GUARD_SECTORS);
        let l = layout(&[(44_100 * 2, 44_100)], 0);
        assert_eq!(l.spans, [38 * STRIDE]); // 75,600 samples is 37.5 sectors
    }

    #[test]
    fn the_longest_song_gets_the_tail_and_the_rest_are_padded_to_it() {
        let l = layout(
            &[
                (44_100 * 10, 44_100),
                (44_100 * 40, 44_100),
                (48_000 * 5, 48_000),
            ],
            3,
        );
        assert_eq!(l.longest, 1);
        let total = (40 + 3) * 37_800_u64;
        assert_eq!(
            l.file_sectors,
            total.div_ceil(2016) * STRIDE + GUARD_SECTORS
        );
        assert_eq!(l.spans[1], (40 * 37_800_u64).div_ceil(2016) * STRIDE);
        assert!(l.spans[0] < l.spans[1] && l.spans[2] < l.spans[0]);
    }

    #[test]
    fn a_tie_picks_the_first_song() {
        assert_eq!(layout(&[(44_100, 44_100), (44_100, 44_100)], 1).longest, 0);
    }

    #[test]
    fn the_manifest_names_every_channel_and_span() {
        let l = Layout {
            file_sectors: 1000,
            spans: vec![40, 80, 60, 20],
            longest: 1,
        };
        let text = rust_manifest(&l, 1);
        for line in [
            "pub const XA_SECTORS:u32=1000;",
            "pub const TITLE_TRACK:u8=0;",
            "pub const BOSS_TRACK:u8=1;",
            "pub const MAWLEK_TRACK:u8=2;",
            "pub const BOSS_DEFEAT_TRACK:u8=3;",
            "XaSong{channel:1,span:80},",
        ] {
            assert!(text.contains(line), "{line} missing from\n{text}");
        }
    }

    #[test]
    fn pcm_round_trips_through_a_wav_and_pads_with_silence() {
        let mut pcm = Pcm {
            rate: 8000,
            channels: 2,
            data: vec![1, 0, 2, 0, 3, 0, 4, 0],
        };
        let back = Pcm::parse(&pcm.wav()).unwrap();
        assert_eq!((back.rate, back.channels, back.frames()), (8000, 2, 2));
        assert_eq!(back.data, pcm.data);
        pcm.pad(1);
        assert_eq!(pcm.frames(), 2 + 8000);
        assert!(pcm.data[8..].iter().all(|&b| b == 0));
        assert!(Pcm::parse(&pcm.wav()[..40]).is_err());
    }

    #[test]
    fn the_encoder_manifest_must_match_the_layout() {
        let l = Layout {
            file_sectors: 1000,
            spans: vec![40, 80, 60, 20],
            longest: 1,
        };
        let good = r#"{"file":1,"stereo":true,"sample_rate":37800,"drive_speed":1,"stride":4,"song_sectors":246,"sectors":1000,"songs":[{},{},{},{}]}"#;
        assert!(check_encoder(good, &l).is_ok());
        assert!(check_encoder(&good.replace("1000", "1004"), &l).is_err());
        assert!(check_encoder(&good.replace("\"stride\":4", "\"stride\":8"), &l).is_err());
    }
}
