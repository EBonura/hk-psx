//! The alpha-aware 4bpp quantiser behind `Atlas._quantize`. Ported from host/quantize.py.
//!
//! Same contract as the cook's: a resized image, a 32-byte palette with index 0 = 0x0000
//! transparent, and an index plane. Each texel is classed from its own alpha (transparent,
//! opaque, or STP/Add semi); the anti-aliased edge of opaque art is re-decided against a
//! dark background prior; the 15 entries are split between the opaque and semi classes by
//! greedy SSE reduction; each class is weighted k-means in the space the GPU displays.
//!
//! Every float operation follows the order numpy performs it in (pairwise sums, the BLAS
//! for the matrix products, `default_rng` for the seeding and sampling), because the
//! decisions compare sums of squared errors and snap to 15-bit colour.

use crate::atlas::{Quantized, EDGE_REACH};
use crate::blas::{matmul_abt, matvec};
use crate::common::{err, Result};
use crate::numpy_math::{average3, row_sum3, sum};
use crate::numpy_rng::Pcg64;
use hk_pil::resample::Filter;
use hk_pil::Image;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const INK_LUMA: f64 = 48.0;
const INK_WEIGHT: f64 = 6.0;
const LUMA: [f64; 3] = [0.299, 0.587, 0.114];
const SAMPLE: usize = 8000;
const ITERS: usize = 16;
/// Background prior for edge texels: dark blue-greys typical of HK rooms.
const HK_DARK_PRIOR: [[f64; 3]; 3] = [[20.0, 22.0, 30.0], [45.0, 50.0, 65.0], [90.0, 95.0, 110.0]];

type Rgb = [f64; 3];

/// `_assign(x, c)`: for each row of `x`, the nearest row of `c` (first on ties) and its squared distance.
fn assign(x: &[Rgb], c: &[Rgb]) -> (Vec<usize>, Vec<f64>) {
    let k = c.len();
    let doubled: Vec<Rgb> = x.iter().map(|v| [2.0 * v[0], 2.0 * v[1], 2.0 * v[2]]).collect();
    let product = matmul_abt(&doubled, c);
    let cc: Vec<f64> = c.iter().map(|r| row_sum3([r[0] * r[0], r[1] * r[1], r[2] * r[2]])).collect();
    let (mut labels, mut best) = (Vec::with_capacity(x.len()), Vec::with_capacity(x.len()));
    for (i, v) in x.iter().enumerate() {
        let xx = row_sum3([v[0] * v[0], v[1] * v[1], v[2] * v[2]]);
        let (mut arg, mut min) = (0usize, f64::INFINITY);
        for j in 0..k {
            let d = (xx - product[i * k + j]) + cc[j];
            if j == 0 || d < min {
                arg = j;
                min = d;
            }
        }
        labels.push(arg);
        best.push(min);
    }
    (labels, best)
}

fn dist2(a: &Rgb, b: &Rgb) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    row_sum3([d[0] * d[0], d[1] * d[1], d[2] * d[2]])
}

/// `np.allclose(a, b)`.
fn allclose(a: &[Rgb], b: &[Rgb]) -> bool {
    a.iter().zip(b).all(|(p, q)| (0..3).all(|k| (p[k] - q[k]).abs() <= 1e-8 + 1e-5 * q[k].abs()))
}

/// numpy's `kahan_sum`, which `Generator.choice` uses to validate probabilities.
fn kahan_sum(p: &[f64]) -> f64 {
    let (mut s, mut c) = (p[0], 0.0);
    for &v in &p[1..] {
        let y = v - c;
        let t = s + y;
        c = (t - s) - y;
        s = t;
    }
    s
}

/// `rng.choice(len(p), p=p)`, with numpy's check that the probabilities sum to one.
fn choice(rng: &mut Pcg64, p: &[f64]) -> Result<usize> {
    if (kahan_sum(p) - 1.0).abs() > f64::EPSILON.sqrt() {
        return err("probabilities do not sum to 1");
    }
    Ok(rng.choice_p(p))
}

/// `_wkmeans(x, w, k, rng)`: centroids and the weighted squared error.
fn wkmeans(x: &[Rgb], w: &[f64], k: usize, rng: &mut Pcg64) -> Result<(Vec<Rgb>, f64)> {
    let n = x.len();
    if n == 0 {
        return Ok((Vec::new(), 0.0));
    }
    if n <= k {
        return Ok((x.to_vec(), 0.0));
    }
    let total = sum(w);
    let p: Vec<f64> = w.iter().map(|v| v / total).collect();
    let mut c = vec![x[choice(rng, &p)?]];
    let mut d2: Vec<f64> = x.iter().map(|v| dist2(v, &c[0])).collect();
    for _ in 1..k {
        let p: Vec<f64> = d2.iter().zip(w).map(|(a, b)| a * b).collect();
        let s = sum(&p);
        if s <= 0.0 {
            break;
        }
        let normalised: Vec<f64> = p.iter().map(|v| v / s).collect();
        let next = x[choice(rng, &normalised)?];
        c.push(next);
        for (d, v) in d2.iter_mut().zip(x) {
            let e = dist2(v, &next);
            if e < *d {
                *d = e;
            }
        }
    }
    for _ in 0..ITERS {
        let (labels, _) = assign(x, &c);
        let mut nc = c.clone();
        for (i, slot) in nc.iter_mut().enumerate() {
            let (members, weights): (Vec<Rgb>, Vec<f64>) = x.iter().zip(w).zip(&labels).filter(|(_, &l)| l == i).map(|((v, wv), _)| (*v, *wv)).unzip();
            if !members.is_empty() {
                *slot = average3(&members, &weights);
            }
        }
        if allclose(&nc, &c) {
            break;
        }
        c = nc;
    }
    let (_, d) = assign(x, &c);
    let weighted: Vec<f64> = d.iter().zip(w).map(|(v, wv)| v.max(0.0) * wv).collect();
    Ok((c, sum(&weighted)))
}

/// `_sample(x, w, rng)`: at most 8000 texels, drawn without replacement.
fn sample(x: Vec<Rgb>, w: Vec<f64>, rng: &mut Pcg64) -> (Vec<Rgb>, Vec<f64>) {
    if x.len() > SAMPLE {
        let idx = rng.choice_without_replacement(x.len(), SAMPLE);
        let scale = x.len() as f64 / SAMPLE as f64;
        (idx.iter().map(|&i| x[i as usize]).collect(), idx.iter().map(|&i| w[i as usize] * scale).collect())
    } else {
        (x, w)
    }
}

/// `to15(c)`: a colour as a 15-bit word.
fn to15(c: &Rgb) -> u16 {
    let q = |v: f64| ((v.round_ties_even().clamp(0.0, 255.0)) as i64 >> 3) as u16;
    q(c[0]) | (q(c[1]) << 5) | (q(c[2]) << 10)
}

/// `expand(word)`: a 15-bit word as the colour the GPU displays.
fn expand(word: u16) -> Rgb {
    let v = [(word & 31) as f64, ((word >> 5) & 31) as f64, ((word >> 10) & 31) as f64];
    [v[0] * 8.0 + v[0] / 4.0, v[1] * 8.0 + v[1] / 4.0, v[2] * 8.0 + v[2] / 4.0]
}

fn sq(v: f64) -> f64 {
    v * v
}

fn mean3(a: f64, b: f64, c: f64) -> f64 {
    ((a + b) + c) / 3.0
}

/// Per-prior costs of dropping a texel, keeping it transparent, or keeping it as Add-blended.
fn cost_transparent(ae: f64, ce: &Rgb) -> f64 {
    let s = |b: &Rgb| row_sum3([sq(ae * (ce[0] - b[0])), sq(ae * (ce[1] - b[1])), sq(ae * (ce[2] - b[2]))]);
    mean3(s(&HK_DARK_PRIOR[0]), s(&HK_DARK_PRIOR[1]), s(&HK_DARK_PRIOR[2]))
}

fn cost_semi(ae: f64) -> f64 {
    let s = |b: &Rgb| row_sum3([sq(ae * b[0]), sq(ae * b[1]), sq(ae * b[2])]);
    mean3(s(&HK_DARK_PRIOR[0]), s(&HK_DARK_PRIOR[1]), s(&HK_DARK_PRIOR[2]))
}

fn cost_opaque(ae: f64, bm: &Rgb) -> f64 {
    let s = |b: &Rgb| row_sum3([sq((1.0 - ae) * (b[0] - bm[0])), sq((1.0 - ae) * (b[1] - bm[1])), sq((1.0 - ae) * (b[2] - bm[2]))]);
    mean3(s(&HK_DARK_PRIOR[0]), s(&HK_DARK_PRIOR[1]), s(&HK_DARK_PRIOR[2]))
}

fn pure_black(rgb: &[Rgb], t: &[bool]) -> bool {
    let mut any = false;
    for (v, &transparent) in rgb.iter().zip(t) {
        if !transparent {
            any = true;
            if v.iter().any(|&c| c != 0.0) {
                return false;
            }
        }
    }
    any
}

/// The caller's own quantizer for pure-black art.
pub type Fallback<'a> = &'a dyn Fn(&Image, usize, usize) -> Result<Quantized>;

/// `quantize(im, w, h, fallback)`.
pub fn quantize(image: &Image, w: usize, h: usize, fallback: Option<Fallback>) -> Result<Quantized> {
    let im = image.to_rgba().resize(w, h, Filter::Lanczos);
    let n = w * h;
    // `a`: the float64 RGBA texels, whose bytes seed the generator.
    let a: Vec<[f64; 4]> = im.data.chunks_exact(4).map(|p| [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64]).collect();
    let al: Vec<f64> = a.iter().map(|p| p[3]).collect();
    let mut rgb: Vec<Rgb> = a.iter().map(|p| [p[0], p[1], p[2]]).collect();
    let mut t: Vec<bool> = al.iter().map(|&v| v < 16.0).collect();
    let mut o: Vec<bool> = al.iter().map(|&v| v >= 224.0).collect();
    let mut s: Vec<bool> = (0..n).map(|i| !t[i] && !o[i]).collect();
    // Pure-black art keeps the caller's path, decided on the source texels.
    if let Some(f) = fallback {
        if pure_black(&rgb, &t) {
            return f(&im, w, h);
        }
    }
    if EDGE_REACH > 0 && o.iter().any(|&v| v) && s.iter().any(|&v| v) {
        let reach = EDGE_REACH as i64;
        let mut near = o.clone();
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                for y in 0..h as i64 {
                    for x in 0..w as i64 {
                        let (sy, sx) = (y + dy, x + dx);
                        if sy >= 0 && sy < h as i64 && sx >= 0 && sx < w as i64 && o[(sy * w as i64 + sx) as usize] {
                            near[(y * w as i64 + x) as usize] = true;
                        }
                    }
                }
            }
        }
        let edge: Vec<usize> = (0..n).filter(|&i| s[i] && near[i]).collect();
        if !edge.is_empty() {
            let bm = {
                let b = &HK_DARK_PRIOR;
                [((b[0][0] + b[1][0]) + b[2][0]) / 3.0, ((b[0][1] + b[1][1]) + b[2][1]) / 3.0, ((b[0][2] + b[1][2]) + b[2][2]) / 3.0]
            };
            for &i in &edge {
                let ae = al[i] / 255.0;
                let ce = rgb[i];
                let (c_t, c_o, c_s) = (cost_transparent(ae, &ce), cost_opaque(ae, &bm), cost_semi(ae));
                // argmin over [cT, cO, cS], first on ties.
                let pick = if c_t <= c_o && c_t <= c_s {
                    0
                } else if c_o <= c_s {
                    1
                } else {
                    2
                };
                match pick {
                    0 => {
                        t[i] = true;
                        s[i] = false;
                    }
                    1 => {
                        for k in 0..3 {
                            rgb[i][k] = ae * ce[k] + (1.0 - ae) * bm[k];
                        }
                        o[i] = true;
                        s[i] = false;
                    }
                    _ => {}
                }
            }
        }
    }
    // Interior semi texels never become opaque, but Add can only brighten: where a texel is
    // darker than the background it covers, dropping it costs less than adding it.
    let interior: Vec<usize> = (0..n).filter(|&i| s[i]).collect();
    for &i in &interior {
        let ai = al[i] / 255.0;
        if cost_transparent(ai, &rgb[i]) < cost_semi(ai) {
            t[i] = true;
            s[i] = false;
        }
    }
    if let Some(f) = fallback {
        if pure_black(&rgb, &t) {
            return f(&im, w, h);
        }
    }
    let mut seed_bytes = Vec::with_capacity(n * 32);
    for p in &a {
        for v in p {
            seed_bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    let digest = Sha256::digest(&seed_bytes);
    let mut rng = Pcg64::new(u64::from_le_bytes(digest[..8].try_into().unwrap()));
    let xo: Vec<Rgb> = (0..n).filter(|&i| o[i]).map(|i| rgb[i]).collect();
    let luma = matvec(&xo, LUMA);
    let wo: Vec<f64> = luma.iter().map(|&v| if v < INK_LUMA { INK_WEIGHT } else { 1.0 }).collect();
    let xs: Vec<Rgb> = (0..n).filter(|&i| s[i]).map(|i| {
        let f = al[i] / 255.0;
        [rgb[i][0] * f, rgb[i][1] * f, rgb[i][2] * f]
    }).collect();
    let ws = vec![1.0; xs.len()];
    let (so, swo) = sample(xo.clone(), wo, &mut rng);
    let (ss, sws) = sample(xs.clone(), ws, &mut rng);
    let mut curves: HashMap<(bool, usize), (Vec<Rgb>, f64)> = HashMap::new();
    let mut curve = |opaque: bool, k: usize, rng: &mut Pcg64| -> Result<(Vec<Rgb>, f64)> {
        if !curves.contains_key(&(opaque, k)) {
            let r = if opaque { wkmeans(&so, &swo, k, rng)? } else { wkmeans(&ss, &sws, k, rng)? };
            curves.insert((opaque, k), r);
        }
        Ok(curves[&(opaque, k)].clone())
    };
    let mut alloc = [(xo.len() > 0) as usize, (xs.len() > 0) as usize];
    let budget = 15 - (alloc[0] + alloc[1]);
    for _ in 0..budget {
        let (mut best, mut gain) = (None, -1.0f64);
        for (cls, count) in [(0usize, xo.len()), (1usize, xs.len())] {
            let k = alloc[cls];
            if k == 0 || k >= count {
                continue;
            }
            let g = curve(cls == 0, k, &mut rng)?.1 - curve(cls == 0, k + 1, &mut rng)?.1;
            if g > gain {
                best = Some(cls);
                gain = g;
            }
        }
        let Some(b) = best else { break };
        alloc[b] += 1;
    }
    let mut words: Vec<u16> = vec![0];
    let mut plane = vec![0u8; n];
    for (cls, mask, x) in [(0usize, &o, &xo), (1usize, &s, &xs)] {
        if alloc[cls] == 0 {
            continue;
        }
        let cents = curve(cls == 0, alloc[cls], &mut rng)?.0;
        let mut cw: Vec<u16> = cents.iter().map(to15).collect();
        for v in &mut cw {
            *v = if cls == 0 { if *v == 0 { 1 } else { *v } } else { *v | 0x8000 };
        }
        let mut uniq: Vec<u16> = Vec::new();
        for v in cw {
            if !uniq.contains(&v) {
                uniq.push(v);
            }
        }
        let disp: Vec<Rgb> = uniq.iter().map(|&v| expand(v & 0x7fff)).collect();
        let (lab, _) = assign(x, &disp);
        let base = words.len();
        words.extend(&uniq);
        let mut it = lab.iter();
        for i in 0..n {
            if mask[i] {
                plane[i] = (it.next().unwrap() + base) as u8;
            }
        }
    }
    words.resize(16, 0);
    Ok(Quantized { image: im, palette: words.iter().flat_map(|v| v.to_le_bytes()).collect(), plane })
}
