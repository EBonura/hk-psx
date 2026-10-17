//! Play-trace tools for the Hollow Knight SFX tiers (S2 spike).
//!
//!   hk-sfx-trace rates <hollow_knight_Data> <docs/audio-census> <out.tsv> [max seconds]
//!       decode every clip up to `max seconds` (default 20) with FMOD and measure
//!       how much of its energy lies above each rate's Nyquist
//!   hk-sfx-trace tally <docs/audio-census> <rates.tsv> <out dir> <manifest>
//!       fold the original's audio-call logs into per-scenario sound sets, call
//!       counts, peak voices and SPU bytes
//!
//!   hk-sfx-trace tiers <docs/audio-census> <rates.tsv> <out dir>
//!       split every scene's reachable local sounds into area and room tiers
//!
//!   hk-sfx-trace edges <docs/audio-census> <rates.tsv> <survey scenes.csv> <out dir>
//!       per gate, the sounds the next room needs that the last one does not hold
//!
//! Output is names, counts, seconds and bytes only: safe to commit.

mod csv;
mod edges;
mod rates;
mod tally;
mod tiers;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = match a.get(1).map(String::as_str) {
        Some("rates") if a.len() >= 5 => rates::run(&a[2], &a[3], &a[4], a.get(5).map(String::as_str)),
        Some("tally") if a.len() == 6 => tally::run(&a[2], &a[3], &a[4], &a[5]),
        Some("tiers") if a.len() == 5 => tiers::run(&a[2], &a[3], &a[4]),
        Some("edges") if a.len() == 6 => edges::run(&a[2], &a[3], &a[4], &a[5]),
        _ => Err("usage: hk-sfx-trace rates <hollow_knight_Data> <census dir> <out.tsv> [max s] | tally <census dir> <rates.tsv> <out dir> <manifest> | tiers <census dir> <rates.tsv> <out dir> | edges <census dir> <rates.tsv> <scenes.csv> <out dir>".into()),
    };
    if let Err(e) = r {
        eprintln!("hk-sfx-trace: {e}");
        std::process::exit(1);
    }
}
