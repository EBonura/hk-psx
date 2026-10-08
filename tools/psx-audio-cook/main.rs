//! `psx-audio-cook` built from the pinned SDK (see Cargo.toml), plus `plan`,
//! which runs the SDK's rate allocator for host/scene_sfx.py.
//!
//! ```text
//! psx-audio-cook plan REQUEST
//! psx-audio-cook resample IN.wav OUT.wav --rate HZ
//! ```
//!
//! `resample` folds the source to mono and runs the SDK's shared resampler
//! (`psx_audio_cook::resample::Sinc`, Kaiser-windowed sinc, low-passed before
//! it downsamples) to `round(len * HZ / source rate)` samples, written as a
//! 16-bit mono WAV, so a Python cooker can hand the SDK's resampled PCM to the
//! encoder instead of resampling with ffmpeg.
//!
//! REQUEST is tab-separated text. The first line is `budget<TAB>BYTES`; every
//! other line is one sound:
//! `WEIGHT<TAB>MAX_STEP<TAB>SOURCE.wav<TAB>RATE,RATE,...<TAB>BYTES,BYTES,...`
//! with its candidate rates high to low and the bytes each one costs. The
//! loss of each rate is `psx_audio_cook::rate::band_loss` on the source, and
//! `psx_audio_cook::rate::allocate` picks one rate per sound so the bytes fit
//! the budget at the least weighted loss. Prints `none` when even every
//! sound's lowest allowed rate does not fit, otherwise one line per sound:
//! `STEP<TAB>LOSS,LOSS,...`.

use psx_audio_cook::rate::{allocate, band_loss, Candidate};
use psx_audio_cook::resample::{to_i16, Sinc};
use std::process::ExitCode;

fn plan(path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines = text.lines();
    let budget: usize = lines
        .next()
        .and_then(|l| l.strip_prefix("budget\t"))
        .and_then(|v| v.trim().parse().ok())
        .ok_or("first line must be budget<TAB>BYTES")?;
    let list = |field: &str| -> Result<Vec<usize>, String> {
        field.split(',').map(|v| v.trim().parse().map_err(|_| format!("bad number {v}"))).collect()
    };
    let mut cands = Vec::new();
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [weight, max_step, source, rates, bytes] = fields[..] else {
            return Err(format!("expected five fields: {line}"));
        };
        let weight: f64 = weight.parse().map_err(|_| format!("bad weight {weight}"))?;
        let max_step: usize = max_step.parse().map_err(|_| format!("bad step {max_step}"))?;
        let rates: Vec<u32> = list(rates)?.into_iter().map(|r| r as u32).collect();
        let bytes = list(bytes)?;
        if rates.is_empty() || rates.len() != bytes.len() {
            return Err(format!("rates and bytes differ in length: {line}"));
        }
        let data = std::fs::read(source).map_err(|e| format!("{source}: {e}"))?;
        let wav = psx_audio_cook::wav::read(&data).map_err(|e| format!("{source}: {e}"))?;
        let loss = band_loss(&wav.samples, wav.rate, &rates);
        cands.push(Candidate { bytes, loss, weight, max_step });
    }
    match allocate(&cands, 0, budget) {
        None => println!("none"),
        Some(steps) => {
            for (c, step) in cands.iter().zip(steps) {
                let loss: Vec<String> = c.loss.iter().map(|l| format!("{l:.3}")).collect();
                println!("{step}\t{}", loss.join(","));
            }
        }
    }
    Ok(())
}

fn resample(input: &str, output: &str, rate: u32) -> Result<(), String> {
    let data = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let wav = psx_audio_cook::wav::read(&data).map_err(|e| format!("{input}: {e}"))?;
    if rate == 0 {
        return Err("rate must be positive".into());
    }
    let out = if wav.rate == rate { wav.samples.clone() } else { Sinc::new().resample(&wav.samples, wav.rate, rate) };
    std::fs::write(output, psx_audio_cook::wav::write_mono16(rate, &to_i16(&out))).map_err(|e| format!("{output}: {e}"))?;
    println!("{{\"source_rate\":{},\"rate\":{},\"source_samples\":{},\"samples\":{}}}", wav.rate, rate, wav.samples.len(), out.len());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("plan") {
        if args.len() != 2 {
            eprintln!("usage: psx-audio-cook plan REQUEST");
            return ExitCode::from(2);
        }
        return match plan(&args[1]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.first().map(String::as_str) == Some("resample") {
        let rate = match (args.len(), args.get(3).map(String::as_str), args.get(4).and_then(|v| v.parse().ok())) {
            (5, Some("--rate"), Some(rate)) => rate,
            _ => {
                eprintln!("usage: psx-audio-cook resample IN.wav OUT.wav --rate HZ");
                return ExitCode::from(2);
            }
        };
        return match resample(&args[1], &args[2], rate) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    psx_audio_cook::cli::run(&args)
}
