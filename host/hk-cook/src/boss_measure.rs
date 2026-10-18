//! `hk-cook boss-measure`: what a boss's art needs, by fight state, and what each way of fitting it costs (docs/BOSS-ART.md).
//!
//! Reads the tree cooker's manifests (`data/hornet3_art.json`, `data/hornet_corpse3_art.json`), optionally the clip trace of
//! simulated fights (`shared/hk-sim/tests/hornet_clip_trace.rs`), and the Steam sprite libraries. Prints tables; with
//! `--sheet DIR` it also writes contact sheets that put the source, the room quantizer and the shared palettes side by side.
use crate::boss_art::{self as ba, Hist, Library, Reference, Used};
use crate::hornet_scene::{manifest, Ent};
use hk_pil::{Image, Mode};
use hk_unity::Source;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Trace {
    /// Per (tree, ent): (tick, clip id, seed), in file order.
    rows: Vec<(u32, u32, u8, u8, u32)>,
    ticks: u32,
}

fn read_trace(path: &Path) -> Result<Trace> {
    let text = std::fs::read_to_string(path)?;
    let mut rows = vec![];
    let mut ticks = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let (seed, tick, tree, ent, clip) = (f[0].parse()?, f[1].parse()?, f[2].parse()?, f[3].parse()?, f[4].parse()?);
        ticks = ticks.max(tick);
        rows.push((seed, tick, tree, ent, clip));
    }
    Ok(Trace { rows, ticks })
}

fn used_from(trace: &Trace, ents: &[Ent]) -> Used {
    let mut used = Used::new();
    for &(_, _, tree, ent, clip) in &trace.rows {
        if let Some(e) = ents.iter().find(|e| e.tree == tree && e.ent == ent) {
            if let Some(c) = e.clips.iter().find(|c| c.1 == clip) {
                used.entry((tree, ent)).or_default().insert(c.0.clone());
            }
        }
    }
    used
}

fn pct(a: usize, b: usize) -> f64 {
    100.0 * a as f64 / b.max(1) as f64
}

struct Cost {
    parts: usize,
    cells_full: usize,
    trim: usize,
    rle: usize,
    lz4: usize,
}

fn sprite_cost(plane: &[u8], w: usize, h: usize, cell: usize) -> Cost {
    let cells_full = w.div_ceil(cell) * h.div_ceil(cell);
    let ps = ba::parts(plane, w, h, cell);
    Cost {
        parts: ps.len(),
        cells_full,
        trim: ps.iter().map(|p| p.texels.len()).sum(),
        rle: ps.iter().map(|p| ba::packbits(&p.texels).len()).sum(),
        lz4: ps.iter().map(|p| hk_lz4::compress_hc(&p.texels).len()).sum(),
    }
}

fn render(plane: &[u8], words: &[u16], w: usize, h: usize) -> Image {
    let mut im = Image::new(Mode::Rgba, w, h);
    for i in 0..w * h {
        let (bg, p) = ([70.0, 72.0, 84.0], plane[i] as usize);
        let c = if p == 0 || p >= words.len() + 1 {
            bg
        } else {
            let wd = words[p - 1];
            let d = ba::display(wd & 0x7fff);
            if wd & 0x8000 != 0 { [(bg[0] + d[0]).min(255.0), (bg[1] + d[1]).min(255.0), (bg[2] + d[2]).min(255.0)] } else { d }
        };
        im.data[i * 4..i * 4 + 4].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
    }
    im
}

fn source_view(s: &ba::Sprite) -> Image {
    let mut im = Image::new(Mode::Rgba, s.w, s.h);
    for i in 0..s.w * s.h {
        let p = &s.image.data[i * 4..i * 4 + 4];
        let a = p[3] as f64 / 255.0;
        let bg = [70.0, 72.0, 84.0];
        for c in 0..3 {
            im.data[i * 4 + c] = (p[c] as f64 * a + bg[c] * (1.0 - a)) as u8;
        }
        im.data[i * 4 + 3] = 255;
    }
    im
}

pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let arg = |name: &str| args.iter().position(|a| a == name).map(|i| args[i + 1].clone());
    let source = Source::from_doctor(root)?;
    let mut ents = manifest(root, "hornet3", 0)?;
    ents.extend(manifest(root, "hornet_corpse3", 1)?);
    let trace = arg("--trace").map(|p| read_trace(Path::new(&p))).transpose()?;
    let mut used = trace.as_ref().map(|t| used_from(t, &ents));
    if let (Some(path), Some(u)) = (arg("--write-used"), used.as_ref()) {
        let mut text = String::from("# tree\tobject\tclip: what the simulated fights played (shared/hk-sim/tests/hornet_clip_trace.rs), written by `hk-cook boss-measure --trace T --write-used PATH`\n");
        for ((tree, ent), names) in u {
            for n in names {
                text += &format!("{tree}\t{ent}\t{n}\n");
            }
        }
        std::fs::write(path, text)?;
    }
    if args.iter().any(|a| a == "--main-all") {
        // Her own body plays every clip the tree names, whether or not the simulated fights reached it.
        let u = used.get_or_insert_with(Used::new);
        for e in ents.iter().filter(|e| e.ent == 0) {
            u.entry((e.tree, e.ent)).or_default().extend(e.clips.iter().map(|c| c.0.clone()));
        }
    }

    // 1. What the manifest names against what the fight plays.
    let all = ba::load(&source, &ents, None)?;
    let (full_clips, full_sprites) = (all.clips.len(), all.sprites.len());
    println!("manifest (every clip the tree names on any object of the library): {full_clips} clips, {full_sprites} sprites");
    let lib = match &used {
        Some(u) => {
            let l = ba::load(&source, &ents, Some(u))?;
            println!("played in {} simulated fights: {} clips, {} sprites", 4, l.clips.len(), l.sprites.len());
            for ((tree, ent), names) in u {
                println!("  tree {tree} object {ent}: {} clips: {}", names.len(), names.iter().cloned().collect::<Vec<_>>().join(", "));
            }
            l
        }
        None => all,
    };
    let n = lib.sprites.len();
    let px: usize = lib.sprites.iter().map(|s| s.w * s.h).sum();
    println!("sprites {n}, {px} px, widest {} tallest {}", lib.sprites.iter().map(|s| s.w).max().unwrap(), lib.sprites.iter().map(|s| s.h).max().unwrap());

    // 2. Reference quantization and the cost of every representation.
    let refs: Vec<Reference> = lib.sprites.iter().map(ba::reference).collect::<Result<_>>()?;
    let mut tot = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut tot32 = (0usize, 0usize);
    let (mut opaque, mut dup) = (0usize, 0usize);
    let mut seen_parts: BTreeSet<Vec<u8>> = BTreeSet::new();
    for (s, r) in lib.sprites.iter().zip(&refs) {
        let c = sprite_cost(&r.plane, s.w, s.h, 64);
        tot.0 += c.parts;
        tot.1 += c.cells_full;
        tot.2 += c.trim;
        tot.3 += c.rle;
        tot.4 += c.lz4;
        tot.5 += s.w.div_ceil(4) * 2 * s.h;
        let c32 = sprite_cost(&r.plane, s.w, s.h, 32);
        tot32.0 += c32.parts;
        tot32.1 += c32.trim;
        opaque += r.plane.iter().filter(|&&p| p != 0).count();
        for p in ba::parts(&r.plane, s.w, s.h, 64) {
            let mut key = r.words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
            key.extend(&p.texels);
            if !seen_parts.insert(key) {
                dup += 1;
            }
        }
    }
    println!("\nrepresentations (reference palettes):");
    println!("  whole frames as 4bpp, rows padded to words      {:>9} B", tot.5);
    println!("  64x64 cells, empty ones kept ({} cells)          {:>9} B (rows of the full cell width: not counted)", tot.1, tot.1 * 2048);
    println!("  64x64 cells trimmed to the opaque box ({} parts) {:>9} B  ({:.0}% of whole frames)", tot.0, tot.2, pct(tot.2, tot.5));
    println!("  ... packbits (ability_art's RLE)                 {:>9} B  ({:.0}%)", tot.3, pct(tot.3, tot.5));
    println!("  ... LZ4 per part                                 {:>9} B  ({:.0}%)", tot.4, pct(tot.4, tot.5));
    println!("  32x32 cells trimmed ({} parts)                   {:>9} B  ({:.0}%)", tot32.0, tot32.1, pct(tot32.1, tot.5));
    println!("  opaque texels {opaque} of {px} px ({:.0}%); {dup} parts repeat another's bytes and palette exactly", pct(opaque, px));

    // 3. Palettes: how many the reference quantizer makes, and the merge curve.
    let mut distinct: BTreeSet<Vec<u16>> = BTreeSet::new();
    for r in &refs {
        distinct.insert(r.words.clone());
    }
    println!("\npalettes: {} sprites, {} distinct 16-word palettes", n, distinct.len());
    let (mut e0, mut w0, mut p0, mut g32_0) = (0.0, 0.0f64, 0usize, 0usize);
    for (s, r) in lib.sprites.iter().zip(&refs) {
        let (e, w) = ba::reference_error(s, r);
        e0 += e;
        w0 = w0.max(w);
        let k = r.plane.iter().filter(|&&p| p != 0).count();
        p0 += k;
        for i in 0..r.plane.len() {
            if let Some((c, _)) = ba::source_colour(s, r, i) {
                let d = ba::display(r.words[r.plane[i] as usize] & 0x7fff);
                if (((c[0] - d[0]).powi(2) + (c[1] - d[1]).powi(2) + (c[2] - d[2]).powi(2)).sqrt()) > 32.0 {
                    g32_0 += 1;
                }
            }
        }
    }
    let rms = |e: f64, px: usize| (e / (3.0 * px as f64)).sqrt();
    println!("  baseline (one palette per sprite): per-channel RMS error {:.2}, worst texel {:.0}, {:.2}% of texels off by more than 32", rms(e0, p0), w0.sqrt(), 100.0 * g32_0 as f64 / p0 as f64);
    let hists: Vec<Hist> = refs.iter().map(ba::histogram).collect();
    let targets: Vec<usize> = [256, 128, 96, 64, 48, 32, 24, 16, 12, 8, 6, 4, 3, 2, 1].into_iter().filter(|&t| t < n).collect();
    let t0 = std::time::Instant::now();
    let (_, snaps) = ba::merge(&hists, 1, f64::INFINITY, &targets, &mut |_, _, _| {});
    eprintln!("merge took {:?}", t0.elapsed());
    println!("  shared palettes (greedy merge of the pair that adds least error):");
    println!("    groups | added over the room quantizer: RMS worst >32 | against the source: RMS worst >32   (RMS per colour channel, 0..255)");
    let mut sheet_sets: Vec<(usize, Vec<Vec<u8>>, Vec<Vec<u16>>)> = vec![];
    for (live, m) in &snaps {
        let mut line = format!("    {:>5} |", live);
        let mut planes: Vec<Vec<u8>> = vec![vec![]; n];
        let mut ordered: Vec<Vec<u16>> = vec![vec![]; n];
        for vs_ref in [true, false] {
            let (mut e, mut w, mut cnt, mut g32) = (0.0, 0.0f64, 0usize, 0usize);
            for (gi, members) in m.members.iter().enumerate() {
                for &si in members {
                    let (plane, err, worst, ord) = ba::recolour(&lib.sprites[si], &refs[si], &m.words[gi], vs_ref);
                    e += err;
                    w = w.max(worst);
                    cnt += plane.iter().filter(|&&p| p != 0).count();
                    for i in 0..plane.len() {
                        if plane[i] != 0 {
                            let c = if vs_ref {
                                ba::display(refs[si].words[refs[si].plane[i] as usize] & 0x7fff)
                            } else {
                                ba::source_colour(&lib.sprites[si], &refs[si], i).unwrap().0
                            };
                            let d = ba::display(ord[plane[i] as usize - 1] & 0x7fff);
                            if ((c[0] - d[0]).powi(2) + (c[1] - d[1]).powi(2) + (c[2] - d[2]).powi(2)).sqrt() > 32.0 {
                                g32 += 1;
                            }
                        }
                    }
                    if vs_ref {
                        planes[si] = plane;
                        ordered[si] = ord;
                    }
                }
            }
            line += &format!(" {:>5.2} {:>4.0} {:>6.2}% |", rms(e, cnt), w.sqrt(), 100.0 * g32 as f64 / cnt as f64);
        }
        println!("{line}");
        sheet_sets.push((*live, planes, ordered));
    }

    // 4. Dual-frame CLUT packing: two frames in one 4bpp texture leave each three colours and a transparent index.
    let (mut e3, mut c3, mut g3) = (0.0, 0usize, 0usize);
    let mut ok = 0usize;
    for (s, r) in lib.sprites.iter().zip(&refs) {
        let (words, _) = ba::best_palette(&ba::histogram(r), 3);
        let (plane, e, _, ord) = ba::recolour(s, r, &words, false);
        let cnt = plane.iter().filter(|&&p| p != 0).count();
        let (be, _) = ba::reference_error(s, r);
        e3 += e;
        c3 += cnt;
        let mut bad = 0;
        for i in 0..plane.len() {
            if plane[i] != 0 {
                let (c, _) = ba::source_colour(s, r, i).unwrap();
                let d = ba::display(ord[plane[i] as usize - 1] & 0x7fff);
                if ((c[0] - d[0]).powi(2) + (c[1] - d[1]).powi(2) + (c[2] - d[2]).powi(2)).sqrt() > 32.0 {
                    bad += 1;
                }
            }
        }
        g3 += bad;
        if cnt > 0 && rms(e, cnt) <= rms(be, cnt) + 3.0 {
            ok += 1;
        }
    }
    println!("\ndual-frame CLUT packing (3 opaque colours per frame): RMS {:.2}, {:.2}% of texels off by more than 32; {} of {n} sprites stay within 3 RMS of their 15-colour version", rms(e3, c3), 100.0 * g3 as f64 / c3 as f64, ok);

    // 5. Working sets in time.
    if let Some(t) = &trace {
        let sizes = per_sprite_bytes(&lib, &refs);
        println!("\nworking set of sprites on screen in a window (simulated fights, {} ticks):", t.ticks);
        let mut by_ent: BTreeMap<(u32, u8, u8), Vec<(u32, u32)>> = BTreeMap::new();
        for &(seed, tick, tree, ent, clip) in &t.rows {
            by_ent.entry((seed, tree, ent)).or_default().push((tick, clip));
        }
        // clip -> sprites
        let mut sprites_of: BTreeMap<(u8, u8, u32), Vec<usize>> = BTreeMap::new();
        for e in &ents {
            for (name, id) in &e.clips {
                if let Some(c) = lib.clips.iter().find(|c| c.group == crate::hornet_scene::group_of(e) && &c.name == name) {
                    sprites_of.insert((e.tree, e.ent, *id), c.frames.clone());
                }
            }
        }
        for window in [60u32, 180, 600] {
            let (mut max_b, mut max_s, mut sum_b, mut cnt) = (0usize, 0usize, 0usize, 0usize);
            for seed in [3u32, 11, 29, 77] {
                let mut start = 0;
                while start < 60 * 400 {
                    let mut set: BTreeSet<usize> = BTreeSet::new();
                    for ((s, tree, ent), rows) in &by_ent {
                        if *s != seed || *tree != 0 {
                            continue;
                        }
                        for (k, &(tick, clip)) in rows.iter().enumerate() {
                            let end = rows.get(k + 1).map_or(tick + 360, |r| r.0.min(tick + 360));
                            if tick < start + window && end > start {
                                if let Some(sp) = sprites_of.get(&(*tree, *ent, clip)) {
                                    set.extend(sp.iter().copied());
                                }
                            }
                        }
                    }
                    if !set.is_empty() {
                        let b: usize = set.iter().map(|&i| sizes[i].0).sum();
                        max_b = max_b.max(b);
                        max_s = max_s.max(set.len());
                        sum_b += b;
                        cnt += 1;
                    }
                    start += window;
                }
            }
            println!("  {:>4.0} s window: mean {:>7} B trimmed, worst {:>7} B and {} sprites", window as f64 / 60.0, sum_b / cnt.max(1), max_b, max_s);
        }
        // Per clip.
        println!("\nper clip (frames, unique sprites, trimmed bytes, packbits bytes):");
        for c in &lib.clips {
            let set: BTreeSet<usize> = c.frames.iter().copied().collect();
            let b: usize = set.iter().map(|&i| sizes[i].0).sum();
            let r: usize = set.iter().map(|&i| sizes[i].1).sum();
            println!("  {:<24} {:<22} {:>3} fr {:>3} spr {:>7} B {:>7} B", c.name, c.library, c.frames.len(), set.len(), b, r);
        }
    }

    // 6. Contact sheets.
    if let Some(dir) = arg("--sheet") {
        std::fs::create_dir_all(&dir)?;
        let pick: Vec<usize> = {
            let mut v = vec![];
            for c in lib.clips.iter().take(40) {
                if let Some(&i) = c.frames.get(c.frames.len() / 2) {
                    if !v.contains(&i) && lib.sprites[i].w <= 200 {
                        v.push(i);
                    }
                }
                if v.len() >= 12 {
                    break;
                }
            }
            v
        };
        let cols = 1 + sheet_sets.len().min(4);
        let cell = (pick.iter().map(|&i| lib.sprites[i].w).max().unwrap_or(64) + 4, pick.iter().map(|&i| lib.sprites[i].h).max().unwrap_or(64) + 4);
        let mut sheet = Image::new(Mode::Rgba, cell.0 * (cols + 1), cell.1 * pick.len());
        let chosen: Vec<&(usize, Vec<Vec<u8>>, Vec<Vec<u16>>)> = [64usize, 32, 16, 8].iter().filter_map(|k| sheet_sets.iter().find(|s| s.0 == *k)).collect();
        for (row, &si) in pick.iter().enumerate() {
            let s = &lib.sprites[si];
            let y = (row * cell.1) as i64;
            sheet.paste(&source_view(s), 0, y, None);
            let ref_words: Vec<u16> = refs[si].words[1..].to_vec();
            sheet.paste(&render(&refs[si].plane, &ref_words, s.w, s.h), cell.0 as i64, y, None);
            for (col, set) in chosen.iter().enumerate() {
                sheet.paste(&render(&set.1[si], &set.2[si], s.w, s.h), (cell.0 * (col + 2)) as i64, y, None);
            }
        }
        crate::png::save_rgba(&Path::new(&dir).join("palette-sheet.png"), &sheet)?;
        println!("\nsheet: {dir}/palette-sheet.png (columns: source, room quantizer, then {:?} shared palettes)", chosen.iter().map(|c| c.0).collect::<Vec<_>>());
    }
    Ok(())
}

/// Per sprite: trimmed and packbits bytes.
fn per_sprite_bytes(lib: &Library, refs: &[Reference]) -> Vec<(usize, usize)> {
    lib.sprites
        .iter()
        .zip(refs)
        .map(|(s, r)| {
            let ps = ba::parts(&r.plane, s.w, s.h, 64);
            (ps.iter().map(|p| p.texels.len()).sum(), ps.iter().map(|p| ba::packbits(&p.texels).len()).sum())
        })
        .collect()
}

/// Per-library totals of a boss scene: every clip of every sprite library its animators use, at the scale they stand in the
/// scene (an upper bound on what a fight can show: `hk-cook boss-survey Ruins1_24_boss Fungus2_15_boss ...`).
pub fn survey(root: &Path, scenes: &[String]) -> Result<()> {
    use hk_unity::scene::{build_settings, Scene};
    use hk_unity::Value;
    let source = Source::from_doctor(root)?;
    for name in scenes {
        let level = build_settings(&source)?.get(name).ok_or("scene is not in BuildSettings")?.clone();
        let file = source.file(&level)?;
        let scene = Scene::open(&source, &level, false)?;
        let num = |v: Option<&Value>| v.and_then(Value::float).unwrap_or(0.0);
        let mut sprite_scale: BTreeMap<i64, [f64; 2]> = BTreeMap::new();
        for info in file.objects.iter().filter(|i| i.class_id == 114) {
            let o = source.object(&file, info.path_id)?;
            if source.typename(&o).map_or(true, |t| t != "tk2dSprite") {
                continue;
            }
            let t = source.read(&o)?;
            if let Some(go) = t.get("m_GameObject").and_then(|p| p.pptr()).map(|p| p.1) {
                let s = t.get("_scale");
                sprite_scale.insert(go, [num(s.and_then(|v| v.get("x"))), num(s.and_then(|v| v.get("y")))]);
            }
        }
        let mut ents: Vec<Ent> = vec![];
        let mut label: Vec<String> = vec![];
        for info in file.objects.iter().filter(|i| i.class_id == 114) {
            let o = source.object(&file, info.path_id)?;
            if source.typename(&o).map_or(true, |t| t != "tk2dSpriteAnimator") {
                continue;
            }
            let tree = source.read(&o)?;
            let Some(go) = tree.get("m_GameObject").and_then(|p| p.pptr()).map(|p| p.1) else { continue };
            let Some(lib) = tree.get("library") else { continue };
            if lib.pptr().is_none_or(|p| p.1 == 0) {
                continue;
            }
            let lib_o = source.deref(&file, lib)?;
            let library = source.read(&lib_o)?;
            let clips: Vec<(String, u32)> = library
                .get("clips")
                .and_then(Value::list)
                .unwrap_or(&[])
                .iter()
                .enumerate()
                .map(|(i, c)| (c.get("name").and_then(Value::str).unwrap_or_default().to_string(), i as u32))
                .collect();
            let world = scene.go_transform.get(&go).and_then(|t| scene.world(*t).ok()).map_or([1.0, 1.0], |m| {
                [(m[0][0].powi(2) + m[1][0].powi(2)).sqrt(), (m[0][1].powi(2) + m[1][1].powi(2)).sqrt()]
            });
            let go_name = scene.go(go).and_then(|g| g.get("m_Name")).and_then(Value::str).unwrap_or_default().to_string();
            ents.push(Ent {
                tree: 0,
                ent: ents.len().min(255) as u8,
                library: lib_o.sid(),
                sprite_scale: sprite_scale.get(&go).copied().unwrap_or([1.0, 1.0]),
                world_scale: world,
                alpha: 1.0,
                clips,
            });
            label.push(go_name);
        }
        // Libraries that big scenes carry for scenery are not the boss's: print each so the reader can judge.
        let mut by_lib: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in ents.iter().enumerate() {
            by_lib.entry(e.library.clone()).or_default().push(i);
        }
        println!("\n== {name} ({level}): {} animators, {} libraries", ents.len(), by_lib.len());
        let mut total = (0usize, 0usize, 0usize, 0usize, 0usize);
        for (library, users) in &by_lib {
            let sub: Vec<Ent> = users
                .iter()
                .map(|&i| Ent { tree: 0, ent: i as u8, library: ents[i].library.clone(), sprite_scale: ents[i].sprite_scale, world_scale: ents[i].world_scale, alpha: 1.0, clips: ents[i].clips.clone() })
                .collect();
            let lib = match ba::load(&source, &sub, None) {
                Ok(l) => l,
                Err(e) => {
                    println!("  {library}: refused ({e})");
                    continue;
                }
            };
            let (mut parts, mut trim, mut rle, mut px) = (0, 0, 0, 0);
            for s in &lib.sprites {
                let r = ba::reference(s)?;
                for p in ba::parts(&r.plane, s.w, s.h, 64) {
                    parts += 1;
                    trim += p.texels.len();
                    rle += ba::packbits(&p.texels).len().min(p.texels.len());
                }
                px += s.w * s.h;
            }
            let names: Vec<&str> = users.iter().take(3).map(|&i| label[i].as_str()).collect();
            println!("  {library:<34} {:>3} clips {:>4} sprites {:>8} px {:>4} parts {:>8} B trimmed {:>8} B coded  (used by {} objects: {})", lib.clips.len(), lib.sprites.len(), px, parts, trim, rle, users.len(), names.join(", "));
            total.0 += lib.sprites.len();
            total.1 += px;
            total.2 += parts;
            total.3 += trim;
            total.4 += rle;
        }
        println!("  total: {} sprites, {} px, {} parts, {} B trimmed, {} B coded", total.0, total.1, total.2, total.3, total.4);
    }
    Ok(())
}
