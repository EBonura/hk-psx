//! What a title style draws over its baked picture: its particles and its animated sprites.
//!
//! The picture itself (`title_style.rs`) is a still. A style's `Animator`s and particle systems are
//! cooked here into one blob per style, read from the disc with the style and played by menu.rs:
//!
//! ```text
//! header    "HKX1", uploads, emitters, layers (one byte each)                    8 bytes
//! uploads   x, y, w, h (halfwords, rows) and the offset of the rows (u32)       12 bytes each
//! emitters  28-byte header, then per slot a life byte and four tracks           see `EMITTER_HEAD`
//! layers    32-byte header, then 4 bytes a sample                               see `LAYER_HEAD`
//! payload   the rows each upload carries
//! ```
//!
//! All integers are little endian. The pieces (particles, flipbook frames) sit in 256x256 texture
//! pages at x 768 and 832 (4bpp) and 896 (8bpp), rows 0 to 255 of video memory, which the title alone
//! uses; an 8bpp page's palette rides at (768, 479). Coverage is the 16-level ramp the title's fonts
//! share (palette at (320, 481)), drawn tinted and added or subtracted.
//!
//! `play` is the reference of what menu.rs does with the blob, and draws the preview stills.
use crate::common::Result;
use crate::title_clips;
use crate::title_particles::{self, Blend, Rng};
use crate::title_style::{inverse_homography, Layer, HEIGHT, WIDTH};
use hk_pil::resample::Filter;
use hk_pil::Image;
use hk_unity::scene::Scene;
use hk_unity::Source;

/// x of each texture page and its depth in bits.
pub const PAGES: [(u16, u8); 3] = [(768, 4), (832, 4), (896, 8)];
/// Where an 8bpp page's palette is uploaded.
pub const PALETTE_AT: (u16, u16) = (768, 479);
/// Tracks per slot: a slot plays one of them per life.
pub const VARIANTS: usize = 4;
/// The most slots one emitter keeps.
pub const MAX_SLOTS: usize = 96;
/// What the particles and the animated layers may ask of the GPU in a frame, in the emulator's GPU
/// cycles (a quad costs about 230 and 1.9 a pixel): with the picture and the text the title stays
/// inside one video period.
const PARTICLE_BUDGET: f64 = 190_000.0;
pub const EMITTER_HEAD: usize = 28;
pub const LAYER_HEAD: usize = 32;
pub const TRACK_BYTES: usize = 16;
/// Frames of a particle texture sheet per row of its piece.
const COLS: usize = 3;

/// A shelf-packed 256x256 texture page, one byte a texel.
struct Page {
    texels: Vec<u8>,
    x: usize,
    y: usize,
    shelf: usize,
    rows: usize,
}

impl Page {
    fn new() -> Self {
        Self { texels: vec![0; 256 * 256], x: 0, y: 0, shelf: 0, rows: 0 }
    }
    /// Claim a `w` x `h` rectangle; its top-left, or `None` when the page has no room.
    fn claim(&mut self, w: usize, h: usize) -> Option<(usize, usize)> {
        if w > 256 || h > 256 {
            return None;
        }
        if self.x + w > 256 {
            self.x = 0;
            self.y += self.shelf;
            self.shelf = 0;
        }
        if self.y + h > 256 {
            return None;
        }
        let at = (self.x, self.y);
        self.x += w;
        self.shelf = self.shelf.max(h);
        self.rows = self.rows.max(self.y + h);
        Some(at)
    }
    fn blit(&mut self, at: (usize, usize), w: usize, h: usize, indices: &[u8]) {
        for row in 0..h {
            self.texels[(at.1 + row) * 256 + at.0..][..w].copy_from_slice(&indices[row * w..][..w]);
        }
    }
    /// The rows as video memory holds them.
    fn rows_bytes(&self, depth: u8) -> Vec<u8> {
        let mut out = Vec::new();
        for y in 0..self.rows {
            let row = &self.texels[y * 256..y * 256 + 256];
            if depth == 4 {
                out.extend(row.chunks(2).map(|p| p[0] | (p[1] << 4)));
            } else {
                out.extend_from_slice(row);
            }
        }
        out
    }
}

/// The result: the blob and what to report.
pub struct Extra {
    pub blob: Vec<u8>,
    pub report: Vec<String>,
    /// VRAM bytes the style's pieces occupy.
    pub vram_bytes: usize,
}

/// The 16-level coverage a texel becomes.
fn level(a: u8, v: u8) -> u8 {
    ((u32::from(a) * u32::from(v) * 15 + 127 * 255) / (255 * 255)) as u8
}

fn mono(image: &Image) -> Vec<u8> {
    image.data.chunks_exact(4).map(|p| level(p[3], p[0].max(p[1]).max(p[2]))).collect()
}

/// A light blur of 16-level coverage (a caustic is magnified five times, so each texel step shows).
fn soften(grey: &[u8], w: usize, h: usize) -> Vec<u8> {
    let at = |x: i64, y: i64| i32::from(grey[y.clamp(0, h as i64 - 1) as usize * w + x.clamp(0, w as i64 - 1) as usize]);
    (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            let sum = 4 * at(x, y) + 2 * (at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1)) + at(x - 1, y - 1) + at(x + 1, y - 1) + at(x - 1, y + 1) + at(x + 1, y + 1);
            ((sum + 8) / 16) as u8
        })
        .collect()
}

fn mean_colour(image: &Image) -> [f64; 3] {
    let (mut s, mut w) = ([0.0; 3], 1e-9);
    for p in image.data.chunks_exact(4) {
        let a = f64::from(p[3]) / 255.0;
        for k in 0..3 {
            s[k] += f64::from(p[k]) / 255.0 * a;
        }
        w += a;
    }
    s.map(|v| v / w)
}

fn texture_of(source: &Source, file: &str, id: i64) -> Result<Image> {
    let file = source.file(file).map_err(|e| e.to_string())?;
    let object = source.object(&file, id).map_err(|e| e.to_string())?;
    Ok(hk_unity::texture::texture_image(source, &object, false).map_err(|e| e.to_string())?.to_rgba())
}

/// A particle texture, by name: where it lives and the size of a frame on the page.
struct Texture {
    name: &'static str,
    file: &'static str,
    id: i64,
    side: (usize, usize),
}

const TEXTURES: [Texture; 6] = [
    Texture { name: "default_mote", file: "resources.assets", id: 415, side: (16, 16) },
    Texture { name: "particle_01", file: "resources.assets", id: 420, side: (8, 8) },
    Texture { name: "wispy_smoke_particle_abyss", file: "resources.assets", id: 367, side: (64, 64) },
    Texture { name: "wispy_smoke_particle_effect", file: "resources.assets", id: 342, side: (64, 64) },
    Texture { name: "dream_particle_01", file: "resources.assets", id: 317, side: (32, 32) },
    Texture { name: "ember_particle", file: "sharedassets1.assets", id: 16, side: (24, 24) },
];

/// How a layer of the scene animates, for the styles that have animators.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Live {
    /// `GG_menu_disc_ebb`: the half-transparent disc's alpha breathes.
    Pulse,
    /// `waterfall`: six sprites of the sheet.
    Waterfall,
    /// `caustic_small_0`: 48 sprites of the sheet, the second copy half a loop later.
    Caustic { late: bool },
    /// `infected_blob_anim`: a dark tendril sways.
    Drift { second: bool },
}

/// The live layers of a style, by the layer's name in the scene.
pub fn live_kind(style: &str, layer: &Layer) -> Option<Live> {
    let name = layer.name.as_str();
    match style {
        "GG_Style" if name.ends_with("back_disc_50_percent") => Some(Live::Pulse),
        "Brilliant_Style" if name.ends_with("Waterfall_base0000") => Some(Live::Waterfall),
        "Brilliant_Style" if name.ends_with("caustic_small_000 (1)") => Some(Live::Caustic { late: true }),
        "Brilliant_Style" if name.ends_with("caustic_small_000") => Some(Live::Caustic { late: false }),
        "Infected_Style" if name.ends_with("Layer-1 (1)") => Some(Live::Drift { second: true }),
        "Infected_Style" if name.ends_with("Layer-1") => Some(Live::Drift { second: false }),
        _ => None,
    }
}

fn mean_under(canvas: &[[f64; 3]], corners: &[[f64; 2]; 4], image: &Image) -> [f64; 3] {
    // The picture under the sprite, weighted by its alpha, sampled on a grid of the sprite.
    let Some(inv) = inverse_homography(corners) else { return [0.0; 3] };
    let (mut sum, mut wsum) = ([0.0; 3], 1e-9);
    for y in (0..HEIGHT).step_by(2) {
        for x in (0..WIDTH).step_by(2) {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let w = inv[6] * px + inv[7] * py + inv[8];
            let (u, v) = ((inv[0] * px + inv[1] * py + inv[2]) / w, (inv[3] * px + inv[4] * py + inv[5]) / w);
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let a = f64::from(image.pixel((u * image.width as f64) as usize, (v * image.height as f64) as usize)[3]) / 255.0;
            for k in 0..3 {
                sum[k] += canvas[y * WIDTH + x][k] * a;
            }
            wsum += a;
        }
    }
    sum.map(|v| v / wsum)
}

struct Builder {
    pages: [Page; 3],
    palette: Vec<u16>,
    pieces: std::collections::HashMap<&'static str, (usize, usize, usize, usize, usize)>,
}

impl Builder {
    /// A particle texture cooked to `frames` frames side by side: (page, u, v, frame w, frame h).
    fn piece(&mut self, source: &Source, name: &str, frames: usize) -> Result<(usize, usize, usize, usize, usize)> {
        let t = TEXTURES.iter().find(|t| t.name == name).ok_or_else(|| format!("no particle texture {name}"))?;
        if let Some(&p) = self.pieces.get(t.name) {
            return Ok(p);
        }
        let image = texture_of(source, t.file, t.id)?;
        let frames = frames.max(1);
        let frame_h = image.height / frames;
        let cols = frames.min(COLS);
        let (w, h) = (t.side.0 * cols, t.side.1 * frames.div_ceil(cols));
        let mut strip = vec![0u8; w * h];
        for k in 0..frames {
            let frame = image
                .crop_int(0, (k * frame_h) as i64, image.width as i64, ((k + 1) * frame_h) as i64)
                .resize(t.side.0, t.side.1, Filter::Lanczos);
            let grey = mono(&frame);
            for y in 0..t.side.1 {
                let at = (k / cols * t.side.1 + y) * w + (k % cols) * t.side.0;
                strip[at..at + t.side.0].copy_from_slice(&grey[y * t.side.0..][..t.side.0]);
            }
        }
        for page in 0..2 {
            if let Some(at) = self.pages[page].claim(w, h) {
                self.pages[page].blit(at, w, h, &strip);
                let out = (page, at.0, at.1, t.side.0, t.side.1);
                self.pieces.insert(t.name, out);
                return Ok(out);
            }
        }
        Err(format!("{name}: no room on the pieces' pages"))
    }
}

fn put16(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// The flipbook of colour frames over the picture, quantised to one shared palette of at most 63
/// colours plus the transparent entry 0. A pixel the sprite covers is blended over the picture behind
/// it, so the frame is opaque there.
fn quantise(frames: &[Image], behind: &[[f64; 3]], rect: (i32, i32)) -> (Vec<u16>, Vec<Vec<u8>>) {
    let (w, h) = (frames[0].width, frames[0].height);
    let mut opaque: Vec<[u8; 4]> = Vec::new();
    let mut maps: Vec<Vec<Option<usize>>> = Vec::new();
    for image in frames {
        let mut map = Vec::new();
        for (i, p) in image.data.chunks_exact(4).enumerate() {
            let a = f64::from(p[3]) / 255.0;
            if a < 0.12 {
                map.push(None);
                continue;
            }
            let (x, y) = (rect.0 + (i % w) as i32, rect.1 + (i / w) as i32);
            let b = if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) { behind[y as usize * WIDTH + x as usize] } else { [0.0; 3] };
            let c = [0, 1, 2].map(|k| ((f64::from(p[k]) / 255.0 * a + b[k] * (1.0 - a)) * 255.0).round().clamp(0.0, 255.0) as u8);
            map.push(Some(opaque.len()));
            opaque.push([c[0], c[1], c[2], 255]);
        }
        maps.push(map);
    }
    let _ = h;
    let (palette, indices) = hk_pil::octree::fast_octree(&opaque, 63);
    let word = |c: [u8; 4]| -> u16 {
        let w = u16::from(c[0] >> 3) | u16::from(c[1] >> 3) << 5 | u16::from(c[2] >> 3) << 10;
        // 0x0000 is the transparent entry; a black that must show becomes the faintest grey.
        if w == 0 {
            0x0421
        } else {
            w
        }
    };
    let mut table = vec![0u16];
    table.extend(palette.iter().map(|&c| word(c)));
    let out = maps.iter().map(|m| m.iter().map(|at| at.map_or(0, |i| indices[i] + 1)).collect()).collect();
    (table, out)
}

/// What a slot asks of the GPU in an average frame (menu.rs `emitter` is what is being costed): over its
/// life and four tracks, a quad on the screen with some strength costs its setup and a cycle or two a pixel.
fn slot_cost(slot: &[u8], alpha: &[u8; 9], size: &[u8; 9]) -> f64 {
    let lerp = |lut: &[u8; 9], u: f64| {
        let x = (u * 8.0).min(7.999);
        let i = x as usize;
        f64::from(lut[i]) * (1.0 - (x - i as f64)) + f64::from(lut[i + 1]) * (x - i as f64)
    };
    let rd = |t: &[u8], at: usize| f64::from(i16::from_le_bytes([t[at], t[at + 1]])) / 4.0;
    let mut sum = 0.0;
    let samples = 16;
    for v in 0..VARIANTS {
        let t = &slot[1 + v * TRACK_BYTES..1 + (v + 1) * TRACK_BYTES];
        for k in 0..samples {
            let u = (f64::from(k) + 0.5) / f64::from(samples);
            let at = |p: usize| rd(t, p) + rd(t, p + 4) * u + rd(t, p + 8) * u * u;
            let side = f64::from(t[12]) * lerp(size, u) / 255.0;
            let strength = f64::from(t[13].max(t[14]).max(t[15])) * lerp(alpha, u) / 255.0;
            if side < 1.0 || strength < 2.0 {
                continue;
            }
            let (x, y) = (at(0), at(2));
            let w = ((x + side / 2.0).min(WIDTH as f64) - (x - side / 2.0).max(0.0)).max(0.0);
            let h = ((y + side / 2.0).min(HEIGHT as f64) - (y - side / 2.0).max(0.0)).max(0.0);
            if w > 0.0 && h > 0.0 {
                sum += 230.0 + 1.9 * w * h;
            }
        }
    }
    sum / f64::from(VARIANTS as u32 * samples)
}

fn clamp16(v: f64) -> i16 {
    v.round().clamp(-32000.0, 32000.0) as i16
}

/// Cook a style's extras. `canvas` is the baked picture (without the live layers).
pub fn build(
    source: &Source,
    scene: &Scene,
    style: &str,
    index: usize,
    layers: &[(Layer, Image)],
    canvas: &[[f64; 3]],
) -> Result<Extra> {
    let (cam, focal) = crate::title_style::camera(scene)?;
    let mut b = Builder { pages: [Page::new(), Page::new(), Page::new()], palette: Vec::new(), pieces: Default::default() };
    let mut report = Vec::new();
    // The animated layers first: they claim whole grids of the pages.
    let sheet = source.file("sharedassets1.assets").map_err(|e| e.to_string())?;
    let sprite = |id: i64| -> Result<Image> {
        let o = source.object(&sheet, id).map_err(|e| e.to_string())?;
        Ok(crate::cook::native_sprite(source, &o)?.0.to_rgba())
    };
    let clip = |id: i64| -> Result<title_clips::Clip> {
        let o = source.object(&sheet, id).map_err(|e| e.to_string())?;
        Ok(title_clips::read(&source.read(&o).map_err(|e| e.to_string())?))
    };
    let mut layer_blobs: Vec<Vec<u8>> = Vec::new();
    let mut layer_costs: Vec<f64> = Vec::new();
    let mut caustic: Option<(usize, usize)> = None;
    for (layer, image) in layers {
        let Some(kind) = live_kind(style, layer) else { continue };
        let corners = layer.corners;
        let mut samples: Vec<[u8; 4]> = Vec::new();
        // (page, blend, u, v, frame w, frame h, columns, frames per page, hold, tint)
        let (page, blend, at, size, cols, per_page, hold, tint): (usize, u8, (usize, usize), (usize, usize), usize, usize, u8, [f64; 3]);
        match kind {
            Live::Pulse => {
                // The disc is baked at its first alpha; the pulse adds what the alpha gains over it.
                let c = clip(86)?;
                let curve = c.curves.iter().find(|k| !k.pptr).ok_or("the disc's clip has no curve")?;
                let small = image.resize(64, 64, Filter::Lanczos);
                let grey = mono(&small);
                let u = b.pages[0].claim(64, 64).ok_or("the disc does not fit")?;
                b.pages[0].blit(u, 64, 64, &grey);
                (page, blend, at, size, cols, per_page, hold) = (0, 0, u, (64, 64), 1, 0, 4);
                tint = mean_colour(image);
                let base = f64::from(curve.at(0.0));
                for s in 0..(c.length * 60.0 / f32::from(hold)).round() as usize {
                    let a = f64::from(curve.at(s as f32 * f32::from(hold) / 60.0));
                    // The baked layer holds `base`; the strength the sprite shows is the extra alpha.
                    let gain = (a - base).max(0.0) * 128.0 * 1.6;
                    samples.push([0, gain.round().clamp(0.0, 255.0) as u8, 0, 0]);
                }
            }
            Live::Waterfall => {
                let c = clip(87)?;
                let curve = c.curves.iter().find(|k| k.pptr).ok_or("the waterfall's clip has no sprites")?;
                let (w, h) = (112usize, 59usize);
                let frames: Vec<Image> = c.sprites.iter().map(|&id| sprite(id).map(|f| f.resize(w, h, Filter::Lanczos))).collect::<Result<_>>()?;
                // The picture behind, at the cooked size.
                let rect = (corners[0][0].round() as i32, corners[0][1].round() as i32);
                let full = (corners[3][0] - corners[0][0], corners[3][1] - corners[0][1]);
                let mut behind = vec![[0.0f64; 3]; WIDTH * HEIGHT];
                behind.copy_from_slice(canvas);
                let scaled: Vec<Image> = frames.iter().map(|f| f.resize(full.0.round() as usize, full.1.round() as usize, Filter::Lanczos)).collect();
                let (palette, idx) = quantise(&scaled, &behind, rect);
                // Re-cut the indices at the stored size by nearest sampling of the full-size frame.
                let stored: Vec<Vec<u8>> = idx
                    .iter()
                    .map(|rows| {
                        let mut out = vec![0u8; w * h];
                        for y in 0..h {
                            for x in 0..w {
                                out[y * w + x] = rows[(y * scaled[0].height / h) * scaled[0].width + x * scaled[0].width / w];
                            }
                        }
                        out
                    })
                    .collect();
                let grid_cols = 2usize;
                let grid_rows = stored.len().div_ceil(grid_cols);
                let u = b.pages[2].claim(grid_cols * w, grid_rows * h).ok_or("the waterfall does not fit its page")?;
                for (k, rows) in stored.iter().enumerate() {
                    b.pages[2].blit((u.0 + (k % grid_cols) * w, u.1 + (k / grid_cols) * h), w, h, rows);
                }
                b.palette = palette;
                (page, blend, at, size, cols, per_page, hold, tint) = (2, 3, u, (w, h), grid_cols, 0, 4, [1.0; 3]);
                for s in 0..(c.length * 60.0 / f32::from(hold)).round().max(1.0) as usize {
                    let f = curve.frame_at(s as f32 * f32::from(hold) / 60.0).min(stored.len() - 1);
                    samples.push([f as u8, 128, 0, 0]);
                }
            }
            Live::Caustic { late } => {
                let c = clip(88)?;
                // Every second sprite of the 48 (12 a second becomes 6): 16 frames fill a page, 8 more the next.
                let side = 64usize;
                let kept: Vec<i64> = c.sprites.iter().copied().step_by(2).collect();
                let origin = match caustic {
                    Some(o) => o,
                    None => {
                        let u0 = b.pages[0].claim(256, 256).ok_or("caustics: no page")?;
                        let u1 = b.pages[1].claim(256, 128).ok_or("caustics: no second page")?;
                        for (k, &id) in kept.iter().enumerate() {
                            let f = sprite(id)?.resize(side, side, Filter::Lanczos);
                            let grey = soften(&mono(&f), side, side);
                            let (p, i) = (k / 16, k % 16);
                            let base = if p == 0 { u0 } else { u1 };
                            b.pages[p].blit((base.0 + (i % 4) * side, base.1 + (i / 4) * side), side, side, &grey);
                        }
                        caustic = Some((0, 0));
                        (0, 0)
                    }
                };
                let _ = origin;
                (page, blend, at, size, cols, per_page, hold) = (0, 0, (0, 0), (side, side), 4, 16, 10);
                tint = [f64::from(layer.colour[0]), f64::from(layer.colour[1]), f64::from(layer.colour[2])];
                let n = kept.len();
                for s in 0..n {
                    let f = (s + if late { n / 2 } else { 0 }) % n;
                    samples.push([f as u8, (f64::from(layer.colour[3]) * 96.0).round().clamp(0.0, 255.0) as u8, 0, 0]);
                }
            }
            Live::Drift { second } => {
                let c = clip(89)?;
                let path = title_clips::crc32(if second { "infected_menu_pieces_0000_Layer-1 (1)" } else { "infected_menu_pieces_0000_Layer-1" });
                let xs = c.curves.iter().find(|k| k.path == path && k.sub == 0);
                let ys = c.curves.iter().find(|k| k.path == path && k.sub == 1);
                // The tendril's texture at about the aspect of the sprite, at most 160 texels a side.
                let (sw, sh) = (image.width as f64, image.height as f64);
                let k = (160.0 / sw.max(sh)).min(1.0);
                let (w, h) = (((sw * k).round() as usize).max(1), ((sh * k).round() as usize).max(1));
                let small = image.resize(w, h, Filter::Lanczos);
                // The tendril is dark: its coverage is its alpha.
                let grey: Vec<u8> = small.data.chunks_exact(4).map(|p| level(p[3], 255)).collect();
                let u = b.pages[1].claim(w, h).ok_or("a tendril does not fit")?;
                b.pages[1].blit(u, w, h, &grey);
                (page, blend, at, size, cols, per_page, hold) = (1, 1, u, (w, h), 1, 0, 3);
                tint = mean_under(canvas, &corners, image);
                // A child's local position moves with its parent's world scale (the moon layer, 14.575 x 1.101).
                let px_per_unit = focal / layer.depth * 14.575 * 1.101;
                let (x0, y0) = (xs.map_or(0.0, |k| f64::from(k.at(0.0))), ys.map_or(0.0, |k| f64::from(k.at(0.0))));
                for s in 0..(c.length * 60.0 / f32::from(hold)).round() as usize {
                    let t = s as f32 * f32::from(hold) / 60.0;
                    let dx = xs.map_or(0.0, |k| f64::from(k.at(t))) - x0;
                    let dy = ys.map_or(0.0, |k| f64::from(k.at(t))) - y0;
                    let q = |v: f64| v.round().clamp(-127.0, 127.0) as i8 as u8;
                    samples.push([0, 128, q(dx * px_per_unit), q(-dy * px_per_unit)]);
                }
            }
        }
        let mut head = vec![page as u8, blend, at.0 as u8, at.1 as u8, size.0 as u8, size.1 as u8, cols as u8, per_page as u8];
        for c in &corners {
            put16(&mut head, clamp16(c[0]));
            put16(&mut head, clamp16(c[1]));
        }
        head.extend_from_slice(&tint.map(|c| (c * 128.0).round().clamp(0.0, 255.0) as u8));
        head.push(0);
        head.extend_from_slice(&(samples.len() as u16).to_le_bytes());
        head.push(hold);
        head.push(0);
        assert_eq!(head.len(), LAYER_HEAD);
        for s in &samples {
            head.extend_from_slice(s);
        }
        // The layer's cost: its setup and the screen pixels it covers (sampled), at about 1.4 cycles a pixel.
        let covered = inverse_homography(&corners).map_or(0.0, |inv| {
            let mut n = 0;
            for y in (0..HEIGHT).step_by(8) {
                for x in (0..WIDTH).step_by(8) {
                    let (px, py) = (x as f64 + 4.0, y as f64 + 4.0);
                    let w = inv[6] * px + inv[7] * py + inv[8];
                    let (u, v) = ((inv[0] * px + inv[1] * py + inv[2]) / w, (inv[3] * px + inv[4] * py + inv[5]) / w);
                    n += usize::from((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v));
                }
            }
            n as f64 * 64.0
        });
        layer_costs.push(230.0 + 1.4 * covered);
        report.push(format!("layer {}: {} samples, {} bytes, covers {covered:.0} px", layer.name, samples.len(), head.len()));
        layer_blobs.push(head);
    }
    // The particles: every system's slots first, then the frame's drawing budget is applied.
    struct Built {
        head: Vec<u8>,
        slots: Vec<(Vec<u8>, f64)>,
        path: String,
    }
    let mut built: Vec<Built> = Vec::new();
    let systems = title_particles::systems(source, scene, style)?;
    for (n, sys) in systems.iter().enumerate() {
        let mut rng = Rng::new(((index as u64) << 8) | n as u64);
        let alive = sys.alive();
        let tries = 300;
        let mut seen = 0;
        for _ in 0..tries {
            let life = rng.between(sys.life.low, sys.life.high);
            if sys.particle(&mut rng, life, cam, focal, canvas).is_some_and(|t| t.on_screen > 0.0) {
                seen += 1;
            }
        }
        let slots = ((alive * f64::from(seen) / f64::from(tries)).round() as usize).min(MAX_SLOTS);
        if slots == 0 {
            report.push(format!("{}: {alive:.0} alive, {seen}/{tries} cross the view, no slots", sys.path));
            continue;
        }
        let (page, u, v, w, h) = b.piece(source, &sys.texture, sys.frames)?;
        let lut = |f: &dyn Fn(f64) -> f64| -> [u8; 9] { std::array::from_fn(|k| (f(k as f64 / 8.0).clamp(0.0, 1.0) * 255.0).round() as u8) };
        let (alpha, size) = (
            lut(&|u| sys.over_life.as_ref().map_or(1.0, |g| g.alpha(u))),
            lut(&|u| sys.size_over_life.as_ref().map_or(1.0, |c| title_particles::eval_curve(c, u))),
        );
        // A particle never draws larger than `maxParticleSize` of the viewport, whatever its size curve does.
        let curve_peak = f64::from(*size.iter().max().unwrap_or(&255)).max(1.0) / 255.0;
        let cap = sys.max_size * HEIGHT as f64 / curve_peak;
        let mut head = vec![page as u8, u as u8, v as u8, w as u8, h as u8, sys.frames as u8, u8::from(sys.blend == Blend::Darken), 0];
        head.extend(alpha);
        head.extend(size);
        head.extend_from_slice(&[sys.frames.min(COLS) as u8, 0]);
        assert_eq!(head.len(), EMITTER_HEAD);
        let mut made = Vec::new();
        for _ in 0..slots {
            let life = rng.between(sys.life.low, sys.life.high);
            let mut blob = vec![(life * 30.0).round().clamp(1.0, 255.0) as u8];
            for _ in 0..VARIANTS {
                let mut chosen = None;
                for _ in 0..400 {
                    if let Some(t) = sys.particle(&mut rng, life, cam, focal, canvas).filter(|t| t.on_screen > 0.12) {
                        chosen = Some(t);
                        break;
                    }
                }
                let t = chosen.ok_or_else(|| format!("{}: no visible particle for a slot", sys.path))?;
                for v in [t.p0[0], t.p0[1], t.b[0], t.b[1], t.c[0], t.c[1]] {
                    put16(&mut blob, clamp16(v * 4.0));
                }
                blob.push(t.size.min(cap).round().clamp(1.0, 255.0) as u8);
                blob.extend(t.colour.map(|c| (c * 128.0).round().clamp(0.0, 255.0) as u8));
            }
            let cost = slot_cost(&blob, &alpha, &size);
            made.push((blob, cost));
        }
        report.push(format!(
            "{}: {alive:.0} alive, {seen}/{tries} cross the view, {slots} slots, {:.0} GPU cycles a frame",
            sys.path,
            made.iter().map(|m| m.1).sum::<f64>()
        ));
        built.push(Built { head, slots: made, path: sys.path.clone() });
    }
    // The drawing budget: the title's frame must stay inside one video period, which the emulator's
    // GPU model puts near 400,000 GPU cycles with the picture and the text (about 190,000 of them) already
    // in it. The layers (the caustics are screen sized) are paid first, then every emitter keeps the
    // same share of its slots.
    let layer_cost: f64 = layer_costs.iter().sum();
    let budget = (PARTICLE_BUDGET - layer_cost).max(40_000.0);
    let total: f64 = built.iter().flat_map(|e| e.slots.iter().map(|s| s.1)).sum();
    let keep = (budget / total.max(1.0)).min(1.0);
    report.push(format!("budget: layers {layer_cost:.0}, particles {total:.0} of {budget:.0}: keep {:.0}%", keep * 100.0));
    let mut emitter_blobs: Vec<Vec<u8>> = Vec::new();
    for mut e in built {
        if e.slots.iter().all(|s| s.1 == 0.0) {
            report.push(format!("{}: draws nothing the display can show, left out", e.path));
            continue;
        }
        let n = ((e.slots.len() as f64 * keep).ceil() as usize).clamp(1.min(e.slots.len()), e.slots.len());
        if n < e.slots.len() {
            report.push(format!("{}: {} of {} slots kept", e.path, n, e.slots.len()));
        }
        e.slots.truncate(n);
        e.head[7] = n as u8;
        let mut blob = e.head;
        for (slot, _) in e.slots {
            blob.extend(slot);
        }
        emitter_blobs.push(blob);
    }
    // Assemble.
    let mut uploads: Vec<(u16, u16, u16, u16, Vec<u8>)> = Vec::new();
    for (p, &(x, depth)) in PAGES.iter().enumerate() {
        if b.pages[p].rows > 0 {
            uploads.push((x, 0, if depth == 4 { 64 } else { 128 }, b.pages[p].rows as u16, b.pages[p].rows_bytes(depth)));
        }
    }
    if !b.palette.is_empty() {
        let bytes: Vec<u8> = b.palette.iter().flat_map(|w| w.to_le_bytes()).collect();
        uploads.push((PALETTE_AT.0, PALETTE_AT.1, b.palette.len() as u16, 1, bytes));
    }
    let vram_bytes = uploads.iter().map(|u| u.4.len()).sum();
    let emitters_len: usize = emitter_blobs.iter().map(Vec::len).sum();
    let layers_len: usize = layer_blobs.iter().map(Vec::len).sum();
    let mut blob = Vec::new();
    blob.extend_from_slice(b"HKX1");
    blob.extend_from_slice(&[uploads.len() as u8, emitter_blobs.len() as u8, layer_blobs.len() as u8, 0]);
    let mut at = 8 + uploads.len() * 12 + emitters_len + layers_len;
    let mut payload = Vec::new();
    for (x, y, w, h, bytes) in &uploads {
        for v in [*x, *y, *w, *h] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        blob.extend_from_slice(&((at + payload.len()) as u32).to_le_bytes());
        payload.extend_from_slice(bytes);
        payload.resize(payload.len().next_multiple_of(4), 0);
    }
    for e in emitter_blobs.iter().chain(&layer_blobs) {
        blob.extend_from_slice(e);
    }
    at = blob.len();
    let _ = at;
    blob.extend_from_slice(&payload);
    blob.resize(blob.len().next_multiple_of(4), 0);
    Ok(Extra { blob, report, vram_bytes })
}

// ---------------------------------------------------------------------------------------------
// The reference player: parses a blob and draws a frame of it onto a canvas, as menu.rs does.

fn mix(a: u32, b: u32) -> u32 {
    let h = (a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b)).wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 13)
}

fn lerp9(lut: &[u8], u: i32) -> i32 {
    let i = (u >> 5).min(7) as usize;
    let f = u & 31;
    (i32::from(lut[i]) * (32 - f) + i32::from(lut[i + 1]) * f) >> 5
}

fn rd16(b: &[u8], at: usize) -> i32 {
    i32::from(i16::from_le_bytes([b[at], b[at + 1]]))
}

/// Draw the blob's particles and layers at vblank `now` onto `canvas` (0..1 floats).
pub fn play(blob: &[u8], canvas: &mut [[f64; 3]], now: u32) {
    assert_eq!(&blob[..4], b"HKX1");
    let (n_up, n_em, n_lay) = (blob[4] as usize, blob[5] as usize, blob[6] as usize);
    // The pages as drawn: rebuild texel lookups from the uploads.
    let mut pages: [Vec<u8>; 3] = [vec![0; 65536], vec![0; 65536], vec![0; 65536]];
    let mut palette: Vec<u16> = Vec::new();
    for k in 0..n_up {
        let e = 8 + k * 12;
        let (x, y, w, h) = (rd16(blob, e) as usize, rd16(blob, e + 2) as usize, rd16(blob, e + 4) as usize, rd16(blob, e + 6) as usize);
        let off = u32::from_le_bytes(blob[e + 8..e + 12].try_into().unwrap()) as usize;
        if (x, y) == (PALETTE_AT.0 as usize, PALETTE_AT.1 as usize) {
            palette = (0..w).map(|i| u16::from_le_bytes([blob[off + i * 2], blob[off + i * 2 + 1]])).collect();
            continue;
        }
        let p = PAGES.iter().position(|&(px, _)| px as usize == x).unwrap();
        for row in 0..h {
            for i in 0..w * 2 {
                let byte = blob[off + row * w * 2 + i];
                if PAGES[p].1 == 4 {
                    pages[p][row * 256 + i * 2] = byte & 15;
                    pages[p][row * 256 + i * 2 + 1] = byte >> 4;
                } else {
                    pages[p][row * 256 + i] = byte;
                }
            }
        }
    }
    let mut at = 8 + n_up * 12;
    let texel = |pages: &[Vec<u8>; 3], page: usize, u: i32, v: i32| -> u8 {
        if !(0..256).contains(&u) || !(0..256).contains(&v) {
            return 0;
        }
        pages[page][v as usize * 256 + u as usize]
    };
    let put = |canvas: &mut [[f64; 3]], x: i32, y: i32, rgb: [f64; 3], sub: bool| {
        if !(0..WIDTH as i32).contains(&x) || !(0..HEIGHT as i32).contains(&y) {
            return;
        }
        let d = &mut canvas[y as usize * WIDTH + x as usize];
        for k in 0..3 {
            d[k] = if sub { (d[k] - rgb[k]).max(0.0) } else { (d[k] + rgb[k]).min(1.0) };
        }
    };
    for _ in 0..n_em {
        let head = &blob[at..at + EMITTER_HEAD];
        let (page, u0, v0, w, h, frames, sub, slots) = (head[0] as usize, head[1] as i32, head[2] as i32, head[3] as i32, head[4] as i32, head[5] as i32, head[6] != 0, head[7] as usize);
        let (alpha, size) = (&head[8..17], &head[17..26]);
        let slot_bytes = 1 + VARIANTS * TRACK_BYTES;
        for i in 0..slots {
            let s = at + EMITTER_HEAD + i * slot_bytes;
            let life = i64::from(blob[s]) * 2;
            let t = i64::from(now) + i as i64 * 37;
            let (cycle, age) = (t / life, t % life);
            let track = (mix(i as u32, cycle as u32) & 3) as usize;
            let tb = s + 1 + track * TRACK_BYTES;
            let u = (age * 256 / life) as i32;
            let (x0, y0, bx, by, cx, cy) = (rd16(blob, tb), rd16(blob, tb + 2), rd16(blob, tb + 4), rd16(blob, tb + 6), rd16(blob, tb + 8), rd16(blob, tb + 10));
            let x = (x0 + ((bx * u) >> 8) + ((((cx * u) >> 8) * u) >> 8)) >> 2;
            let y = (y0 + ((by * u) >> 8) + ((((cy * u) >> 8) * u) >> 8)) >> 2;
            let sz = (i32::from(blob[tb + 12]) * lerp9(size, u)) >> 8;
            if sz <= 0 {
                continue;
            }
            let a = lerp9(alpha, u);
            let gain = [0, 1, 2].map(|k| f64::from((i32::from(blob[tb + 13 + k]) * a) >> 8) / 128.0);
            if gain.iter().cloned().fold(0.0, f64::max) < 2.0 / 128.0 {
                continue;
            }
            let frame = ((u * frames) >> 8).min(frames - 1);
            let cols = i32::from(blob[at + 26]).max(1);
            for dy in 0..sz {
                for dx in 0..sz {
                    let tu = u0 + (frame % cols) * w + dx * w / sz;
                    let tv = v0 + (frame / cols) * h + dy * h / sz;
                    let c = f64::from(texel(&pages, page, tu, tv)) / 15.0;
                    if c > 0.0 {
                        put(canvas, x - sz / 2 + dx, y - sz / 2 + dy, gain.map(|g| g * c), sub);
                    }
                }
            }
        }
        at += EMITTER_HEAD + slots * slot_bytes;
    }
    for _ in 0..n_lay {
        let head = &blob[at..at + LAYER_HEAD];
        let (page, blend, u0, v0, w, h, cols, per_page) = (head[0] as usize, head[1], head[2] as i32, head[3] as i32, head[4] as i32, head[5] as i32, head[6] as i32, head[7] as i32);
        let corners: [[f64; 2]; 4] = std::array::from_fn(|k| [f64::from(rd16(head, 8 + k * 4)), f64::from(rd16(head, 10 + k * 4))]);
        let tint = [head[24], head[25], head[26]];
        let samples = u16::from_le_bytes([head[28], head[29]]) as usize;
        let hold = head[30].max(1) as u32;
        let sample = &blob[at + LAYER_HEAD + (((now / hold) as usize) % samples) * 4..][..4];
        let (frame, gain, dx, dy) = (i32::from(sample[0]), i32::from(sample[1]), f64::from(sample[2] as i8), f64::from(sample[3] as i8));
        at += LAYER_HEAD + samples * 4;
        if gain == 0 {
            continue;
        }
        let moved: [[f64; 2]; 4] = corners.map(|c| [c[0] + dx, c[1] + dy]);
        let Some(inv) = inverse_homography(&moved) else { continue };
        let (fp, fi) = if per_page > 0 { (page as i32 + frame / per_page, frame % per_page) } else { (page as i32, frame) };
        let (fu, fv) = (u0 + (fi % cols.max(1)) * w, v0 + (fi / cols.max(1)) * h);
        let xs = moved.iter().map(|c| c[0]);
        let ys = moved.iter().map(|c| c[1]);
        let (x0, x1) = (xs.clone().fold(f64::MAX, f64::min).floor().max(0.0) as i32, xs.fold(f64::MIN, f64::max).ceil().min(WIDTH as f64) as i32);
        let (y0, y1) = (ys.clone().fold(f64::MAX, f64::min).floor().max(0.0) as i32, ys.fold(f64::MIN, f64::max).ceil().min(HEIGHT as f64) as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let ww = inv[6] * px + inv[7] * py + inv[8];
                let (u, v) = ((inv[0] * px + inv[1] * py + inv[2]) / ww, (inv[3] * px + inv[4] * py + inv[5]) / ww);
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let t = texel(&pages, fp as usize, fu + (u * f64::from(w)) as i32, fv + (v * f64::from(h)) as i32);
                if PAGES[fp as usize].1 == 8 {
                    if t == 0 {
                        continue;
                    }
                    let c = palette[t as usize];
                    let rgb = [f64::from(c & 31) / 31.0, f64::from((c >> 5) & 31) / 31.0, f64::from((c >> 10) & 31) / 31.0];
                    canvas[y as usize * WIDTH + x as usize] = rgb;
                } else {
                    let c = f64::from(t) / 15.0;
                    if c > 0.0 {
                        let g = [0, 1, 2].map(|k| c * f64::from(tint[k]) / 128.0 * f64::from(gain) / 128.0);
                        put(canvas, x, y, g, blend == 1);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_packs_shelves_and_refuses_what_does_not_fit() {
        let mut p = Page::new();
        assert_eq!(p.claim(100, 40), Some((0, 0)));
        assert_eq!(p.claim(100, 20), Some((100, 0)));
        assert_eq!(p.claim(100, 10), Some((0, 40)));
        assert_eq!(p.claim(256, 256), None);
        assert_eq!(p.rows, 50);
    }

    #[test]
    fn the_nine_point_lookup_interpolates_between_samples() {
        let lut = [0u8, 32, 64, 96, 128, 160, 192, 224, 255];
        assert_eq!(lerp9(&lut, 0), 0);
        assert_eq!(lerp9(&lut, 16), 16);
        assert_eq!(lerp9(&lut, 255), 253);
    }

    #[test]
    fn rows_are_stored_with_the_first_texel_in_the_low_nibble() {
        let mut p = Page::new();
        let at = p.claim(2, 1).unwrap();
        p.blit(at, 2, 1, &[3, 5]);
        assert_eq!(p.rows_bytes(4)[0], 3 | 5 << 4);
    }
}
