//! Parity probe for hk-unity: prints, per object, the hash of its canonical
//! value in the same format as the stage-2 oracle, so the two can be diffed.
//!
//!   hk-unity-parity objects <data dir> <file list> <out>
//!   hk-unity-parity one <data dir> <file> <path id>
use hk_unity::{Obj, Source};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::time::Instant;

fn line(source: &Source, obj: &Obj, key: &str) -> String {
    let typename = source.typename(obj);
    let value = source.read(obj);
    match (typename, value) {
        (Ok(t), Ok(v)) => {
            let mut s = String::new();
            v.canonical(&mut s);
            let hash = Sha256::digest(s.as_bytes());
            let hex: String = hash.iter().take(12).map(|b| format!("{b:02x}")).collect();
            format!(
                "{key}\t{}\t{}\t{t}\t{hex}\t{}\n",
                obj.path_id(),
                obj.class_id(),
                s.len()
            )
        }
        (t, v) => {
            let err = v
                .err()
                .map(|e| e.to_string())
                .or(t.err().map(|e| e.to_string()))
                .unwrap_or_default();
            format!(
                "{key}\t{}\t{}\t?\tERR {}\t0\n",
                obj.path_id(),
                obj.class_id(),
                &err[..err.len().min(120)]
            )
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let source = Source::new(&args[2]).expect("source");
    match args[1].as_str() {
        "objects" => {
            let files: Vec<String> = std::fs::read_to_string(&args[3])
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            let start = Instant::now();
            let chunks: Vec<String> = files
                .par_iter()
                .map(|key| {
                    let file = source.file(key).expect("file");
                    let objs: Vec<Obj> = file
                        .objects
                        .iter()
                        .map(|i| Obj {
                            file: file.clone(),
                            info: *i,
                        })
                        .collect();
                    objs.par_iter()
                        .map(|o| line(&source, o, key))
                        .collect::<Vec<_>>()
                        .concat()
                })
                .collect();
            let elapsed = start.elapsed();
            let mut out = std::fs::File::create(&args[4]).unwrap();
            for c in &chunks {
                out.write_all(c.as_bytes()).unwrap();
            }
            eprintln!("{} files in {:.2?}", files.len(), elapsed);
        }
        "generated" => {
            // Compare the generator with TypeTreeGeneratorAPI's dump (oracle `generated` mode).
            let text = std::fs::read_to_string(&args[3]).unwrap();
            let generator = hk_unity::generator::Generator::load(
                &std::path::Path::new(&args[2]).join("Managed"),
                "",
            )
            .unwrap();
            let mut lines = text.lines();
            let (mut same, mut differ, mut both_fail, mut we_fail, mut they_fail) = (0, 0, 0, 0, 0);
            let start = Instant::now();
            while let Some(line) = lines.next() {
                let f: Vec<&str> = line.split('\t').collect();
                let ours = generator.nodes(f[1], f[2]);
                if f[0] == "error" {
                    if ours.is_err() {
                        both_fail += 1;
                    } else {
                        they_fail += 1;
                        if they_fail <= 5 {
                            println!("THEY FAIL, WE GENERATE: {} {}", f[1], f[2]);
                        }
                    }
                    continue;
                }
                let n: usize = f[3].parse().unwrap();
                let expected: Vec<String> =
                    (0..n).map(|_| lines.next().unwrap().to_string()).collect();
                match ours {
                    Err(e) => {
                        we_fail += 1;
                        if we_fail <= 10 {
                            println!("WE FAIL: {} {}: {e}", f[1], f[2]);
                        }
                    }
                    Ok(node) => {
                        let got: Vec<String> = node
                            .rows()
                            .into_iter()
                            .map(|(l, t, n, m)| format!("{l}\t{t}\t{n}\t{m}"))
                            .collect();
                        if got == expected {
                            same += 1;
                        } else {
                            differ += 1;
                            if differ <= 6 {
                                println!("DIFFER: {} {}", f[1], f[2]);
                                let i = got
                                    .iter()
                                    .zip(&expected)
                                    .position(|(a, b)| a != b)
                                    .unwrap_or(got.len().min(expected.len()));
                                for k in i.saturating_sub(2)..(i + 4) {
                                    println!(
                                        "   ours {:<50} theirs {}",
                                        got.get(k).map_or("-", |s| s),
                                        expected.get(k).map_or("-", |s| s)
                                    );
                                }
                            }
                        }
                    }
                }
            }
            println!("same {same} differ {differ} we-fail {we_fail} both-fail {both_fail} they-fail {they_fail} in {:.2?}", start.elapsed());
        }
        "scenes" => {
            // Scene-level parity with the oracle's `scenes` mode: hk-unity-parity scenes <data dir> <scene table tsv> <out>
            // The table lists (scene name, level file) in SCENE_TABLE order.
            let table: Vec<(String, String)> = std::fs::read_to_string(&args[3])
                .unwrap()
                .lines()
                .map(|l| {
                    let f: Vec<&str> = l.split('\t').collect();
                    (f[0].to_string(), f[1].to_string())
                })
                .collect();
            let start = Instant::now();
            let lines: Vec<String> = table
                .par_iter()
                .map(|(name, file)| {
                    let sc = match hk_unity::scene::Scene::new(&source, file) {
                        Ok(sc) => sc,
                        Err(e) => return format!("{name}\tERR {e}\n"),
                    };
                    let mut h = Sha256::new();
                    for o in &sc.objects {
                        let mut s = String::new();
                        o.tree.canonical(&mut s);
                        h.update(format!("{}\t{}\t", o.id, o.typename).as_bytes());
                        h.update(s.as_bytes());
                        h.update(b"\n");
                    }
                    let mut tids: Vec<i64> = sc.transforms.keys().copied().collect();
                    tids.sort();
                    let mut w = Sha256::new();
                    for tid in tids {
                        match sc.world(tid) {
                            Ok(m) => {
                                for row in m {
                                    for v in row {
                                        w.update(v.to_le_bytes());
                                    }
                                }
                            }
                            Err(_) => w.update(b"E"),
                        }
                    }
                    let mut gos: Vec<i64> = sc.gos.keys().copied().collect();
                    gos.sort();
                    let act: Vec<i64> = gos.into_iter().filter(|&g| sc.active(g)).collect();
                    let mut off: Vec<i64> = sc.gated_off.iter().copied().collect();
                    off.sort();
                    let pylist = |v: &[i64]| {
                        format!(
                            "[{}]",
                            v.iter()
                                .map(|x| x.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    let short = |d: &[u8]| {
                        d.iter()
                            .take(8)
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    };
                    let sids: Vec<String> = sc.objects.iter().map(|o| sc.sid(o.id)).collect();
                    format!(
                        "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                        sc.objects.len(),
                        sc.errors.len(),
                        sc.additive.join(","),
                        off.len(),
                        short(&Sha256::digest(pylist(&off).as_bytes())),
                        short(&h.finalize()),
                        short(&w.finalize()),
                        act.len(),
                        short(&Sha256::digest(pylist(&act).as_bytes())),
                        short(&Sha256::digest(sids.join("\n").as_bytes())),
                    )
                })
                .collect();
            eprintln!("{} scenes in {:.2?}", table.len(), start.elapsed());
            std::fs::write(&args[4], lines.concat()).unwrap();
        }
        "Texture2D" | "Sprite" => {
            // Pixel parity with the oracle's Texture2D / Sprite modes: <data dir> <file list> <out>
            let kind = args[1].clone();
            let class = if kind == "Texture2D" { 28 } else { 213 };
            let files: Vec<String> = std::fs::read_to_string(&args[3])
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            let start = Instant::now();
            let chunks: Vec<String> = files
                .par_iter()
                .map(|key| {
                    let file = source.file(key).expect("file");
                    let objs: Vec<Obj> = file
                        .objects
                        .iter()
                        .filter(|i| i.class_id == class)
                        .map(|i| Obj {
                            file: file.clone(),
                            info: *i,
                        })
                        .collect();
                    objs.par_iter()
                        .map(|o| {
                            let img = if class == 28 {
                                hk_unity::texture::texture_image(&source, o, true)
                                    .map(|i| i.to_rgba())
                            } else {
                                hk_unity::texture::sprite_image(&source, o).map(|(i, _)| i)
                            };
                            match img {
                                Ok(i) => {
                                    let hash: String = Sha256::digest(&i.data)
                                        .iter()
                                        .take(12)
                                        .map(|b| format!("{b:02x}"))
                                        .collect();
                                    format!(
                                        "{key}\t{}\tRGBA\t{}x{}\t{hash}\n",
                                        o.path_id(),
                                        i.width,
                                        i.height
                                    )
                                }
                                Err(e) => format!("{key}\t{}\tERR {e}\n", o.path_id()),
                            }
                        })
                        .collect::<Vec<_>>()
                        .concat()
                })
                .collect();
            eprintln!("{kind}: {} files in {:.2?}", files.len(), start.elapsed());
            std::fs::write(&args[4], chunks.concat()).unwrap();
        }
        "one" => {
            let file = source.file(&args[3]).unwrap();
            let obj = source.object(&file, args[4].parse().unwrap()).unwrap();
            let mut s = String::new();
            source.read(&obj).unwrap().canonical(&mut s);
            println!("{s}");
        }
        other => panic!("unknown mode {other}"),
    }
}
