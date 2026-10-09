//! What the audio cookers share: the SDK's psx-audio-cook command line
//! (encode, resample, plan, score), ffmpeg's mono resampler, WAV framing and
//! the SPU-ADPCM one-shot checks (host/spu_cook.py and the helpers of
//! host/cook_audio.py, which the Python audio cookers imported from each other).

use crate::common::{err, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// spu_cook.py `NOMINAL_RATE`: the encoder never resamples here, the PCM goes
/// in and out at one nominal rate.
pub const NOMINAL_RATE: u32 = 22050;

/// scene_bank.py / ambience.py `fnv`.
pub fn fnv(data: &[u8]) -> u32 {
    data.iter().fold(0x811c9dc5u32, |v, &b| (v ^ b as u32).wrapping_mul(0x01000193))
}

/// cook_audio.py `decode_oneshot`'s framing check: no flags but the silent
/// END terminator.
pub fn check_oneshot(bank: &[u8]) -> Result<()> {
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

const FILTERS: [(i32, i32); 5] = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60)];

/// spu_cook.py `decode`: ADPCM blocks decoded as the SPU decodes them
/// (clamped history, shift 13-15 read as 9), flags ignored.
pub fn decode(adpcm: &[u8]) -> Result<Vec<i32>> {
    if !adpcm.len().is_multiple_of(16) {
        return err("ADPCM block alignment");
    }
    let mut out = Vec::with_capacity(adpcm.len() / 16 * 28);
    let (mut s1, mut s2) = (0i32, 0i32);
    for block in adpcm.chunks_exact(16) {
        let (f1, f2) = FILTERS[(block[0] >> 4).min(4) as usize];
        let shift = block[0] & 15;
        let shift = if shift > 12 { 9 } else { shift };
        for &packed in &block[2..] {
            for nibble in [packed & 15, packed >> 4] {
                let signed = if nibble > 7 { nibble as i32 - 16 } else { nibble as i32 };
                let value = ((signed << 12) >> shift) + ((s1 * f1) >> 6) + ((s2 * f2) >> 6);
                let value = value.clamp(-32768, 32767);
                out.push(value);
                s2 = s1;
                s1 = value;
            }
        }
    }
    Ok(out)
}

/// cook_audio.py `decode_oneshot`: the framing check, then the samples of
/// everything but the terminator.
pub fn decode_oneshot(bank: &[u8]) -> Result<Vec<i32>> {
    check_oneshot(bank)?;
    decode(&bank[..bank.len() - 16])
}

/// ambience.py `validate_blocks`: whole ADPCM blocks with a zero initial
/// predictor and legal headers.
pub fn validate_blocks(data: &[u8]) -> Result<()> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return err("partial or empty ADPCM blocks");
    }
    if data[0] >> 4 != 0 {
        return err("initial ADPCM predictor must be zero");
    }
    if data.chunks_exact(16).any(|b| b[0] >> 4 > 4 || b[0] & 15 > 12) {
        return err("invalid ADPCM header");
    }
    Ok(())
}

/// ambience.py `validate_loop`: loop-start on the first block, loop-end and
/// repeat on the last, nothing else.
pub fn validate_loop(data: &[u8]) -> Result<()> {
    validate_blocks(data)?;
    for (i, block) in data.chunks_exact(16).enumerate() {
        let expected = (if i == 0 { 4 } else { 0 }) | (if (i + 1) * 16 == data.len() { 3 } else { 0 });
        if block[1] != expected {
            return err("invalid loop start/end flags");
        }
    }
    Ok(())
}

/// ambience.py `loop_payload`: flagless blocks made into a looping sample.
pub fn loop_payload(data: &[u8]) -> Result<Vec<u8>> {
    validate_blocks(data)?;
    if data.chunks_exact(16).any(|b| b[1] != 0) {
        return err("input contains transport flags");
    }
    let mut result = data.to_vec();
    result[1] = 4;
    let n = result.len();
    result[n - 15] |= 3;
    validate_loop(&result)?;
    Ok(result)
}

/// focus_audio.py `oneshot_payload`: flagless blocks and the silent END block.
pub fn oneshot_payload(data: &[u8]) -> Result<Vec<u8>> {
    validate_blocks(data)?;
    if data.chunks_exact(16).any(|b| b[1] != 0) {
        return err("one-shot input contains transport flags");
    }
    let mut out = data.to_vec();
    out.extend_from_slice(&[12, 1]);
    out.extend_from_slice(&[0; 14]);
    Ok(out)
}

// ---------------------------------------------------------------- WAV

pub struct Wav {
    pub channels: u16,
    pub rate: u32,
    pub width: u16,
    pub data: Vec<u8>,
}
/// What Python's `wave` module reads: the fmt chunk and the data chunk.
pub fn read_wav(bytes: &[u8]) -> Result<Wav> {
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
pub fn mono_wav(rate: u32, samples: &[i16]) -> Vec<u8> {
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
pub fn samples_of(data: &[u8]) -> Vec<i16> {
    data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

// ---------------------------------------------------------------- processes

pub fn run(cmd: &mut Command, input: Option<Vec<u8>>) -> Result<Vec<u8>> {
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

/// ffmpeg's polyphase resampler, folding to mono: `-ar rate -ac 1 -f s16le`
/// over a WAV on stdin (cook_audio.py `convert_wav`'s default resampler).
pub fn ffmpeg_mono(wav: &[u8], rate: i64) -> Result<Vec<i16>> {
    let out = run(Command::new("ffmpeg").args(["-v", "error", "-i", "pipe:0", "-ar", &rate.to_string(), "-ac", "1", "-f", "s16le", "pipe:1"]), Some(wav.to_vec()))?;
    Ok(samples_of(&out))
}

/// The SDK's cooker built from the pinned tree (spu_cook.py `binary`), with a
/// scratch directory for its files that goes away with the value.
pub struct Tool {
    pub binary: PathBuf,
    pub scratch: PathBuf,
}

impl Tool {
    pub fn build(root: &Path, scratch_name: &str) -> Result<Tool> {
        let crate_dir = root.join("tools/psx-audio-cook");
        let target = crate_dir.join("target");
        run(
            Command::new("cargo").args(["build", "-q", "--release", "--manifest-path"]).arg(crate_dir.join("Cargo.toml")).arg("--target-dir").arg(&target).current_dir(root),
            None,
        )?;
        let scratch = std::env::temp_dir().join(format!("{scratch_name}-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        Ok(Tool { binary: target.join("release/psx-audio-cook"), scratch })
    }

    /// spu_cook.py `encode_pcm(samples, 'none')` plus cook_audio.py `encode`'s
    /// silent terminator block: one-shot PSX ADPCM, every flag zero.
    pub fn encode_oneshot(&self, pcm: &[i16]) -> Result<Vec<u8>> {
        let mut out = if pcm.is_empty() {
            Vec::new()
        } else {
            let (input, output) = (self.scratch.join("in.wav"), self.scratch.join("out.adpcm"));
            std::fs::write(&input, mono_wav(NOMINAL_RATE, pcm)).map_err(|x| x.to_string())?;
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
        Ok(out)
    }

    /// spu_cook.py `encode_pcm(samples, mode)` for the loop modes `none`,
    /// `restart` and `whole` (a ring): ADPCM blocks with every flag zero and
    /// no terminator. Empty input gives empty output.
    pub fn encode(&self, pcm: &[i16], mode: &str) -> Result<Vec<u8>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
        if mode == "whole" && !pcm.len().is_multiple_of(28) {
            return err("a ring loop must be whole ADPCM blocks");
        }
        let (input, output) = (self.scratch.join("in.wav"), self.scratch.join("out.adpcm"));
        std::fs::write(&input, mono_wav(NOMINAL_RATE, pcm)).map_err(|x| x.to_string())?;
        run(
            Command::new(&self.binary)
                .arg("encode")
                .arg(&input)
                .arg(&output)
                .args(["--rate", &NOMINAL_RATE.to_string(), "--format", "raw", "--no-normalize", "--no-flags", "--loop", mode]),
            None,
        )?;
        let data = std::fs::read(&output).map_err(|x| x.to_string())?;
        if data.len() != pcm.len().div_ceil(28) * 16 {
            return err(format!("encoded {} bytes for {} samples", data.len(), pcm.len()));
        }
        Ok(data)
    }

    /// spu_cook.py `resample`: mono 16-bit PCM at `rate` through the SDK's
    /// shared resampler.
    pub fn resample(&self, wav: &[u8], rate: i64) -> Result<Vec<i16>> {
        let (input, output) = (self.scratch.join("resample-in.wav"), self.scratch.join("resample-out.wav"));
        std::fs::write(&input, wav).map_err(|x| x.to_string())?;
        run(Command::new(&self.binary).arg("resample").arg(&input).arg(&output).args(["--rate", &rate.to_string()]), None)?;
        let out = read_wav(&std::fs::read(&output).map_err(|x| x.to_string())?)?;
        if out.channels != 1 || out.width != 2 || out.rate as i64 != rate {
            return err("SDK resampler returned an unexpected WAV");
        }
        Ok(samples_of(&out.data))
    }

    /// The SDK rate allocator over a request file's text (`plan`).
    pub fn plan(&self, request: &str) -> Result<String> {
        let path = self.scratch.join("plan.txt");
        std::fs::write(&path, request).map_err(|x| x.to_string())?;
        String::from_utf8(run(Command::new(&self.binary).arg("plan").arg(&path), None)?).map_err(|x| x.to_string())
    }
}

impl Drop for Tool {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}
