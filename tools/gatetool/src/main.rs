//! Reads HK_GATE_LOG out of RAM dumps of gate-log builds and compares runs.
//! usage: gatetool <scene-names.tsv> <label=ram.bin:game.map>...
//! `scene-names.tsv` is `scene_id<TAB>name` per line:
//! `jq -r '.regions | map({k:.scene_id, v:.scene_name}) | unique_by(.k) | .[] | "\(.k)\t\(.v)"' data/regions.json`.
//! Environment: PHASES=1 adds the phase times of the gates over 30 vb, ALL=1 of every gate.
//! One row per gate with, per run, black (vblanks from the end of the 20 vb exit fade to the first lit
//! frame), load (vblanks of the gate's load), prefetched/total sectors, then a summary per run.
use std::fs;

const WORDS: usize = 23;

fn sym(map: &str, name: &str) -> Option<u32> {
    for line in map.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.len() == 5 && t[4] == name {
            if let Ok(a) = u32::from_str_radix(t[0], 16) {
                return Some(a);
            }
        }
    }
    None
}

fn word(ram: &[u8], addr: u32) -> u32 {
    let o = (addr & 0x1fff_ffff) as usize;
    u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
}

#[derive(Clone, Default)]
struct Gate {
    src: u32,
    dst: u32,
    black: i32,
    load: i32,
    pre: u32,
    total: u32,
    reads: u32,
    ms: [f32; 16],
}

fn read_run(ram_path: &str, map_path: &str) -> Vec<Gate> {
    let ram = fs::read(ram_path).expect("ram");
    let map = fs::read_to_string(map_path).expect("map");
    let log = sym(&map, "HK_GATE_LOG").expect("HK_GATE_LOG");
    let count = word(&ram, sym(&map, "HK_GATE_COUNT").expect("count"));
    let mut out = Vec::new();
    for g in 0..count as usize {
        let base = log + (g * WORDS * 4) as u32;
        let w: Vec<u32> = (0..WORDS)
            .map(|i| word(&ram, base + 4 * i as u32))
            .collect();
        let mut ms = [0f32; 16];
        for (p, m) in ms.iter_mut().enumerate() {
            *m = ((w[11 + p / 2] >> ((p % 2) * 16)) & 0xffff) as f32 * 0.0636;
        }
        out.push(Gate {
            src: w[1],
            dst: w[2],
            black: w[19].wrapping_sub(w[3]).wrapping_sub(20) as i32,
            load: w[5].wrapping_sub(w[4]) as i32,
            pre: w[20],
            total: w[21],
            reads: w[10],
            ms,
        });
    }
    out
}

fn counters(ram_path: &str, map_path: &str) -> String {
    let ram = fs::read(ram_path).expect("ram");
    let map = fs::read_to_string(map_path).expect("map");
    let mut out = String::new();
    for name in [
        "HK_INPUT_MISSED_VBLANKS",
        "HK_INPUT_FAULT",
        "HK_ROOM_LOAD_ERROR",
        "HK_CD_STREAM_ERROR",
        "HK_MODULE_ART_LATE",
        "HK_MODULE_GATE_ART_MISSED",
        "HK_TOUR_STEP",
        "HK_TOUR_MISSES",
    ] {
        if let Some(a) = sym(&map, name) {
            out += &format!(" {}={}", name.trim_start_matches("HK_"), word(&ram, a));
        }
    }
    out
}

fn pct(v: &mut Vec<i32>, q: f32) -> i32 {
    v.sort();
    v[((v.len() as f32 - 1.0) * q) as usize]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let scene_names: std::collections::HashMap<u32, String> = fs::read_to_string(&a[1])
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter_map(|(i, n)| Some((i.parse().ok()?, n.to_string())))
        .collect();
    let mut runs: Vec<(String, Vec<Gate>)> = Vec::new();
    for spec in &a[2..] {
        let (label, rest) = spec.split_once('=').unwrap();
        let (ram, map) = rest.split_once(':').unwrap();
        println!("{label}:{}", counters(ram, map));
        runs.push((label.to_string(), read_run(ram, map)));
    }
    let n = runs.iter().map(|r| r.1.len()).min().unwrap_or(0);
    if n == 0 {
        println!("no gates recorded");
        return;
    }
    print!("  n {:<44}", "gate");
    for r in &runs {
        print!(" | {:>6} black load pre/tot rd", r.0);
    }
    println!();
    for g in 0..n {
        let x0 = &runs[0].1[g];
        let name = format!(
            "{} -> {}",
            scene_names.get(&x0.src).map_or("?", |s| s.as_str()),
            scene_names.get(&x0.dst).map_or("?", |s| s.as_str())
        );
        print!("{:3} {:<44}", g, name);
        for r in &runs {
            let x = &r.1[g];
            print!(
                " | {:>6} {:>5} {:>4} {:>3}/{:<3} {}",
                "", x.black, x.load, x.pre, x.total, x.reads
            );
        }
        println!();
    }
    if std::env::var("PHASES").is_ok() {
        let r = &runs[runs.len() - 1];
        println!("phases (ms) of {} for gates over 30 vb: prep drive cd amb hashS reloc(gate_begin) lz4 hashR(audio bank) valid(ambience+map) cover upload scene meta disp region other", r.0);
        for (g, x) in r.1.iter().enumerate() {
            if x.black > 30 || std::env::var("ALL").is_ok() {
                let name = format!(
                    "{} -> {}",
                    scene_names.get(&x.src).map_or("?", |s| s.as_str()),
                    scene_names.get(&x.dst).map_or("?", |s| s.as_str())
                );
                print!(
                    "{:3} {:<40} black {:3} {:3}/{:<3} |",
                    g, name, x.black, x.pre, x.total
                );
                for m in x.ms.iter() {
                    print!(" {:4.0}", m);
                }
                println!();
            }
        }
    }
    for r in &runs {
        let mut avg = [0f32; 16];
        for x in &r.1[..n] {
            for p in 0..16 {
                avg[p] += x.ms[p] / n as f32;
            }
        }
        println!(
            "{} mean ms/gate: prep {:.0} drive {:.0} cd {:.0} hashS {:.0} lz4 {:.0} upload {:.0} meta {:.0} region {:.0} other {:.0}",
            r.0, avg[0], avg[1], avg[2], avg[4], avg[6], avg[10], avg[12], avg[14], avg[15]
        );
    }
    for r in &runs {
        let mut b: Vec<i32> = r.1[..n].iter().map(|x| x.black).collect();
        let within = b.iter().filter(|&&x| x <= 30).count();
        let mut l: Vec<i32> = r.1[..n].iter().map(|x| x.load).collect();
        let over: i32 = r.1[..n].iter().map(|x| (x.black - 30).max(0)).sum();
        let sum: i32 = r.1[..n].iter().map(|x| x.black).sum();
        println!(
            "{}: gates {} within30vb {} over-30 sum {} black sum {} median {} p90 {} max {}; load median {} p90 {}",
            r.0, n, within, over, sum, pct(&mut b, 0.5), pct(&mut b, 0.9), pct(&mut b, 1.0), pct(&mut l, 0.5), pct(&mut l, 0.9)
        );
    }
}
