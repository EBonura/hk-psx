//! Parity probes for the Rust cookers against the Python ones, during the
//! migration only (removed with the last Python cooker).
//!
//!   hk-cook-parity ops <data dir> <sprite list> <out>
//!     Pillow operations on props.py's path, applied to real sprites with the
//!     same fixed parameters as the stage-3 oracle; one line of hashes each.
use hk_cook::materials::quantize_alpha_coverage;
use hk_pil::resample::Filter;
use hk_pil::Image;
use hk_unity::{Obj, Source};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

fn h(b: &[u8]) -> String {
    Sha256::digest(b).iter().take(12).map(|x| format!("{x:02x}")).collect()
}

/// Image.tobytes(): RGB without its pad byte.
fn tobytes(im: &Image) -> Vec<u8> {
    match im.mode {
        hk_pil::Mode::Rgb => im.data.chunks_exact(4).flat_map(|p| p[..3].to_vec()).collect(),
        _ => im.data.clone(),
    }
}

fn ops(source: &Source, key: &str, pid: i64) -> String {
    let file = source.file(key).unwrap();
    let obj = Obj { file: file.clone(), info: *file.object(pid).unwrap() };
    let (im, _) = hk_unity::texture::sprite_image(source, &obj).unwrap();
    let (w, h_) = (im.width, im.height);
    let mut res: Vec<String> = Vec::new();
    let a = im.resize(((w as f64 * 0.55).ceil() as usize).max(1), ((h_ as f64 * 0.7).ceil() as usize).max(1), Filter::Lanczos);
    res.push(format!("A={}", h(&tobytes(&a))));
    res.push(format!("B={}", h(&tobytes(&im.resize(w * 2 + 1, (h_ / 2).max(1), Filter::Lanczos)))));
    res.push(format!("C={}", h(&tobytes(&im.resize((w / 3).max(1), h_ + 5, Filter::Bilinear)))));
    res.push(format!("D={}", h(&tobytes(&im.resize((w / 3).max(1), h_ + 5, Filter::Box)))));
    let (tw, th) = ((w / 2 + 3).max(1), (h_ / 2 + 1).max(1));
    let e = im.affine_bilinear(tw, th, [w as f64 / tw as f64 * 0.9, 0.13, 1.25, -0.07, h_ as f64 / th as f64, 0.5]);
    res.push(format!("E={}", h(&tobytes(&e))));
    let f = im.affine_bilinear(th, tw, [0.0, w as f64 / th as f64, 0.0, h_ as f64 / tw as f64, 0.0, 0.0]);
    res.push(format!("F={}", h(&tobytes(&f))));
    for op in [128, 85, 43] {
        match quantize_alpha_coverage(&a, op) {
            Ok(q) => {
                let mut b = q.palette.to_vec();
                b.extend_from_slice(&q.packed);
                res.push(format!("Q{op}={}", h(&b)));
            }
            Err(_) => res.push(format!("Q{op}=ERR")),
        }
    }
    format!("{key}\t{pid}\t{w}x{h_}\t{}\n", res.join(" "))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let lazy_source = || Source::new(&args[2]).expect("source");
    match args[1].as_str() {
        "ops" => {
            let list: Vec<(String, i64)> = std::fs::read_to_string(&args[3])
                .unwrap()
                .lines()
                .map(|l| {
                    let f: Vec<&str> = l.split('\t').collect();
                    (f[0].to_string(), f[1].parse().unwrap())
                })
                .collect();
            let start = std::time::Instant::now();
            let source = lazy_source();
            let lines: Vec<String> = list.par_iter().map(|(k, p)| ops(&source, k, *p)).collect();
            eprintln!("{} sprites in {:.2?}", list.len(), start.elapsed());
            std::fs::write(&args[4], lines.concat()).unwrap();
        }
        "lz4" => {
            // hk-cook-parity lz4 _ <file list> <out>: sha of the HC block per file.
            let files: Vec<String> = std::fs::read_to_string(&args[3]).unwrap().lines().map(str::to_string).collect();
            let start = std::time::Instant::now();
            let lines: Vec<String> = files
                .par_iter()
                .map(|f| {
                    let data = std::fs::read(f).unwrap();
                    let z = hk_lz4::compress_hc(&data);
                    assert_eq!(hk_lz4::decompress(&z, data.len()).as_deref(), Some(&data[..]), "round trip {f}");
                    format!("{f}\t{}\t{}\n", z.len(), h(&z))
                })
                .collect();
            eprintln!("{} files in {:.2?}", files.len(), start.elapsed());
            std::fs::write(&args[4], lines.concat()).unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
