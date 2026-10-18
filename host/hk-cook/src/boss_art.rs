//! A boss's art as its own bank (docs/BOSS-ART.md): the sprites its fight can show, one shared palette per group of
//! similar sprites, the parts that are left once transparent cells are cut away, and the coded texel stream.
//!
//! Nothing here knows about Hornet. A boss supplies its animated objects (`hornet_scene::Ent`, from the tree cooker's
//! manifest) and, optionally, the clips each object really plays; this module cooks those clips into sprites, quantizes them
//! with the room cook's own quantizer, merges palettes while the visible error stays inside a bound, and cuts every sprite
//! into the 64x64-cell parts the animation cache uploads.
use crate::actor_art::{guest_wrap_of, scale_alpha};
use crate::atlas::SLOT_PIXELS;
use crate::common::{get, py_round};
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::hornet_scene::{group_of, Ent, Group};
use crate::quantize;
use hk_pil::Image;
use hk_unity::{Source, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// One sprite as the room cook draws it: resized to the pixel size it has on screen.
pub struct Sprite {
    pub id: String,
    pub box_: [f64; 4],
    pub w: usize,
    pub h: usize,
    /// RGBA at `w` by `h`.
    pub image: Image,
}

/// One clip of one group: the sprites its frames show, in order.
pub struct ClipRec {
    pub group: Group,
    pub name: String,
    pub library: String,
    pub fps: f64,
    pub wrap: i64,
    pub loop_start: i64,
    pub frames: Vec<usize>,
}

pub struct Library {
    pub sprites: Vec<Sprite>,
    pub clips: Vec<ClipRec>,
}

/// The clips a boss's objects play, by (tree, object): all of what the manifest names when `used` has no entry.
pub type Used = BTreeMap<(u8, u8), BTreeSet<String>>;

pub fn wanted_clips(ents: &[Ent], used: Option<&Used>) -> BTreeMap<Group, (usize, BTreeSet<String>)> {
    let mut wanted: BTreeMap<Group, (usize, BTreeSet<String>)> = BTreeMap::new();
    for (i, e) in ents.iter().enumerate() {
        let slot = wanted.entry(group_of(e)).or_insert_with(|| (i, BTreeSet::new()));
        match used.and_then(|u| u.get(&(e.tree, e.ent))) {
            Some(names) => slot.1.extend(names.iter().cloned()),
            None if used.is_none() => slot.1.extend(e.clips.iter().map(|c| c.0.clone())),
            None => {}
        }
    }
    wanted.retain(|_, v| !v.1.is_empty());
    wanted
}

/// Cook the wanted clips into sprites at their on-screen size.
pub fn load(source: &Source, ents: &[Ent], used: Option<&Used>) -> Result<Library> {
    let scale = focal() / -CAM_Z;
    let mut textures: HashMap<String, Arc<Image>> = HashMap::new();
    let mut collections: HashMap<String, Value> = HashMap::new();
    let mut lib = Library { sprites: vec![], clips: vec![] };
    let mut index: HashMap<(String, i64, u64, u64, i64), usize> = HashMap::new();
    for (group, (first, names)) in &wanted_clips(ents, used) {
        let e = &ents[*first];
        let (file, path) = e.library.rsplit_once(':').ok_or("library id")?;
        let library_file = source.file(file)?;
        let library_o = source.object(&library_file, path.parse()?)?;
        let library = source.read(&library_o)?;
        let (sx, sy) = (f64::from_bits(group.1), f64::from_bits(group.2));
        for name in names {
            let clip = library
                .get("clips")
                .and_then(Value::list)
                .and_then(|l| l.iter().find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name.as_str())))
                .ok_or_else(|| format!("{} has no clip {name}", e.library))?;
            let frames = clip.get("frames").and_then(Value::list).unwrap_or(&[]);
            let mut seq = vec![];
            for frame in frames {
                let co = source.deref(&library_o.file, get(frame, "spriteCollection")?)?;
                let sid = co.sid();
                if !collections.contains_key(&sid) {
                    collections.insert(sid.clone(), source.read(&co)?);
                }
                let sprite_id = get(frame, "spriteId")?.int().unwrap_or(0);
                let key = (sid.clone(), sprite_id, group.1, group.2, group.3);
                let at = match index.get(&key) {
                    Some(&i) => i,
                    None => {
                        let (mut image, b) = tk_sprite(source, &co.file, &collections[&sid], sprite_id as usize, &mut textures)?;
                        scale_alpha(&mut image, e.alpha);
                        let b = [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
                        let w = (((b[2] - b[0]) * scale).ceil() as usize).clamp(1, 64 * 20);
                        let h = (((b[3] - b[1]) * scale).ceil() as usize).clamp(1, 64 * 20);
                        let image = image.to_rgba().resize(w, h, hk_pil::resample::Filter::Lanczos);
                        lib.sprites.push(Sprite { id: format!("{sid}:{sprite_id}@{:.3}x{:.3}", sx, sy), box_: b, w, h, image });
                        index.insert(key, lib.sprites.len() - 1);
                        lib.sprites.len() - 1
                    }
                };
                seq.push(at);
            }
            lib.clips.push(ClipRec {
                group: group.clone(),
                name: name.clone(),
                library: e.library.clone(),
                fps: clip.get("fps").and_then(Value::float).unwrap_or(12.0),
                wrap: guest_wrap_of(clip, frames.len())?,
                loop_start: clip.get("loopStart").and_then(Value::int).unwrap_or(0),
                frames: seq,
            });
        }
    }
    Ok(lib)
}

// ---- palettes --------------------------------------------------------------------------------------------------------

type Rgb = [f64; 3];

/// The colour a 15-bit palette word shows (the room quantizer's `expand`).
pub fn display(word: u16) -> Rgb {
    let v = [(word & 31) as f64, ((word >> 5) & 31) as f64, ((word >> 10) & 31) as f64];
    [v[0] * 8.0 + v[0] / 4.0, v[1] * 8.0 + v[1] / 4.0, v[2] * 8.0 + v[2] / 4.0]
}

fn to15(c: &Rgb) -> u16 {
    let q = |v: f64| ((v.round_ties_even().clamp(0.0, 255.0)) as i64 >> 3) as u16;
    q(c[0]) | (q(c[1]) << 5) | (q(c[2]) << 10)
}

fn d2(a: &Rgb, b: &Rgb) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// A sprite after the room quantizer: palette words (index 0 transparent) and the index plane.
pub struct Reference {
    pub words: Vec<u16>,
    pub plane: Vec<u8>,
}

pub fn reference(sprite: &Sprite) -> Result<Reference> {
    let q = quantize::atlas_quantizer(&sprite.image, sprite.w, sprite.h)?;
    let words = q.palette.chunks(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    Ok(Reference { words, plane: q.plane })
}

/// What a texel of the sprite is shown as: its source colour (premultiplied for a semi texel) and weight class.
/// `None` for a transparent texel of the reference.
pub fn source_colour(sprite: &Sprite, r: &Reference, i: usize) -> Option<(Rgb, bool)> {
    let idx = r.plane[i] as usize;
    if idx == 0 {
        return None;
    }
    let semi = r.words[idx] & 0x8000 != 0;
    let p = &sprite.image.data[i * 4..i * 4 + 4];
    let f = if semi { p[3] as f64 / 255.0 } else { 1.0 };
    Some(([p[0] as f64 * f, p[1] as f64 * f, p[2] as f64 * f], semi))
}

/// Pixels of a group of sprites per colour word.
pub type Hist = BTreeMap<u16, f64>;

pub fn histogram(r: &Reference) -> Hist {
    let mut h = Hist::new();
    for &i in &r.plane {
        if i != 0 {
            *h.entry(r.words[i as usize]).or_insert(0.0) += 1.0;
        }
    }
    h
}

/// Weighted k-means on one class of colours, seeded deterministically (farthest point first). Returns the centres.
fn cluster(points: &[(Rgb, f64)], k: usize) -> Vec<Rgb> {
    if points.len() <= k {
        return points.iter().map(|p| p.0).collect();
    }
    let total: f64 = points.iter().map(|p| p.1).sum();
    let mean = points.iter().fold([0.0; 3], |a, p| [a[0] + p.0[0] * p.1 / total, a[1] + p.0[1] * p.1 / total, a[2] + p.0[2] * p.1 / total]);
    let mut centres: Vec<Rgb> = vec![];
    let first = points.iter().min_by(|a, b| d2(&a.0, &mean).partial_cmp(&d2(&b.0, &mean)).unwrap()).unwrap();
    centres.push(first.0);
    let mut dist: Vec<f64> = points.iter().map(|p| d2(&p.0, &centres[0])).collect();
    while centres.len() < k {
        let (at, _) = dist.iter().enumerate().map(|(i, d)| (i, d * points[i].1.sqrt())).fold((0, -1.0), |b, c| if c.1 > b.1 { c } else { b });
        centres.push(points[at].0);
        for (i, p) in points.iter().enumerate() {
            dist[i] = dist[i].min(d2(&p.0, centres.last().unwrap()));
        }
    }
    for _ in 0..12 {
        let mut sum = vec![([0.0; 3], 0.0); k];
        for p in points {
            let j = (0..k).min_by(|&a, &b| d2(&p.0, &centres[a]).partial_cmp(&d2(&p.0, &centres[b])).unwrap()).unwrap();
            for c in 0..3 {
                sum[j].0[c] += p.0[c] * p.1;
            }
            sum[j].1 += p.1;
        }
        let mut moved = false;
        for j in 0..k {
            if sum[j].1 > 0.0 {
                let n = [sum[j].0[0] / sum[j].1, sum[j].0[1] / sum[j].1, sum[j].0[2] / sum[j].1];
                if d2(&n, &centres[j]) > 0.01 {
                    moved = true;
                }
                centres[j] = n;
            }
        }
        if !moved {
            break;
        }
    }
    centres
}

fn sse(points: &[(Rgb, f64)], words: &[u16]) -> f64 {
    let colours: Vec<Rgb> = words.iter().map(|&w| display(w & 0x7fff)).collect();
    points.iter().map(|p| colours.iter().map(|c| d2(&p.0, c)).fold(f64::INFINITY, f64::min) * p.1).sum()
}

/// The best palette of at most `limit` entries (index 0 reserved: at most 15) for a histogram, split between opaque and semi
/// entries by greedy error reduction. Returns the entry words and the weighted squared error against the histogram.
pub fn best_palette(h: &Hist, limit: usize) -> (Vec<u16>, f64) {
    let split = |semi: bool| -> Vec<(Rgb, f64)> { h.iter().filter(|(w, _)| (**w & 0x8000 != 0) == semi).map(|(w, n)| (display(*w & 0x7fff), *n)).collect() };
    let class = [split(false), split(true)];
    let mut memo: [HashMap<usize, (Vec<u16>, f64)>; 2] = [HashMap::new(), HashMap::new()];
    let mut get = |c: usize, k: usize, memo: &mut [HashMap<usize, (Vec<u16>, f64)>; 2]| -> (Vec<u16>, f64) {
        if let Some(v) = memo[c].get(&k) {
            return v.clone();
        }
        let centres = cluster(&class[c], k);
        let mut words: Vec<u16> = vec![];
        for ce in &centres {
            let mut v = to15(ce);
            v = if c == 0 { if v == 0 { 1 } else { v } } else { v | 0x8000 };
            if !words.contains(&v) {
                words.push(v);
            }
        }
        let e = sse(&class[c], &words);
        memo[c].insert(k, (words.clone(), e));
        (words, e)
    };
    let mut alloc = [(!class[0].is_empty()) as usize, (!class[1].is_empty()) as usize];
    for _ in 0..limit.saturating_sub(alloc[0] + alloc[1]) {
        let mut best: Option<(usize, f64)> = None;
        for c in 0..2 {
            if alloc[c] == 0 || alloc[c] >= class[c].len() {
                continue;
            }
            let g = get(c, alloc[c], &mut memo).1 - get(c, alloc[c] + 1, &mut memo).1;
            if best.is_none_or(|b| g > b.1) {
                best = Some((c, g));
            }
        }
        match best {
            Some((c, _)) => alloc[c] += 1,
            None => break,
        }
    }
    let mut words = vec![];
    let mut err = 0.0;
    for c in 0..2 {
        if alloc[c] > 0 {
            let (w, e) = get(c, alloc[c], &mut memo);
            words.extend(w);
            err += e;
        }
    }
    (words, err)
}

/// Merge palette groups while the added error stays under `bound` (summed squared error over all texels) or until `target`
/// groups are left, whichever stops first; `target = 0` means only the bound. Returns the groups (sprite indices).
pub struct Merge {
    pub members: Vec<Vec<usize>>,
    pub hist: Vec<Hist>,
    pub words: Vec<Vec<u16>>,
    pub err: Vec<f64>,
}

pub fn merge(hists: &[Hist], target: usize, bound: f64, snapshots: &[usize], trace: &mut dyn FnMut(usize, f64, f64)) -> (Merge, Vec<(usize, Merge)>) {
    let n = hists.len();
    let mut g = Merge { members: (0..n).map(|i| vec![i]).collect(), hist: hists.to_vec(), words: vec![], err: vec![] };
    for h in hists {
        let (w, e) = best_palette(h, 15);
        g.words.push(w);
        g.err.push(e);
    }
    let union = |a: &Hist, b: &Hist| -> Hist {
        let mut u = a.clone();
        for (k, v) in b {
            *u.entry(*k).or_insert(0.0) += v;
        }
        u
    };
    // cost[i][j]: the error that merging i and j adds.
    let mut alive: Vec<bool> = vec![true; n];
    let mut cost: Vec<Vec<f64>> = vec![vec![f64::INFINITY; n]; n];
    let pair = |g: &Merge, i: usize, j: usize| -> f64 {
        let u = union(&g.hist[i], &g.hist[j]);
        best_palette(&u, 15).1 - g.err[i] - g.err[j]
    };
    for i in 0..n {
        for j in 0..i {
            cost[i][j] = pair(&g, i, j);
        }
    }
    let mut live = n;
    let mut added = 0.0;
    let mut snaps: Vec<(usize, Merge)> = vec![];
    let total_px: f64 = hists.iter().flat_map(|h| h.values()).sum();
    loop {
        if target != 0 && live <= target {
            break;
        }
        let mut best = (usize::MAX, usize::MAX, f64::INFINITY);
        for i in 0..n {
            if !alive[i] {
                continue;
            }
            for j in 0..i {
                if alive[j] && cost[i][j] < best.2 {
                    best = (i, j, cost[i][j]);
                }
            }
        }
        if best.0 == usize::MAX {
            break;
        }
        if target == 0 && added + best.2 > bound {
            break;
        }
        let (i, j, c) = best;
        let u = union(&g.hist[i], &g.hist[j]);
        let (w, e) = best_palette(&u, 15);
        g.hist[i] = u;
        g.words[i] = w;
        g.err[i] = e;
        let moved = std::mem::take(&mut g.members[j]);
        g.members[i].extend(moved);
        alive[j] = false;
        live -= 1;
        added += c;
        trace(live, added, (added / total_px).sqrt());
        if snapshots.contains(&live) {
            snaps.push((live, compact(&alive, &g)));
        }
        for k in 0..n {
            if alive[k] && k != i {
                let v = pair(&g, i, k);
                if i > k {
                    cost[i][k] = v;
                } else {
                    cost[k][i] = v;
                }
            }
        }
    }
    (compact(&alive, &g), snaps)
}

fn compact(alive: &[bool], g: &Merge) -> Merge {
    let keep: Vec<usize> = (0..alive.len()).filter(|&i| alive[i]).collect();
    Merge {
        members: keep.iter().map(|&i| g.members[i].clone()).collect(),
        hist: keep.iter().map(|&i| g.hist[i].clone()).collect(),
        words: keep.iter().map(|&i| g.words[i].clone()).collect(),
        err: keep.iter().map(|&i| g.err[i]).collect(),
    }
}

/// A sprite's plane recoloured onto a shared palette: every non-transparent texel of the reference takes the nearest entry of
/// its own class (opaque or semi). Returns the plane (indices into `[0] + words`, opaque entries first) and the squared error
/// summed over the sprite's texels against the source colour.
/// `vs_ref` measures against what the room quantizer decided each texel shows (its palette entry), which is the error a shared
/// palette adds to the standard path; otherwise against the source texel.
pub fn recolour(sprite: &Sprite, r: &Reference, words: &[u16], vs_ref: bool) -> (Vec<u8>, f64, f64, Vec<u16>) {
    // Entries are laid out as the room quantizer does: transparent, opaque ones, semi ones.
    let mut ordered: Vec<u16> = words.iter().copied().filter(|w| w & 0x8000 == 0).collect();
    ordered.extend(words.iter().copied().filter(|w| w & 0x8000 != 0));
    let colours: Vec<Rgb> = ordered.iter().map(|&w| display(w & 0x7fff)).collect();
    let mut plane = vec![0u8; r.plane.len()];
    let (mut err, mut worst) = (0.0, 0.0f64);
    for i in 0..r.plane.len() {
        let Some((mut c, semi)) = source_colour(sprite, r, i) else { continue };
        if vs_ref {
            c = display(r.words[r.plane[i] as usize] & 0x7fff);
        }
        let mut best: Option<(usize, f64)> = None;
        for (j, w) in ordered.iter().enumerate() {
            if (*w & 0x8000 != 0) != semi {
                continue;
            }
            let d = d2(&c, &colours[j]);
            if best.is_none_or(|b| d < b.1) {
                best = Some((j, d));
            }
        }
        // A class with no entry in the shared palette falls to the nearest of the other class.
        let (j, d) = best.unwrap_or_else(|| {
            colours.iter().enumerate().map(|(j, k)| (j, d2(&c, k))).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap()
        });
        plane[i] = (j + 1) as u8;
        err += d;
        worst = worst.max(d);
    }
    (plane, err, worst, ordered)
}

/// The error the reference quantizer itself leaves: its palette against the source.
pub fn reference_error(sprite: &Sprite, r: &Reference) -> (f64, f64) {
    let (mut err, mut worst) = (0.0, 0.0f64);
    for i in 0..r.plane.len() {
        let Some((c, _)) = source_colour(sprite, r, i) else { continue };
        let d = d2(&c, &display(r.words[r.plane[i] as usize] & 0x7fff));
        err += d;
        worst = worst.max(d);
    }
    (err, worst)
}

// ---- parts -----------------------------------------------------------------------------------------------------------

/// One cell of a sprite: the opaque bounding box inside a `cell` by `cell` window, as packed 4bpp rows.
pub struct Part {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub texels: Vec<u8>,
}

pub fn stride(w: usize) -> usize {
    ((w + 3) & !3) / 2
}

/// Cut a plane into `cell`-sized windows, drop the empty ones and trim the rest to their opaque box.
pub fn parts(plane: &[u8], w: usize, h: usize, cell: usize) -> Vec<Part> {
    let mut out = vec![];
    for cy in (0..h).step_by(cell) {
        for cx in (0..w).step_by(cell) {
            let (tw, th) = (cell.min(w - cx), cell.min(h - cy));
            let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
            for y in 0..th {
                for x in 0..tw {
                    if plane[(cy + y) * w + cx + x] != 0 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x + 1);
                        y1 = y1.max(y + 1);
                    }
                }
            }
            if x0 == usize::MAX {
                continue;
            }
            let (pw, ph) = (x1 - x0, y1 - y0);
            let mut texels = vec![0u8; stride(pw) * ph];
            for y in 0..ph {
                for x in 0..pw {
                    let v = plane[(cy + y0 + y) * w + cx + x0 + x];
                    texels[y * stride(pw) + x / 2] |= v << ((x & 1) * 4);
                }
            }
            out.push(Part { x: cx + x0, y: cy + y0, w: pw, h: ph, texels });
        }
    }
    out
}

/// The tokens `game/src/ability_art.rs` decodes: below 0x80 a literal of token + 1 bytes, from 0x80 a run of
/// (token & 0x7f) + 2 copies of the next byte.
pub fn packbits(src: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    let mut i = 0;
    let mut lit: Vec<u8> = vec![];
    let flush = |lit: &mut Vec<u8>, out: &mut Vec<u8>| {
        for chunk in lit.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend(chunk);
        }
        lit.clear();
    };
    while i < src.len() {
        let mut run = 1;
        while i + run < src.len() && src[i + run] == src[i] && run < 129 {
            run += 1;
        }
        if run >= 3 || (run == 2 && lit.is_empty()) {
            flush(&mut lit, &mut out);
            out.push(0x80 | (run - 2) as u8);
            out.push(src[i]);
            i += run;
        } else {
            lit.push(src[i]);
            i += 1;
        }
    }
    flush(&mut lit, &mut out);
    out
}

pub fn unpackbits(coded: &[u8], len: usize) -> Vec<u8> {
    let (mut out, mut at) = (vec![], 0);
    while out.len() < len {
        let t = coded[at];
        at += 1;
        if t < 0x80 {
            out.extend_from_slice(&coded[at..at + t as usize + 1]);
            at += t as usize + 1;
        } else {
            out.extend(std::iter::repeat_n(coded[at], (t & 0x7f) as usize + 2));
            at += 1;
        }
    }
    out.truncate(len);
    out
}

pub fn px_per_cell() -> usize {
    SLOT_PIXELS
}

pub fn round(v: f64) -> i64 {
    py_round(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_round_trips() {
        let mut data = vec![0u8; 300];
        data.extend((0..200u32).map(|i| (i * 7 % 251) as u8));
        data.extend(std::iter::repeat_n(9u8, 5));
        data.push(1);
        data.push(1);
        data.push(2);
        let coded = packbits(&data);
        assert_eq!(unpackbits(&coded, data.len()), data);
        assert!(coded.len() < data.len());
    }

    #[test]
    fn parts_trim_and_cover_every_opaque_texel() {
        let (w, h) = (70, 66);
        let mut plane = vec![0u8; w * h];
        for (x, y) in [(3, 4), (60, 10), (68, 65), (5, 5)] {
            plane[y * w + x] = 1 + ((x + y) % 15) as u8;
        }
        let ps = parts(&plane, w, h, 64);
        assert_eq!(ps.len(), 2, "the cell holding (3,4), (5,5) and (60,10), and the corner cell");
        let opaque: usize = ps
            .iter()
            .map(|p| {
                (0..p.h).map(|y| (0..p.w).filter(|&x| (p.texels[y * stride(p.w) + x / 2] >> ((x & 1) * 4)) & 15 != 0).count()).sum::<usize>()
            })
            .sum();
        assert_eq!(opaque, 4);
    }

    #[test]
    fn merging_identical_palettes_costs_nothing() {
        let mut a = Hist::new();
        a.insert(0x0421, 10.0);
        a.insert(0x7fff, 5.0);
        let (words, e) = best_palette(&a, 15);
        assert_eq!(words.len(), 2);
        assert_eq!(e, 0.0);
        let (m, _) = merge(&[a.clone(), a.clone()], 0, 1.0, &[], &mut |_, _, _| {});
        assert_eq!(m.members.len(), 1);
    }
}
