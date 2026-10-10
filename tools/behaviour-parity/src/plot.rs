//! Pictures for review: the terrain of a scene with the original's path and the port's path over it.
use super::*;
use crate::compare::{match_actors, run_port_scene};
use crate::og_trace::load_run;
use crate::world_data::load_scene;
use std::collections::BTreeMap;

struct Canvas {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}
impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            rgb: vec![255; w * h * 3],
        }
    }
    fn dot(&mut self, x: i64, y: i64, r: i64, c: [u8; 3]) {
        for dy in -r..=r {
            for dx in -r..=r {
                let (px, py) = (x + dx, y + dy);
                if px >= 0 && py >= 0 && (px as usize) < self.w && (py as usize) < self.h {
                    let i = (py as usize * self.w + px as usize) * 3;
                    self.rgb[i..i + 3].copy_from_slice(&c);
                }
            }
        }
    }
    fn line(&mut self, a: (i64, i64), b: (i64, i64), c: [u8; 3]) {
        let n = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
        for i in 0..=n {
            self.dot(a.0 + (b.0 - a.0) * i / n, a.1 + (b.1 - a.1) * i / n, 0, c);
        }
    }
    fn save(&self, path: &std::path::Path) {
        let file = std::fs::File::create(path).expect("create png");
        let mut enc =
            png::Encoder::new(std::io::BufWriter::new(file), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&self.rgb)
            .unwrap();
    }
}

/// `plot RUN SCENE SOURCE_ID FROM TO OUT.png`: ticks FROM..TO of one pair over its terrain; the
/// original is drawn in blue, the port in red, the start of each with a larger dot.
pub fn run(run_dir: &std::path::Path, names: &BTreeMap<usize, String>, args: &[String]) {
    let (scene_name, source_id) = (&args[0], args[1].parse::<u32>().unwrap());
    let (from, to): (usize, usize) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let out = std::path::Path::new(&args[4]);
    let traces = load_run(run_dir);
    let trace = &traces[scene_name];
    let id = *names.iter().find(|(_, n)| *n == scene_name).unwrap().0;
    let ticks = (trace.last_frame - trace.origin).max(0) as usize;
    let port = run_port_scene(id, trace, ticks);
    let (pairs, _, _) = match_actors(scene_name, trace, &port);
    let pair = pairs
        .iter()
        .find(|p| p.port.source_id == source_id)
        .expect("pair");
    let to = to.min(pair.port.ticks.len());
    let og: Vec<(f64, f64)> = (from..to)
        .filter_map(|t| {
            pair.og
                .samples
                .iter()
                .find(|s| s.frame == trace.origin + t as i64)
                .map(|s| (s.x, s.y))
        })
        .collect();
    let pt: Vec<(f64, f64)> = pair.port.ticks[from..to]
        .iter()
        .map(|t| (t.x, t.y))
        .filter(|p| p.0.is_finite())
        .collect();
    // The view: both paths with a margin.
    let all: Vec<(f64, f64)> = og.iter().chain(pt.iter()).copied().collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &all {
        x0 = x0.min(p.0);
        x1 = x1.max(p.0);
        y0 = y0.min(p.1);
        y1 = y1.max(p.1);
    }
    let pad = 2.5;
    let (x0, y0, x1, y1) = (x0 - pad, y0 - pad, x1 + pad, y1 + pad);
    let scale = (900.0 / (x1 - x0)).min(600.0 / (y1 - y0));
    let (w, h) = (
        ((x1 - x0) * scale) as usize + 1,
        ((y1 - y0) * scale) as usize + 1,
    );
    let mut c = Canvas::new(w, h);
    let tx = |x: f64| ((x - x0) * scale) as i64;
    let ty = |y: f64| (h as f64 - 1.0 - (y - y0) * scale) as i64;
    for region in load_scene(id) {
        let room = region.room();
        for i in 0..room.counts[5] {
            let e = room.edge(i).map(|v| v as f64 / 65536.0);
            if e == [0.0; 4] {
                continue;
            }
            c.line((tx(e[0]), ty(e[1])), (tx(e[2]), ty(e[3])), [150, 150, 150]);
        }
    }
    for (k, p) in og.iter().enumerate() {
        c.dot(tx(p.0), ty(p.1), if k == 0 { 5 } else { 1 }, [30, 80, 220]);
    }
    for (k, p) in pt.iter().enumerate() {
        c.dot(tx(p.0), ty(p.1), if k == 0 { 4 } else { 0 }, [220, 40, 40]);
    }
    c.save(out);
    println!(
        "{} ({}x{}): blue original {} points, red port {} points",
        out.display(),
        w,
        h,
        og.len(),
        pt.len()
    );
}
