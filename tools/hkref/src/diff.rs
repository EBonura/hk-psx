//! Compare the two traces tick by tick and event by event.

use crate::trace::{Event, Trace};
use std::collections::BTreeMap;
use std::fmt::Write;

pub struct Opts {
    pub t0: i64,
    pub n: usize,
    pub tol: BTreeMap<String, f64>,
}

fn tol_for(o: &Opts, chan: &str) -> f64 {
    if let Some(t) = o.tol.get(chan) {
        return *t;
    }
    match chan.rsplit('.').next().unwrap_or("") {
        "x" | "y" if chan.starts_with("cam") => 0.3,
        "x" | "y" => 0.1,
        "hp" => 0.0,
        "face" => 0.0,
        "soul" => 0.0,
        _ => 0.1,
    }
}

fn series(t: &Trace, chan: &str, t0: i64, n: usize) -> Vec<Option<f64>> {
    (0..n as i64).map(|i| t.get(chan, t0 + i)).collect()
}

fn derived(t: &Trace, chan: &str, t0: i64, n: usize) -> Vec<Option<f64>> {
    // per-tick delta of a position channel
    let s = series(t, chan, t0 - 1, n + 1);
    (0..n)
        .map(|i| match (s[i], s[i + 1]) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        })
        .collect()
}

pub struct Chan {
    pub name: String,
    pub o: Vec<Option<f64>>,
    pub p: Vec<Option<f64>>,
    pub tol: f64,
}

pub fn channels(o: &Trace, p: &Trace, opt: &Opts) -> Vec<Chan> {
    let mut v = Vec::new();
    let mut names: Vec<String> = o
        .num
        .keys()
        .filter(|k| p.num.contains_key(*k))
        .cloned()
        .collect();
    names.retain(|k| {
        !k.starts_with("input.")
            && !k.starts_with("port.")
            && !k.starts_with("og.")
            && k != "hero.vx"
            && k != "hero.vy"
            && k != "hero.clip_frame"
            && k != "hero.soul"
    });
    for k in names {
        let (a, b) = (series(o, &k, opt.t0, opt.n), series(p, &k, opt.t0, opt.n));
        if a.iter().all(Option::is_none) || b.iter().all(Option::is_none) {
            continue;
        }
        let tol = tol_for(opt, &k);
        v.push(Chan {
            name: k,
            o: a,
            p: b,
            tol,
        });
    }
    for k in ["hero.x", "hero.y"] {
        let (a, b) = (derived(o, k, opt.t0, opt.n), derived(p, k, opt.t0, opt.n));
        v.push(Chan {
            name: format!("{}.step", k),
            o: a,
            p: b,
            tol: 0.03,
        });
    }
    v
}

pub struct Stat {
    pub n: usize,
    pub mean: f64,
    pub max: f64,
    pub first: Option<usize>,
    pub best_lag: i32,
    pub lag_mean: f64,
}

pub fn stat(c: &Chan) -> Stat {
    let n = c.o.len();
    let err = |lag: i32| -> (usize, f64, f64) {
        let (mut k, mut s, mut m) = (0usize, 0.0, 0.0f64);
        for i in 0..n as i32 {
            let j = i + lag;
            if j < 0 || j >= n as i32 {
                continue;
            }
            if let (Some(a), Some(b)) = (c.o[i as usize], c.p[j as usize]) {
                let d = (a - b).abs();
                k += 1;
                s += d;
                m = m.max(d);
            }
        }
        (k, if k > 0 { s / k as f64 } else { f64::NAN }, m)
    };
    let (k, mean, max) = err(0);
    let mut first = None;
    let mut run = 0;
    for i in 0..n {
        match (c.o[i], c.p[i]) {
            (Some(a), Some(b)) if (a - b).abs() > c.tol => {
                run += 1;
                if run == 3 && first.is_none() {
                    first = Some(i - 2);
                }
            }
            _ => run = 0,
        }
    }
    let (mut bl, mut bm) = (0, mean);
    for lag in -8..=8 {
        let (kk, m, _) = err(lag);
        if kk > 10 && m < bm - 1e-9 {
            bm = m;
            bl = lag;
        }
    }
    Stat {
        n: k,
        mean,
        max,
        first,
        best_lag: bl,
        lag_mean: bm,
    }
}

/// Greedy in-order pairing of two sorted tick lists within `window` ticks.
pub fn pair(a: &[i64], b: &[i64], window: i64) -> (Vec<(i64, i64)>, Vec<i64>, Vec<i64>) {
    let (mut i, mut j) = (0, 0);
    let (mut pairs, mut ua, mut ub) = (vec![], vec![], vec![]);
    while i < a.len() && j < b.len() {
        let d = b[j] - a[i];
        if d.abs() <= window {
            pairs.push((a[i], b[j]));
            i += 1;
            j += 1;
        } else if d > 0 {
            ua.push(a[i]);
            i += 1
        } else {
            ub.push(b[j]);
            j += 1
        }
    }
    ua.extend_from_slice(&a[i..]);
    ub.extend_from_slice(&b[j..]);
    (pairs, ua, ub)
}

pub fn report(
    name: &str,
    o: &Trace,
    p: &Trace,
    oe: &[Event],
    pe: &[Event],
    opt: &Opts,
    tape_note: &str,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Side-by-side diff: {name}\n");
    let _ = writeln!(
        s,
        "Window: ticks {}..{} ({} ticks, 60 Hz sim clock). {tape_note}\n",
        opt.t0,
        opt.t0 + opt.n as i64,
        opt.n
    );
    let _ = writeln!(s, "Lag convention: a positive best lag means the PORT shows the same value that many ticks LATER than the original.\n");
    let _ = writeln!(s, "## Channels\n\n| channel | ticks | mean abs diff | max abs diff | tolerance | first divergence (3 ticks over tol) | best lag | mean diff at best lag |\n|---|---|---|---|---|---|---|---|");
    let ch = channels(o, p, opt);
    let mut worst: Vec<(String, usize, String)> = vec![];
    for c in &ch {
        let st = stat(c);
        let first = st
            .first
            .map(|f| format!("tick {}", opt.t0 + f as i64))
            .unwrap_or_else(|| "none".into());
        let _ = writeln!(
            s,
            "| {} | {} | {:.4} | {:.4} | {} | {} | {} | {:.4} |",
            c.name, st.n, st.mean, st.max, c.tol, first, st.best_lag, st.lag_mean
        );
        if let Some(f) = st.first {
            if let (Some(a), Some(b)) = (c.o[f], c.p[f]) {
                worst.push((c.name.clone(), f, format!("original {a:.3}, port {b:.3}")));
            }
        }
    }
    worst.sort_by_key(|w| w.1);
    let _ = writeln!(s, "\n## First divergences, in order\n");
    if worst.is_empty() {
        let _ = writeln!(s, "None over tolerance.\n");
    }
    for (c, f, d) in &worst {
        let _ = writeln!(
            s,
            "- tick {}: `{}` leaves tolerance ({d})",
            opt.t0 + *f as i64,
            c
        );
    }
    // text channels of the original (informational): hero state transitions
    if let Some(col) = o.text.get("s.hero.state") {
        let _ = writeln!(
            s,
            "\n## Original hero state transitions (the port exports no state word)\n"
        );
        let mut prev = "";
        for (i, v) in col.iter().enumerate() {
            let t = o.ticks[i];
            if t < opt.t0 || t >= opt.t0 + opt.n as i64 {
                continue;
            }
            if v != prev && !v.is_empty() {
                let _ = writeln!(
                    s,
                    "- tick {t}: {v} (clip {})",
                    o.text
                        .get("s.hero.clip")
                        .map(|c| c[i].as_str())
                        .unwrap_or("")
                );
                prev = v;
            }
        }
    }
    latency_section(&mut s, o, p, opt);
    // events
    let _ = writeln!(s, "\n## Events\n");
    let kinds: Vec<String> = {
        let mut k: Vec<String> = oe.iter().chain(pe.iter()).map(|e| e.kind.clone()).collect();
        k.sort();
        k.dedup();
        k
    };
    let in_win = |e: &&Event| e.tick >= opt.t0 && e.tick < opt.t0 + opt.n as i64;
    let _ = writeln!(s, "| kind | original | port | paired (within 12 ticks) | mean port-orig | min | max | unmatched orig | unmatched port |\n|---|---|---|---|---|---|---|---|---|");
    let mut lines = vec![];
    for k in &kinds {
        if k == "sfx" {
            continue;
        }
        let a: Vec<i64> = oe
            .iter()
            .filter(in_win)
            .filter(|e| e.kind == *k)
            .map(|e| e.tick)
            .collect();
        let b: Vec<i64> = pe
            .iter()
            .filter(in_win)
            .filter(|e| e.kind == *k)
            .map(|e| e.tick)
            .collect();
        if a.is_empty() && b.is_empty() {
            continue;
        }
        let (pairs, ua, ub) = pair(&a, &b, 12);
        let d: Vec<i64> = pairs.iter().map(|(x, y)| y - x).collect();
        let mean = if d.is_empty() {
            f64::NAN
        } else {
            d.iter().sum::<i64>() as f64 / d.len() as f64
        };
        let _ = writeln!(
            s,
            "| {k} | {} | {} | {} | {:.1} | {} | {} | {} | {} |",
            a.len(),
            b.len(),
            pairs.len(),
            mean,
            d.iter().min().map_or("".into(), |v| v.to_string()),
            d.iter().max().map_or("".into(), |v| v.to_string()),
            ua.len(),
            ub.len()
        );
        for (x, y) in pairs.iter().take(12) {
            lines.push(format!(
                "- `{k}`: original tick {x}, port tick {y} ({:+} ticks)",
                y - x
            ));
        }
        for x in ua.iter().take(6) {
            lines.push(format!(
                "- `{k}`: original tick {x} has no port counterpart"
            ));
        }
        for y in ub.iter().take(6) {
            lines.push(format!(
                "- `{k}`: port tick {y} has no original counterpart"
            ));
        }
    }
    let _ = writeln!(s, "\n### Event detail (first rows per kind)\n");
    for l in lines {
        let _ = writeln!(s, "{l}");
    }
    let sfx: Vec<&Event> = oe
        .iter()
        .filter(in_win)
        .filter(|e| e.kind == "sfx")
        .collect();
    if !sfx.is_empty() {
        let mut by: BTreeMap<&str, (usize, i64)> = BTreeMap::new();
        for e in &sfx {
            let x = by.entry(&e.name).or_insert((0, e.tick));
            x.0 += 1;
        }
        let _ = writeln!(s, "\n### Original SFX calls in the window (the port has no per-call trace; see AUDIT.md for bank coverage)\n\n| clip | calls | first tick |\n|---|---|---|");
        for (n, (c, t)) in by {
            let _ = writeln!(s, "| {n} | {c} | {t} |");
        }
    }
    let fsm: Vec<&Event> = oe
        .iter()
        .filter(in_win)
        .filter(|e| e.kind.ends_with(".fsm"))
        .collect();
    if !fsm.is_empty() {
        let _ = writeln!(s, "\n### Original actor FSM state changes in the window\n");
        for e in fsm.iter().take(60) {
            let _ = writeln!(
                s,
                "- tick {}: {} -> {}",
                e.tick,
                e.kind.trim_end_matches(".fsm"),
                e.name
            );
        }
    }
    s
}

/// Input latency: for every rising edge of Right/Left/Cross in the shared tape,
/// how many ticks until the hero visibly moves, in each game.
fn latency_section(s: &mut String, o: &Trace, p: &Trace, opt: &Opts) {
    let _ = writeln!(
        s,
        "\n## Input latency (ticks from a button edge to the first visible motion)\n"
    );
    let _ = writeln!(
        s,
        "| button | press tick | original | port | port - original |\n|---|---|---|---|---|"
    );
    let (mut so, mut sp, mut n) = (0.0, 0.0, 0.0);
    for (name, bit, chan, sign) in [
        ("right", 0x20u32, "hero.x", 1.0),
        ("left", 0x80, "hero.x", -1.0),
        ("cross", 0x4000, "hero.y", 1.0),
    ] {
        let mut prev = 0u32;
        for i in 0..opt.n as i64 {
            let t = opt.t0 + i;
            let pad = o.get("input.pad", t).map_or(prev, |v| v as u32);
            let rising = pad & bit != 0 && prev & bit == 0;
            prev = pad;
            if !rising || i < 2 {
                continue;
            }
            let onset = |tr: &Trace| -> Option<i64> {
                (0..14).find(|d| match (tr.get(chan, t + d), tr.get(chan, t + d - 1)) {
                    (Some(a), Some(b)) => (a - b) * sign > 0.004,
                    _ => false,
                })
            };
            // only count a press that starts from rest (no motion on the previous tick) in both games
            let rest = |tr: &Trace| match (tr.get(chan, t), tr.get(chan, t - 1)) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-4,
                _ => false,
            };
            if !rest(o) || !rest(p) {
                continue;
            }
            if let (Some(a), Some(b)) = (onset(o), onset(p)) {
                let _ = writeln!(s, "| {name} | {t} | {a} | {b} | {:+} |", b - a);
                so += a as f64;
                sp += b as f64;
                n += 1.0;
            }
        }
    }
    if n > 0.0 {
        let _ = writeln!(s, "\nMean over {n} presses from rest: original {:.2} ticks, port {:.2} ticks, port - original {:+.2}.", so / n, sp / n, (sp - so) / n);
    } else {
        let _ = writeln!(s, "\nNo clean presses from rest in this window.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_is_in_order_and_reports_leftovers() {
        let (p, ua, ub) = pair(&[10, 50, 90], &[12, 48, 200], 6);
        assert_eq!(p, vec![(10, 12), (50, 48)]);
        assert_eq!(ua, vec![90]);
        assert_eq!(ub, vec![200]);
    }
}
