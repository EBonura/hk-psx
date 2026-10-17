//! Per-clip bandwidth need: decode each clip with FMOD (the cook's own decoder),
//! fold to mono and measure the share of energy above each ladder rate's Nyquist.
//!
//! The criterion is a necessary condition, not a verdict: a rate whose out-of-band
//! energy is at most -15 / -20 / -30 / -40 dB of the clip's total keeps that much of it. The SDK rate allocator (psx_audio_cook::rate) stays the
//! authority when a bank is cooked; this table tells how low it can usefully go.

use crate::csv::tsv;
use hk_unity::Source;
use rayon::prelude::*;
use std::fmt::Write as _;
use std::path::Path;

/// The SDK allocator's ladder, high to low (psx_audio_cook::rate::LADDER).
pub const LADDER: [i64; 20] = [
    22050, 18900, 16000, 13000, 11025, 10000, 9000, 8000, 7000, 6000, 5500, 5000, 4500, 4000, 3600,
    3200, 2800, 2400, 2000, 1600,
];

/// 16 bytes hold 28 samples; the rate is capped at the source's own.
pub fn adpcm_bytes(seconds: f64, rate: i64, source_rate: i64) -> u64 {
    let r = rate.min(source_rate).max(1);
    ((seconds * r as f64).ceil() as u64).div_ceil(28) * 16
}

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

pub struct Measure {
    pub peak_db: f64,
    pub rms_db: f64,
    /// Frequency below which 99 %, 99.9 %, 99.99 % of the energy lies.
    pub f: [f64; 3],
    /// Energy above each ladder rate's Nyquist, dB relative to the total.
    pub above_db: Vec<f64>,
    /// The same at 5,512 Hz, the port's one-shot rate (not a ladder step).
    pub above_port_db: f64,
}

/// Mono samples (float, -1..1) at `rate`.
pub fn measure(mono: &[f32], rate: f64) -> Measure {
    let peak = mono.iter().fold(0f32, |m, s| m.max(s.abs())) as f64;
    let rms =
        (mono.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / mono.len().max(1) as f64).sqrt();
    let db = |x: f64| if x > 0.0 { 20.0 * x.log10() } else { -200.0 };
    let n = mono.len().max(2).next_power_of_two().max(1024);
    let mut re = vec![0f64; n];
    let mut im = vec![0f64; n];
    let m = mono.len();
    for (i, s) in mono.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (i as f64 + 0.5) / m as f64).cos();
        re[i] = *s as f64 * w;
    }
    fft(&mut re, &mut im);
    let half = n / 2;
    let power: Vec<f64> = (0..=half).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
    let total: f64 = power.iter().sum();
    let bin_hz = rate / n as f64;
    let above = |hz: f64| -> f64 {
        if total <= 0.0 {
            return -200.0;
        }
        let from = ((hz / bin_hz).floor() as usize + 1).min(half + 1);
        let e: f64 = power[from..].iter().sum();
        if e <= 0.0 {
            -200.0
        } else {
            10.0 * (e / total).log10()
        }
    };
    let mut f = [0.0; 3];
    if total > 0.0 {
        let mut acc = 0.0;
        let (mut i99, mut i999, mut i9999) = (None, None, None);
        for (k, p) in power.iter().enumerate() {
            acc += p;
            let r = acc / total;
            if i99.is_none() && r >= 0.99 {
                i99 = Some(k);
            }
            if i999.is_none() && r >= 0.999 {
                i999 = Some(k);
            }
            if i9999.is_none() && r >= 0.9999 {
                i9999 = Some(k);
            }
        }
        f = [
            i99.unwrap_or(half) as f64 * bin_hz,
            i999.unwrap_or(half) as f64 * bin_hz,
            i9999.unwrap_or(half) as f64 * bin_hz,
        ];
    }
    Measure {
        peak_db: db(peak),
        rms_db: db(rms),
        f,
        above_db: LADDER.iter().map(|r| above(*r as f64 / 2.0)).collect(),
        above_port_db: above(5512.0 / 2.0),
    }
}

/// The lowest ladder rate whose out-of-band energy stays at or under `limit_db`,
/// never above the clip's own rate.
pub fn min_rate(above_db: &[f64], limit_db: f64, source_rate: i64) -> i64 {
    let mut best = LADDER[0].min(source_rate);
    for (r, a) in LADDER.iter().zip(above_db) {
        if *r > source_rate {
            continue;
        }
        if *a <= limit_db {
            best = *r;
        } else {
            break;
        }
    }
    best
}

pub fn run(dir: &str, census: &str, out: &str, max_s: Option<&str>) -> Result<(), String> {
    let max_s: f64 = max_s
        .map(|s| s.parse().map_err(|_| "max seconds"))
        .transpose()?
        .unwrap_or(20.0);
    let rows = tsv(&Path::new(census).join("clips.tsv"))?;
    let source = Source::new(dir).map_err(|e| e.to_string())?;
    let todo: Vec<&Vec<String>> = rows
        .iter()
        .filter(|r| r.len() >= 8 && r[4].parse::<f64>().unwrap_or(0.0) <= max_s && r[7] != "Music")
        .collect();
    eprintln!("{} clips up to {max_s} s", todo.len());
    let done = std::sync::atomic::AtomicUsize::new(0);
    let lines: Vec<Result<String, String>> = todo
        .par_iter()
        .map(|r| {
            let (key, name, rate, secs) = (&r[0], &r[1], r[3].parse::<i64>().unwrap_or(44100), r[4].parse::<f64>().unwrap_or(0.0));
            let (file, pid) = key.split_once(':').ok_or("bad key")?;
            let f = source.file(file).map_err(|e| e.to_string())?;
            let obj = source.object(&f, pid.parse::<i64>().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let tree = source.read(&obj).map_err(|e| e.to_string())?;
            let wav = hk_cook::fmod::clip_wav(Path::new("."), &source, &tree).map_err(|e| e.to_string())?;
            let w = hk_cook::spu::read_wav(&wav).map_err(|e| e.to_string())?;
            let ch = w.channels.max(1) as usize;
            let s16 = hk_cook::spu::samples_of(&w.data);
            let mono: Vec<f32> = s16
                .chunks(ch)
                .map(|c| c.iter().map(|v| *v as f32).sum::<f32>() / ch as f32 / 32768.0)
                .collect();
            let m = measure(&mono, w.rate as f64);
            let r15 = min_rate(&m.above_db, -15.0, rate);
            let r20 = min_rate(&m.above_db, -20.0, rate);
            let r30 = min_rate(&m.above_db, -30.0, rate);
            let r40 = min_rate(&m.above_db, -40.0, rate);
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n.is_multiple_of(200) {
                eprintln!("  {n}/{}", todo.len());
            }
            Ok(format!(
                "{key}\t{name}\t{secs:.3}\t{rate}\t{:.1}\t{:.1}\t{:.0}\t{:.0}\t{:.0}\t{r15}\t{r20}\t{r30}\t{r40}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                m.peak_db,
                m.rms_db,
                m.f[0],
                m.f[1],
                m.f[2],
                adpcm_bytes(secs, 22050, rate),
                adpcm_bytes(secs, 5512, rate),
                adpcm_bytes(secs, r15, rate),
                adpcm_bytes(secs, r20, rate),
                adpcm_bytes(secs, r30, rate),
                adpcm_bytes(secs, r40, rate),
                [11025i64, 8000, 4000]
                    .iter()
                    .map(|r| format!("{:.1}", m.above_db[LADDER.iter().position(|x| x == r).expect("on the ladder")]))
                    .chain(std::iter::once(format!("{:.1}", m.above_port_db)))
                    .collect::<Vec<_>>()
                    .join(";"),
            ))
        })
        .collect();
    let mut text = String::from("# key\tname\tseconds\tsrc_rate\tpeak_dB\trms_dB\tf99_hz\tf999_hz\tf9999_hz\tmin_rate_m15\tmin_rate_m20\tmin_rate_m30\tmin_rate_m40\tB@22050\tB@5512\tB@min15\tB@min20\tB@min30\tB@min40\tabove_dB@11025;8000;4000;5512\n");
    let mut failed = 0;
    for l in lines {
        match l {
            Ok(l) => {
                let _ = writeln!(text, "{l}");
            }
            Err(e) => {
                failed += 1;
                eprintln!("skipped: {e}");
            }
        }
    }
    std::fs::write(out, text).map_err(|e| e.to_string())?;
    eprintln!("wrote {out} ({failed} skipped)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_has_no_energy_above_its_nyquist() {
        let rate = 44100.0;
        let tone: Vec<f32> = (0..22050)
            .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / rate).sin() as f32 * 0.5)
            .collect();
        let m = measure(&tone, rate);
        // 1 kHz lies under every ladder Nyquist down to 2 kHz
        assert!(m.above_db[LADDER.len() - 3] < -60.0, "{:?}", m.above_db);
        assert!(m.f[1] < 1500.0, "{:?}", m.f);
        assert_eq!(min_rate(&m.above_db, -40.0, 44100), 2400);
    }

    #[test]
    fn noise_needs_the_top_of_the_ladder() {
        let mut x = 12345u32;
        let noise: Vec<f32> = (0..22050)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 16) as f32 / 32768.0 - 1.0
            })
            .collect();
        let m = measure(&noise, 44100.0);
        assert_eq!(min_rate(&m.above_db, -30.0, 44100), 22050);
    }

    #[test]
    fn bytes_follow_the_census_formula() {
        assert_eq!(adpcm_bytes(1.0, 22050, 44100), 788 * 16);
        assert_eq!(
            adpcm_bytes(1.0, 22050, 11025),
            adpcm_bytes(1.0, 11025, 11025)
        );
    }
}
