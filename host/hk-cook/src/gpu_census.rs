//! tools/gpu_census.py in Rust: rank static scenery draws by estimated GPU
//! raster cost along a replay.
//!
//! Uses the emulator's silicon-calibrated per-pixel model (bus cycles), the same
//! rates as tools/frame_packets.py: textured 2.80/px, flat opaque 0.535/px.
//! Those rates and the two 128-pixel scissor payoff thresholds below have no
//! host authority; they are pinned to the emulator raster source recorded in
//! `opaque_tiles::RASTER_SOURCE`.
//!
//! A draw is billed flat only where `opaque_groups::flat_opaque_record` would
//! admit it as an immutable flat opaque Tilemap quad. That admission is a near
//! neighbour of the renderer's own flat path and not the same test:
//! `game/src/render.rs` emits `write_black` for a draw whose texture is solid
//! black, at full opacity, carrying BLACK_AVERAGE; it does not ask whether the
//! draw is a front draw or whether its source is a tilemap rect, and this has
//! no opacity to ask about. Today that difference is the whole difference: 2174
//! of the 10005 draws a bare tilemap test would bill flat are refused here, and
//! every one of them is refused for being a back draw. Until something owns the
//! raster question itself, this bills them textured, which is the direction that
//! does not quietly understate the frame.
//!
//! Cameras come from a profile's route/gpu logs, so the estimate can be checked
//! against the emulator's own per-frame gpu_cycles. Assumes every static draw
//! visible; dynamic sprites are not counted.

use crate::common::{err, py_round, Result};
use crate::opaque_groups::{flat_opaque_record, solid_word_one};
use crate::opaque_tiles::{
    draw_record, int, list, mutable_draws, read, read_json, text, u16_at, u32_at,
};
use crate::region_delta::layout;
use serde_json::Value as J;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// tools/frame_packets.py: textured and flat opaque bus cycles per pixel.
const TEXTURED: f64 = 179.0 / 64.0;
const FLAT: f64 = 137.0 / 256.0;
/// Bus cycles per millisecond.
const BUS_PER_MS: f64 = 33868.8;

type Point = (i64, i64);
type Box4 = (i64, i64, i64, i64);

fn area(r: Box4) -> i64 {
    (r.2 - r.0).max(0) * (r.3 - r.1).max(0)
}
fn clip(v: &[Point; 4]) -> Box4 {
    let xs = || v.iter().map(|p| p.0);
    let ys = || v.iter().map(|p| p.1);
    (
        xs().min().unwrap().max(0),
        ys().min().unwrap().max(0),
        xs().max().unwrap().min(320),
        ys().max().unwrap().min(240),
    )
}
fn projection(xy: &[i64; 8], scale: i64, c: [i64; 2]) -> [Point; 4] {
    let px = ((c[0] >> 8) * scale) >> 12;
    let py = ((c[1] >> 8) * scale) >> 12;
    core::array::from_fn(|i| {
        (
            160 + ((xy[i * 2] - px) >> 8),
            120 - ((xy[i * 2 + 1] - py) >> 8),
        )
    })
}
fn axis(v: &[Point; 4]) -> bool {
    v[0].1 == v[1].1 && v[0].0 == v[2].0 && v[1].0 == v[3].0 && v[2].1 == v[3].1
}

/// The span of screen positions along one axis whose texel falls inside
/// `[low, high)`, stepping the draw's own 4.12 texel walk.
fn interval(a: i64, b: i64, n: i64, low: i64, high: i64, screen: i64) -> (i64, i64) {
    let lo = a.min(b).max(0);
    let hi = a.max(b).min(screen);
    let sign = if b > a { 1 } else { -1 };
    let step = ((sign * (n - 1) * 4096) as f64 / (b - a).abs() as f64).trunc() as i64;
    let seed = (if b < a { n - 1 } else { 0 }) * 4096 + 2048;
    let good: Vec<i64> = (lo..hi)
        .filter(|&x| (low..high).contains(&((seed + (x - a.min(b)) * step) >> 12)))
        .collect();
    match (good.first(), good.last()) {
        (Some(&first), Some(&last)) => (first, last + 1),
        _ => (0, 0),
    }
}

/// Pixels a draw costs once its opaque scissor cover is taken out.
fn scissored(v: &[Point; 4], w: i64, h: i64, cover: &Option<Vec<u8>>) -> Result<i64> {
    let boxed = clip(v);
    let cover = match cover {
        Some(c) if !c.is_empty() => c,
        _ => return Ok(area(boxed)),
    };
    if !axis(v) || cover[0] > 4 || cover[0] == 0 || area(boxed) == 0 {
        return Ok(area(boxed));
    }
    let (dx, dy) = (v[1].0 - v[0].0, v[2].1 - v[0].1);
    if dx == 0 || dy == 0 || area(boxed) <= 128 {
        return Ok(area(boxed));
    }
    if cover[0] == 1
        && cover
            .get(4..8)
            .map(|c| c.iter().map(|&b| i64::from(b)).collect::<Vec<_>>())
            == Some(vec![0, 0, w - 1, h - 1])
    {
        return Ok(area(boxed));
    }
    let mut out = 0;
    for i in 0..usize::from(cover[0]) {
        let Some(r) = cover.get(4 + i * 4..8 + i * 4) else {
            return err("cover record outside its bytes");
        };
        let [x, y, ww, hh] = [r[0], r[1], r[2], r[3]].map(i64::from);
        let (l, rr) = interval(v[0].0, v[1].0, w, x, x + ww + 1, 320);
        let (t, b) = interval(v[0].1, v[2].1, h, y, y + hh + 1, 240);
        if l < rr && t < b {
            out += (rr - l) * (b - t);
        }
    }
    Ok(
        if area(boxed) - out > 128.max(128 * (i64::from(cover[0]) - 1).max(0)) {
            out
        } else {
            area(boxed)
        },
    )
}

/// Every draw source the static renderer mutates, per `opaque_tiles::mutable_draws`.
///
/// A source pooled into several regions is mutable everywhere once any region
/// mutates it, which is why this is one whole-world set rather than a per-region
/// one; reading the source names back costs one pass over the scene metadata.
fn mutated_sources(root: &Path, meta: &J) -> Result<HashSet<String>> {
    let geo = read_json(&root.join(".hkpsx/geo-provenance.json"))?;
    let life = read_json(&root.join(".hkpsx/lifeblood-provenance.json"))?;
    let regions = list(meta, "regions")?;
    let mut out = HashSet::new();
    for (index, ids) in mutable_draws(meta, &geo, &life)?.iter().enumerate() {
        if ids.is_empty() {
            continue;
        }
        let chunk = int(&regions[index], "chunk_id")?;
        let scene = read_json(&root.join(format!("data/regions/region-{chunk:03}/scene.json")))?;
        let draws = list(&scene, "draws")?;
        for &i in ids {
            out.insert(
                draws
                    .get(i as usize)
                    .ok_or("mutable draw outside its scene")?["source"]
                    .to_string(),
            );
        }
    }
    Ok(out)
}

struct Draw {
    id: usize,
    front: bool,
    scale: i64,
    xy: [i64; 8],
    w: i64,
    h: i64,
    cover: Option<Vec<u8>>,
    source: String,
    name: String,
    flat: bool,
    black_average: bool,
    empty: bool,
}

fn load(root: &Path, row: &J, mutated: &HashSet<String>) -> Result<Vec<Draw>> {
    let raw = read(&root.join(text(row, "path")?))?;
    let lay = layout(&raw)?;
    let scene = read_json(&root.join(format!(
        "data/regions/region-{:03}/scene.json",
        int(row, "chunk_id")?
    )))?;
    let source = list(&scene, "draws")?;
    // solid_word_one reads a pack's own texture table, palettes and pages; in an
    // HKROOM02 room those three sit where `layout` says they do, so the
    // cooker's solid-black test runs here unchanged.
    let sections = serde_json::json!({"sections": {"textures": {"offset": 40}, "palettes": {"offset": lay.prefix}, "pages": {"offset": lay.pages}}});
    let draws_at = 40 + lay.counts[1] as usize * 16;
    let features = u32_at(&raw, 36)?;
    let mut solid: HashMap<usize, bool> = HashMap::new();
    let mut draws = Vec::new();
    for i in 0..lay.counts[2] as usize {
        let (t, front, scale, xy, flags) = draw_record(&raw, draws_at, i)?;
        let t = usize::from(t);
        let at = 40 + t * 16;
        let [page, _u, _v, w, h, _pal] = core::array::from_fn(|k| u16_at(&raw, at + 2 * k));
        let (page, w, h) = (page?, w?, h?);
        let offset = u32_at(&raw, at + 12)? as usize;
        let cover = (page != 65535 && features & 1 != 0).then(|| {
            let start = (lay.stream + offset).min(raw.len());
            raw[start..(lay.stream + offset + 20).min(raw.len())].to_vec()
        });
        let src = source
            .get(i)
            .ok_or("scene.json has fewer draws than the room")?;
        let mut flat = flat_opaque_record(src, front, scale, &xy, flags[3], mutated);
        if flat {
            flat = match solid.get(&t) {
                Some(&known) => known,
                None => {
                    let known = solid_word_one(&raw, &sections, t)?;
                    solid.insert(t, known);
                    known
                }
            };
        }
        draws.push(Draw {
            id: i,
            front: front != 0,
            scale: i64::from(scale),
            xy,
            w: i64::from(w),
            h: i64::from(h),
            cover,
            source: src["source"].as_str().unwrap_or_default().to_string(),
            name: src["name"].as_str().unwrap_or_default().to_string(),
            flat,
            black_average: flags[3] == 1,
            empty: page == 65535,
        });
    }
    Ok(draws)
}

/// Estimated (cycles, pixels, draw index) per visible draw at a camera.
fn frame_cost(draws: &[Draw], camera: [f64; 2]) -> Result<Vec<(f64, i64, usize)>> {
    let c = [py_round(camera[0] * 65536.0), py_round(camera[1] * 65536.0)];
    let mut out = Vec::new();
    for (index, d) in draws.iter().enumerate() {
        if d.empty {
            continue;
        }
        let px = scissored(&projection(&d.xy, d.scale, c), d.w, d.h, &d.cover)?;
        if px == 0 {
            continue;
        }
        let rate = if d.flat { FLAT } else { TEXTURED };
        out.push((px as f64 * rate, px, index));
    }
    Ok(out)
}

/// Python's `format(v, ',.0f')`.
fn commas(v: f64) -> String {
    let text = format!("{v:.0}");
    let (sign, digits) = text
        .strip_prefix('-')
        .map_or(("", text.as_str()), |d| ("-", d));
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}")
}

/// Python's `math.fsum`: the correctly rounded sum of the values.
fn fsum(values: &[f64]) -> f64 {
    let mut partials: Vec<f64> = Vec::new();
    for &x in values {
        let mut x = x;
        let mut i = 0;
        for k in 0..partials.len() {
            let mut y = partials[k];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[i] = lo;
                i += 1;
            }
            x = hi;
        }
        partials.truncate(i);
        partials.push(x);
    }
    let mut total = 0.0;
    for &p in partials.iter().rev() {
        total += p;
    }
    total
}
fn mean(values: &[f64]) -> f64 {
    fsum(values) / values.len() as f64
}
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// An insertion-ordered tally, the `collections.Counter` the Python keeps.
struct Tally<K: std::hash::Hash + Eq + Clone> {
    index: HashMap<K, usize>,
    items: Vec<(K, f64)>,
}
impl<K: std::hash::Hash + Eq + Clone> Tally<K> {
    fn new() -> Self {
        Tally {
            index: HashMap::new(),
            items: Vec::new(),
        }
    }
    fn add(&mut self, key: K, v: f64) {
        match self.index.get(&key) {
            Some(&i) => self.items[i].1 += v,
            None => {
                self.index.insert(key.clone(), self.items.len());
                self.items.push((key, v));
            }
        }
    }
    /// `most_common(n)`: largest first, ties in insertion order.
    fn most_common(&self, n: usize) -> Vec<&(K, f64)> {
        let mut all: Vec<&(K, f64)> = self.items.iter().collect();
        all.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        all.truncate(n);
        all
    }
}

struct Options {
    profile: Option<PathBuf>,
    camera: Option<[f64; 2]>,
    region: usize,
    top: usize,
    every: usize,
}
fn options(args: &[String]) -> Result<Options> {
    let mut o = Options {
        profile: None,
        camera: None,
        region: 1,
        top: 25,
        every: 10,
    };
    let mut it = args.iter();
    let value = |name: &str, it: &mut std::slice::Iter<String>| {
        it.next()
            .cloned()
            .ok_or_else(|| format!("{name} needs a value"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--profile" => o.profile = Some(PathBuf::from(value(a, &mut it)?)),
            "--camera" => {
                let (x, y) = (value(a, &mut it)?, value(a, &mut it)?);
                o.camera = Some([
                    x.parse().map_err(|_| "--camera X Y")?,
                    y.parse().map_err(|_| "--camera X Y")?,
                ]);
            }
            "--region" => o.region = value(a, &mut it)?.parse().map_err(|_| "--region N")?,
            "--top" => o.top = value(a, &mut it)?.parse().map_err(|_| "--top N")?,
            "--every" => o.every = value(a, &mut it)?.parse().map_err(|_| "--every N")?,
            other => return err(format!("gpu-census: unknown option {other}")),
        }
    }
    Ok(o)
}

/// A CSV file's rows as header-keyed maps (the logs hold no quoted fields).
fn csv_rows(path: &Path) -> Result<Vec<HashMap<String, String>>> {
    let text = String::from_utf8(read(path)?).map_err(|e| e.to_string())?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or("empty csv")?.split(',').collect();
    Ok(lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            header
                .iter()
                .map(|h| h.to_string())
                .zip(l.split(',').map(str::to_string))
                .collect()
        })
        .collect())
}

struct Frame {
    cycles: i64,
    x: i64,
    y: i64,
    region: i64,
}

pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let a = options(args)?;
    let metadata = read_json(&root.join("data/regions.json"))?;
    let meta = list(&metadata, "regions")?;
    let mutated = mutated_sources(root, &metadata)?;
    let mut cache: HashMap<usize, Vec<Draw>> = HashMap::new();
    let region = |i: usize, cache: &mut HashMap<usize, Vec<Draw>>| -> Result<()> {
        if !cache.contains_key(&i) {
            let row = meta
                .get(i.wrapping_sub(1))
                .ok_or_else(|| format!("no region {i}"))?;
            cache.insert(i, load(root, row, &mutated)?);
        }
        Ok(())
    };
    if let Some(camera) = a.camera {
        region(a.region, &mut cache)?;
        let draws = &cache[&a.region];
        let mut costs = frame_cost(draws, camera)?;
        costs.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
        let total = costs.iter().fold(0.0, |sum, c| sum + c.0);
        println!(
            "region {} camera [{:?}, {:?}]: {} draws, {} estimated bus cycles = {:.1} ms",
            a.region,
            camera[0],
            camera[1],
            costs.len(),
            commas(total),
            total / BUS_PER_MS
        );
        for &(c, px, index) in costs.iter().take(a.top) {
            let d = &draws[index];
            let kind = if d.flat {
                "FLAT "
            } else if d.black_average {
                "avg"
            } else {
                "tex"
            };
            println!(
                "  {:>9} cyc {:>7} px {} {} scale {:6.2} {}x{} {} [{}]",
                commas(c),
                commas(px as f64),
                if d.front { "front" } else { "back " },
                kind,
                d.scale as f64 / 4096.0,
                d.w,
                d.h,
                d.name,
                d.source
            );
        }
        return Ok(());
    }
    let profile = a
        .profile
        .as_ref()
        .ok_or("gpu-census: pass --profile DIR or --camera X Y")?;
    let mut watches = None;
    for name in ["replay.json", "command.json"] {
        if profile.join(name).exists() {
            watches = Some(read_json(&profile.join(name))?["watches"].clone());
            break;
        }
    }
    let watches = watches.ok_or("gpu-census: the profile has no replay.json or command.json")?;
    let col = |n: &str| -> Result<String> {
        Ok(format!(
            "ram_{}",
            watches[n]
                .as_str()
                .ok_or_else(|| format!("no watch {n}"))?
                .get(2..)
                .unwrap_or("")
        ))
    };
    let (col_x, col_y, col_region) = (
        col("HK_PLAYER_X")?,
        col("HK_PLAYER_Y")?,
        col("HK_REGION_ID")?,
    );
    let rows = csv_rows(&profile.join("route.csv"))?;
    let gpu: HashMap<i64, HashMap<String, String>> = csv_rows(&profile.join("gpu.csv"))?
        .into_iter()
        .map(|r| {
            Ok((
                r["route_tick"].parse::<i64>().map_err(|e| e.to_string())?,
                r,
            ))
        })
        .collect::<Result<_>>()?;
    let num = |m: &HashMap<String, String>, k: &str| -> Result<i64> {
        m.get(k)
            .ok_or_else(|| format!("missing column {k}"))?
            .parse::<i64>()
            .map_err(|e| format!("{k}: {e}"))
    };
    let mut frames: Vec<Frame> = Vec::new();
    let mut current: Option<Frame> = None;
    for r in &rows {
        let g = gpu.get(&num(r, "route_tick")?);
        if let Some(g) = g.filter(|g| g["display_start_changed"] == "1") {
            frames.extend(current.take());
            let _ = g;
            current = Some(Frame {
                cycles: 0,
                x: num(r, &col_x)?,
                y: num(r, &col_y)?,
                region: num(r, &col_region)?,
            });
        }
        if let (Some(cur), Some(g)) = (current.as_mut(), g) {
            cur.cycles += num(g, "gpu_cycles")?;
        }
    }
    let frames: Vec<&Frame> = frames
        .iter()
        .filter(|f| f.region > 0 && f.x < (1 << 31))
        .collect();
    let mut by_source: Tally<(i64, usize, String, bool, bool)> = Tally::new();
    let mut pairs: Vec<(f64, i64)> = Vec::new();
    for f in frames.iter().step_by(a.every.max(1)) {
        let m = meta
            .get((f.region - 1) as usize)
            .ok_or("a frame's region is outside the metadata")?;
        let (x, y) = (f.x as f64 / 65536.0, f.y as f64 / 65536.0);
        let bound = |key: &str, i: usize| {
            m[key][i]
                .as_f64()
                .ok_or_else(|| format!("{key} is not a number"))
        };
        let cam = [
            x.max(bound("camera_x", 0)?).min(bound("camera_x", 1)?),
            (y + 2.0)
                .max(bound("camera_y", 0)?)
                .min(bound("camera_y", 1)?),
        ];
        region(f.region as usize, &mut cache)?;
        let draws = &cache[&(f.region as usize)];
        let costs = frame_cost(draws, cam)?;
        let est = costs.iter().fold(0.0, |sum, c| sum + c.0);
        pairs.push((est, f.cycles));
        for &(c, _, index) in &costs {
            let d = &draws[index];
            by_source.add((f.region, d.id, d.name.clone(), d.front, d.flat), c);
        }
    }
    if !pairs.is_empty() {
        let ratio: Vec<f64> = pairs.iter().map(|&(e, g)| e / g.max(1) as f64).collect();
        let emulator: Vec<f64> = pairs.iter().map(|p| p.1 as f64).collect();
        let estimate: Vec<f64> = pairs.iter().map(|p| p.0).collect();
        println!(
            "{} sampled frames: estimate/emulator ratio mean {:.3} median {:.3}; emulator mean {} cyc/frame, estimate mean {}",
            pairs.len(),
            mean(&ratio),
            median(&ratio),
            commas(mean(&emulator)),
            commas(mean(&estimate))
        );
    }
    let total = by_source.items.iter().fold(0.0, |sum, i| sum + i.1);
    println!("top draws by summed estimated cost over sampled frames (share of static estimate):");
    for ((reg, i, name, front, flat), c) in by_source
        .most_common(a.top)
        .into_iter()
        .map(|e| (&e.0, e.1))
    {
        println!(
            "  {:5.1}%  region {:3} draw {:3} {} {} {}",
            c * 100.0 / total,
            reg,
            i,
            if *front { "front" } else { "back " },
            if *flat { "FLAT" } else { "tex " },
            name
        );
    }
    let mut by_name: Tally<String> = Tally::new();
    for ((_, _, name, _, _), c) in &by_source.items {
        by_name.add(
            name.trim_end_matches(|ch: char| "0123456789 ()".contains(ch))
                .to_string(),
            *c,
        );
    }
    println!("by name family:");
    for (k, c) in by_name.most_common(a.top).into_iter().map(|e| (&e.0, e.1)) {
        println!("  {:5.1}%  {}", c * 100.0 / total, k);
    }
    Ok(())
}
