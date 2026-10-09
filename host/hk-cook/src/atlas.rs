//! The room atlas: canonical 4bpp textures, their page packing and the animation/alpha
//! stream. Ported from `Atlas`, `canonical_black` and the module constants of host/cook.py.
//!
//! Colour quantization is a caller-supplied function (`Quantizer`), because Python chose it
//! per call (`host/quantize.py`, with the octree fallback for pure-black art).

use crate::alpha_cover::{compute_record, HAS_ALPHA_COVERS, RECORD_BYTES};
use crate::common::{err, Result};
use crate::packer::{aligned, dense_pack};
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use std::collections::{BTreeSet, HashMap};

/// `cook.SLOT_PIXELS`, `ANIMATION_SLOTS`, `RESERVED_SLOTS`, `MAX_FRAME_TILES`, `MAX_TEXTURE_AXIS`.
pub const SLOT_PIXELS: usize = 64;
pub const MAX_FRAME_TILES: usize = 24 - 4;
pub const MAX_TEXTURE_AXIS: usize = 252;
/// HKROOM02's own texture-table limit.
pub const MAX_ROOM_TEXTURES: usize = 640;
/// `quantize.EDGE_REACH`.
pub const EDGE_REACH: usize = 2;

/// What a quantizer returns: the resized image, the 32-byte palette and a `w * h` index plane.
pub struct Quantized {
    pub image: Image,
    pub palette: Vec<u8>,
    pub plane: Vec<u8>,
}

pub type Quantizer<'a> = &'a dyn Fn(&Image, usize, usize) -> Result<Quantized>;

/// A placed texture: (page, x, y, w, h, clut, stream offset). `page` is 65535 for a streamed texture.
pub type Entry = [i64; 7];

const BINARY_BLACK_WORDS: [u16; 2] = [0x0001, 0x8000];

/// `canonical_black(palette, pixels, w, h)`: a binary black palette with its opaque sentinel at index 1.
pub fn canonical_black(palette: &[u8], pixels: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut words: Vec<u16> = palette
        .chunks(2)
        .take(16)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    if words.len() != 16
        || words[0] != 0
        || !words[1..].iter().all(|w| BINARY_BLACK_WORDS.contains(w))
        || !words[1..].contains(&1)
        || words[1] == 1
    {
        return (palette.to_vec(), pixels.to_vec());
    }
    let k = 1 + words[1..].iter().position(|&w| w == 1).unwrap();
    words.swap(1, k);
    let swap = |v: u8| -> u8 {
        if v == 1 {
            k as u8
        } else if v as usize == k {
            1
        } else {
            v
        }
    };
    let out = pixels
        .iter()
        .map(|&b| swap(b & 15) | (swap(b >> 4) << 4))
        .collect();
    (words.iter().flat_map(|w| w.to_le_bytes()).collect(), out)
}

type CanonicalKey = (bool, usize, usize, Vec<u8>, Vec<u8>);

pub struct Atlas {
    pub flags: u32,
    pub animation_bytes: usize,
    pub alpha_cover_bytes: usize,
    pub max_textures: usize,
    pub max_cluts: usize,
    pub cluts: usize,
    pub max_pages: usize,
    pub images: Vec<Option<Image>>,
    pub entries: Vec<Option<Entry>>,
    pub pages: Vec<Vec<u8>>,
    pub palettes: Vec<Vec<u8>>,
    pub streamed: BTreeSet<usize>,
    pub stream: Vec<u8>,
    /// (w, h, palette, pixels) per texture.
    pub quantized: Vec<(usize, usize, Vec<u8>, Vec<u8>)>,
    canonical: HashMap<CanonicalKey, usize>,
    pub request_map: Vec<usize>,
    deduplicate: bool,
    /// First tile of a multi-slot frame to its (columns, rows).
    pub grids: HashMap<usize, (usize, usize)>,
    pub tile_owner: HashMap<usize, usize>,
}

fn clamp_axis(v: f64) -> usize {
    (v.ceil().min(MAX_TEXTURE_AXIS as f64) as usize).max(1)
}

impl Atlas {
    pub fn new(
        deduplicate: bool,
        max_pages: usize,
        max_textures: usize,
        alpha_covers: bool,
        max_cluts: Option<usize>,
    ) -> Atlas {
        Atlas {
            flags: if alpha_covers { HAS_ALPHA_COVERS } else { 0 },
            animation_bytes: 0,
            alpha_cover_bytes: 0,
            max_textures,
            // A CLUT slot is a distinct palette, not a texture.
            max_cluts: max_cluts.unwrap_or(max_textures),
            cluts: 0,
            max_pages,
            images: Vec::new(),
            entries: Vec::new(),
            pages: Vec::new(),
            palettes: Vec::new(),
            streamed: BTreeSet::new(),
            stream: Vec::new(),
            quantized: Vec::new(),
            canonical: HashMap::new(),
            request_map: Vec::new(),
            deduplicate,
            grids: HashMap::new(),
            tile_owner: HashMap::new(),
        }
    }

    /// `Atlas()` with the defaults.
    pub fn standard() -> Atlas {
        Atlas::new(true, 20, MAX_ROOM_TEXTURES, true, None)
    }

    /// `add_quantized`: admit exact final texels; different palettes/storage classes never alias.
    pub fn add_quantized(
        &mut self,
        w: usize,
        h: usize,
        palette: &[u8],
        pixels: &[u8],
        streamed: bool,
        unique: bool,
    ) -> Result<usize> {
        if !(1..=256).contains(&w)
            || !(1..=256).contains(&h)
            || palette.len() != 32
            || pixels.len() != w.div_ceil(2) * h
        {
            return err("invalid canonical texture dimensions/palette/texels");
        }
        if streamed && (w > SLOT_PIXELS || h > SLOT_PIXELS) {
            return err(format!(
                "animation cache dimensions exceed {SLOT_PIXELS}x{SLOT_PIXELS}: {w}x{h}"
            ));
        }
        let (palette, pixels) = canonical_black(palette, pixels);
        let key: CanonicalKey = (streamed, w, h, palette.clone(), pixels.clone());
        let found = if self.deduplicate && !unique {
            self.canonical.get(&key).copied()
        } else {
            None
        };
        let index = match found {
            Some(i) => i,
            None => {
                let index = self.images.len();
                self.images.push(None);
                self.quantized.push((w, h, palette, pixels));
                if streamed {
                    self.streamed.insert(index);
                }
                if !unique {
                    self.canonical.insert(key, index);
                }
                index
            }
        };
        self.request_map.push(index);
        Ok(index)
    }

    /// `_pack_plane`: a rectangle of an index plane as packed 4bpp rows.
    pub fn pack_plane(
        plane: &[u8],
        plane_width: usize,
        x0: usize,
        y0: usize,
        w: usize,
        h: usize,
    ) -> Vec<u8> {
        let stride = w.div_ceil(2);
        let mut pixels = vec![0u8; stride * h];
        for yy in 0..h {
            let row = (y0 + yy) * plane_width + x0;
            for xx in 0..w {
                pixels[yy * stride + xx / 2] |= plane[row + xx] << ((xx & 1) * 4);
            }
        }
        pixels
    }

    /// `add`: quantize one frame to the size it is drawn at and admit it.
    pub fn add(
        &mut self,
        im: &Image,
        w: f64,
        h: f64,
        streamed: bool,
        quantize: Quantizer,
    ) -> Result<usize> {
        let (w, h) = (clamp_axis(w), clamp_axis(h));
        if streamed && (w > SLOT_PIXELS || h > SLOT_PIXELS) {
            return err(format!(
                "animation cache dimensions exceed {SLOT_PIXELS}x{SLOT_PIXELS}: {w}x{h}"
            ));
        }
        let q = quantize(im, w, h)?;
        let index = self.add_quantized(
            w,
            h,
            &q.palette,
            &Atlas::pack_plane(&q.plane, w, 0, 0, w, h),
            streamed,
            false,
        )?;
        self.images[index] = Some(q.image);
        Ok(index)
    }

    /// `add_tiled`: one streamed frame across a rectangle of 64x64 animation slots.
    pub fn add_tiled(&mut self, im: &Image, w: f64, h: f64, quantize: Quantizer) -> Result<usize> {
        let (w, h) = (clamp_axis(w), clamp_axis(h));
        let (cols, rows) = (w.div_ceil(SLOT_PIXELS), h.div_ceil(SLOT_PIXELS));
        if cols * rows > MAX_FRAME_TILES {
            return err(format!(
                "frame {w}x{h} binds {} of the {MAX_FRAME_TILES} animation slots a frame may hold",
                cols * rows
            ));
        }
        if cols * rows == 1 {
            return self.add(im, w as f64, h as f64, true, quantize);
        }
        let q = quantize(im, w, h)?;
        let mut base: Option<usize> = None;
        for row in 0..rows {
            for col in 0..cols {
                let (x0, y0) = (col * SLOT_PIXELS, row * SLOT_PIXELS);
                let (tw, th) = (SLOT_PIXELS.min(w - x0), SLOT_PIXELS.min(h - y0));
                let index = self.add_quantized(
                    tw,
                    th,
                    &q.palette,
                    &Atlas::pack_plane(&q.plane, w, x0, y0, tw, th),
                    true,
                    true,
                )?;
                match base {
                    None => {
                        base = Some(index);
                        self.grids.insert(index, (cols, rows));
                    }
                    Some(b) if index != b + row * cols + col => {
                        return err("a frame's tiles are no longer consecutive texture IDs")
                    }
                    _ => {}
                }
                self.tile_owner.insert(index, base.unwrap());
                self.images[index] = Some(q.image.crop_int(
                    x0 as i64,
                    y0 as i64,
                    (x0 + tw) as i64,
                    (y0 + th) as i64,
                ));
            }
        }
        Ok(base.unwrap())
    }

    /// `add_frames_shared`: several streamed frames on one shared palette, the first texture of each.
    pub fn add_frames_shared(
        &mut self,
        items: &[(Image, f64, f64)],
        quantize: Quantizer,
    ) -> Result<Vec<usize>> {
        let gap = EDGE_REACH * 2 + 2;
        struct Sized {
            im: Image,
            w: usize,
            h: usize,
            cols: usize,
            rows: usize,
        }
        let mut sized = Vec::new();
        for (im, w, h) in items {
            let (w, h) = (clamp_axis(*w), clamp_axis(*h));
            let (cols, rows) = (w.div_ceil(SLOT_PIXELS), h.div_ceil(SLOT_PIXELS));
            if cols * rows > MAX_FRAME_TILES {
                return err(format!("frame {w}x{h} binds {} of the {MAX_FRAME_TILES} animation slots a frame may hold", cols * rows));
            }
            sized.push(Sized {
                im: im.to_rgba().resize(w, h, Filter::Lanczos),
                w,
                h,
                cols,
                rows,
            });
        }
        let width: usize = sized.iter().map(|s| s.w).sum::<usize>() + gap * (sized.len() - 1);
        let height = sized.iter().map(|s| s.h).max().unwrap_or(0);
        let mut sheet = Image::new(Mode::Rgba, width, height);
        let (mut x, mut origins) = (0usize, Vec::new());
        for s in &sized {
            sheet.paste(&s.im, x as i64, 0, None);
            origins.push(x);
            x += s.w + gap;
        }
        let q = quantize(&sheet, width, height)?;
        let mut firsts = Vec::new();
        for (s, x0) in sized.iter().zip(origins) {
            let mut sub = Vec::with_capacity(s.w * s.h);
            for y in 0..s.h {
                for xx in 0..s.w {
                    sub.push(q.plane[y * width + x0 + xx]);
                }
            }
            if s.cols * s.rows == 1 {
                let index = self.add_quantized(
                    s.w,
                    s.h,
                    &q.palette,
                    &Atlas::pack_plane(&sub, s.w, 0, 0, s.w, s.h),
                    true,
                    false,
                )?;
                self.images[index] = Some(s.im.clone());
                firsts.push(index);
                continue;
            }
            let mut base: Option<usize> = None;
            for row in 0..s.rows {
                for col in 0..s.cols {
                    let (tx, ty) = (col * SLOT_PIXELS, row * SLOT_PIXELS);
                    let (tw, th) = (SLOT_PIXELS.min(s.w - tx), SLOT_PIXELS.min(s.h - ty));
                    let index = self.add_quantized(
                        tw,
                        th,
                        &q.palette,
                        &Atlas::pack_plane(&sub, s.w, tx, ty, tw, th),
                        true,
                        true,
                    )?;
                    match base {
                        None => {
                            base = Some(index);
                            self.grids.insert(index, (s.cols, s.rows));
                        }
                        Some(b) if index != b + row * s.cols + col => {
                            return err("a frame's tiles are no longer consecutive texture IDs")
                        }
                        _ => {}
                    }
                    self.tile_owner.insert(index, base.unwrap());
                    self.images[index] = Some(s.im.crop_int(
                        tx as i64,
                        ty as i64,
                        (tx + tw) as i64,
                        (ty + th) as i64,
                    ));
                }
            }
            firsts.push(base.unwrap());
        }
        Ok(firsts)
    }

    /// `pack`: lay the static textures into pages and the animation textures into the stream.
    pub fn pack(&mut self) -> Result<()> {
        let n = self.quantized.len();
        self.entries = vec![None; n];
        self.palettes = Vec::new();
        self.stream = Vec::new();
        let mut distinct: BTreeSet<Vec<u8>> = BTreeSet::new();
        let rects: Vec<(i64, i64, usize)> = self
            .quantized
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.streamed.contains(i))
            .map(|(i, q)| (aligned(q.0 as i64), q.1 as i64, i))
            .collect();
        let (page_count, placements) = dense_pack(&rects)?;
        if page_count > self.max_pages {
            return err("4bpp VRAM page budget exceeded");
        }
        self.pages = vec![vec![0u8; 32768]; page_count];
        let positions: HashMap<usize, (usize, i64, i64)> = placements
            .iter()
            .map(|&(i, page, x, y, _, _)| (i, (page, x, y)))
            .collect();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| {
            (
                std::cmp::Reverse(self.quantized[i].1),
                std::cmp::Reverse(self.quantized[i].0),
                i,
            )
        });
        for i in order {
            let (w, h, palette, pixels) = self.quantized[i].clone();
            let streamed = self.streamed.contains(&i);
            let (mut stream_offset, mut stride) = (0usize, 0usize);
            let (p, x, y): (i64, i64, i64);
            if streamed {
                // Original UV dimensions survive word-aligned transfer padding.
                let pad = (4 - self.stream.len() % 4) % 4;
                self.stream.extend(std::iter::repeat_n(0u8, pad));
                stream_offset = self.stream.len();
                stride = ((w + 3) & !3) / 2;
                self.stream.extend(std::iter::repeat_n(0u8, stride * h));
                // A streamed texture's VRAM origin is its animation slot, so these two fields carry the frame's tile grid.
                let (cols, rows) = self.grids.get(&i).copied().unwrap_or((0, 0));
                (p, x, y) = (65535, cols as i64, rows as i64);
            } else {
                let pos = positions[&i];
                (p, x, y) = (pos.0 as i64, pos.1, pos.2);
            }
            if let Some(&owner) = self.tile_owner.get(&i) {
                if self.quantized[owner].2 != palette {
                    return err("tiles of one frame disagree on their palette");
                }
            }
            self.palettes.push(palette.clone());
            let cl = self.palettes.len() - 1;
            distinct.insert(palette);
            if distinct.len() > self.max_cluts {
                return err("CLUT budget exceeded");
            }
            if self.palettes.len() > self.max_textures {
                return err("HKROOM02 texture table exceeded");
            }
            for yy in 0..h {
                for xx in 0..w {
                    let v = (pixels[yy * w.div_ceil(2) + xx / 2] >> ((xx & 1) * 4)) & 15;
                    let (k, shift, target): (usize, usize, &mut Vec<u8>) = if streamed {
                        (
                            stream_offset + yy * stride + xx / 2,
                            (xx & 1) * 4,
                            &mut self.stream,
                        )
                    } else {
                        (
                            (((y as usize + yy) * 256 + x as usize + xx) / 2),
                            ((x as usize + xx) & 1) * 4,
                            &mut self.pages[p as usize],
                        )
                    };
                    target[k] = (target[k] & !(15 << shift)) | (v << shift);
                }
            }
            self.entries[i] = Some([p, x, y, w as i64, h as i64, cl as i64, stream_offset as i64]);
        }
        let pad = (4 - self.stream.len() % 4) % 4;
        self.stream.extend(std::iter::repeat_n(0u8, pad));
        self.cluts = distinct.len();
        self.animation_bytes = self.stream.len();
        self.alpha_cover_bytes = 0;
        if self.flags & HAS_ALPHA_COVERS != 0 {
            for i in 0..n {
                if self.streamed.contains(&i) {
                    continue;
                }
                let (w, h, palette, pixels) = self.quantized[i].clone();
                let mut e = self.entries[i].unwrap();
                e[6] = self.stream.len() as i64;
                self.entries[i] = Some(e);
                self.stream.extend(compute_record(w, h, &palette, &pixels)?);
                self.alpha_cover_bytes += RECORD_BYTES;
            }
        }
        if self.stream.len() > 256 * 1024 {
            return err("animation and alpha metadata RAM bank exceeds 256 KiB");
        }
        Ok(())
    }
}
