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
    Sha256::digest(b)
        .iter()
        .take(12)
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// Image.tobytes(): RGB without its pad byte.
fn tobytes(im: &Image) -> Vec<u8> {
    match im.mode {
        hk_pil::Mode::Rgb => im
            .data
            .chunks_exact(4)
            .flat_map(|p| p[..3].to_vec())
            .collect(),
        _ => im.data.clone(),
    }
}

fn ops(source: &Source, key: &str, pid: i64) -> String {
    let file = source.file(key).unwrap();
    let obj = Obj {
        file: file.clone(),
        info: *file.object(pid).unwrap(),
    };
    let (im, _) = hk_unity::texture::sprite_image(source, &obj).unwrap();
    let (w, h_) = (im.width, im.height);
    let mut res: Vec<String> = Vec::new();
    let a = im.resize(
        ((w as f64 * 0.55).ceil() as usize).max(1),
        ((h_ as f64 * 0.7).ceil() as usize).max(1),
        Filter::Lanczos,
    );
    res.push(format!("A={}", h(&tobytes(&a))));
    res.push(format!(
        "B={}",
        h(&tobytes(&im.resize(
            w * 2 + 1,
            (h_ / 2).max(1),
            Filter::Lanczos
        )))
    ));
    res.push(format!(
        "C={}",
        h(&tobytes(&im.resize(
            (w / 3).max(1),
            h_ + 5,
            Filter::Bilinear
        )))
    ));
    res.push(format!(
        "D={}",
        h(&tobytes(&im.resize((w / 3).max(1), h_ + 5, Filter::Box)))
    ));
    let (tw, th) = ((w / 2 + 3).max(1), (h_ / 2 + 1).max(1));
    let e = im.affine_bilinear(
        tw,
        th,
        [
            w as f64 / tw as f64 * 0.9,
            0.13,
            1.25,
            -0.07,
            h_ as f64 / th as f64,
            0.5,
        ],
    );
    res.push(format!("E={}", h(&tobytes(&e))));
    let f = im.affine_bilinear(
        th,
        tw,
        [
            0.0,
            w as f64 / th as f64,
            0.0,
            h_ as f64 / tw as f64,
            0.0,
            0.0,
        ],
    );
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
            let files: Vec<String> = std::fs::read_to_string(&args[3])
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            let start = std::time::Instant::now();
            let lines: Vec<String> = files
                .par_iter()
                .map(|f| {
                    let data = std::fs::read(f).unwrap();
                    let z = hk_lz4::compress_hc(&data);
                    assert_eq!(
                        hk_lz4::decompress(&z, data.len()).as_deref(),
                        Some(&data[..]),
                        "round trip {f}"
                    );
                    format!("{f}\t{}\t{}\n", z.len(), h(&z))
                })
                .collect();
            eprintln!("{} files in {:.2?}", files.len(), start.elapsed());
            std::fs::write(&args[4], lines.concat()).unwrap();
        }
        "actors" => {
            // hk-cook-parity actors <source dir> <oracle dir> <regions.json>: the controls the
            // ported recognizers give each actor against the Python actor_sources dump.
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions {
                f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone())
            } else {
                None
            }) else {
                panic!("no scenes")
            };
            let get = |j: &Json, k: &str| -> Option<Json> {
                if let Json::Obj(f) = j {
                    f.iter().find(|x| x.0 == k).map(|x| x.1.clone())
                } else {
                    None
                }
            };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let Some(Json::Str(file)) = get(s, "file") else {
                        panic!("scene file")
                    };
                    let Some(Json::Str(name)) = get(s, "scene_name") else {
                        panic!("scene name")
                    };
                    let Some(Json::List(b)) = get(s, "runtime_bounds") else {
                        panic!("runtime_bounds")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let ported = [
                "ZombieSwipeWalker",
                "Baldur",
                "Aspid",
                "FalseKnight",
                "Climber",
                "MossWalker",
                "Vengefly",
                "AcidFlyer",
                "Mosquito",
                "Gruzzer",
                "GruzzerReserve",
                "GruzMother",
                "WalkLeftRight",
                "EggSac",
                "Mawlek",
                "ZombieShield",
                "Pigeon",
                "Blocker",
                "HuskGuard",
                "Hatcher",
                "HatcherBaby",
            ];
            let (mut checked, mut bad) =
                (std::collections::BTreeMap::<String, usize>::new(), 0usize);
            for s in &scenes {
                let (Some(Json::Str(name)), Some(Json::Str(file))) =
                    (get(s, "scene_name"), get(s, "file"))
                else {
                    panic!("scene row")
                };
                let oracle =
                    parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap())
                        .unwrap();
                let Json::List(oracle) = oracle else {
                    panic!("oracle shape")
                };
                let sc = Scene::new(&source, &file).unwrap();
                let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
                for row in &oracle {
                    let Some(Json::Str(src)) = get(row, "source") else {
                        continue;
                    };
                    let control = get(row, "movement_control");
                    let kind = control.as_ref().and_then(|c| get(c, "kind"));
                    let mine = rows
                        .iter()
                        .find(|r| r.source == src)
                        .and_then(|r| r.control.as_ref());
                    match (&kind, mine) {
                        (Some(Json::Str(k)), m) if ported.contains(&k.as_str()) => {
                            *checked.entry(k.clone()).or_default() += 1;
                            if m.map(|m| &m.1) != control.as_ref() {
                                bad += 1;
                                println!(
                                    "{name} {src}: {k} control differs{}",
                                    if m.is_none() { " (not recognized)" } else { "" }
                                );
                            }
                            // `movement_supported` carries the additive-scene rule.
                            let python =
                                matches!(get(row, "movement_supported"), Some(Json::Bool(true)));
                            if rows
                                .iter()
                                .find(|r| r.source == src)
                                .is_some_and(|r| r.supported != python)
                            {
                                bad += 1;
                                println!(
                                    "{name} {src}: {k} supported flag differs (oracle {python})"
                                );
                            }
                        }
                        (_, Some((k, _))) => {
                            bad += 1;
                            println!("{name} {src}: recognized as {k}, oracle has {:?}", kind);
                        }
                        _ => {}
                    }
                    // The rest of the actor record the cook reads.
                    if let Some(r) = rows.iter().find(|r| r.source == src) {
                        let strs = |v: &[String]| {
                            Json::List(v.iter().map(|s| Json::Str(s.clone())).collect())
                        };
                        let mine = [
                            (
                                "position",
                                Json::List(r.position.iter().map(|&f| Json::Float(f)).collect()),
                            ),
                            (
                                "health",
                                hk_cook::music::value_json(r.health_manager.get("hp").unwrap()),
                            ),
                            (
                                "components",
                                Json::Obj(
                                    r.components
                                        .iter()
                                        .map(|c| (c.0.to_string(), Json::Str(c.1.clone())))
                                        .collect(),
                                ),
                            ),
                            ("colliders", r.colliders.clone()),
                            ("fsm_ids", strs(&r.fsm_ids)),
                            ("limitations", strs(&r.limitations)),
                        ];
                        for (key, value) in mine {
                            if get(row, key).as_ref() != Some(&value) {
                                bad += 1;
                                println!("{name} {src}: {key} differs");
                            }
                        }
                        *checked.entry("rows".to_string()).or_default() += 1;
                    } else {
                        bad += 1;
                        println!("{name} {src}: no such actor");
                    }
                }
                if rows.len() != oracle.len() {
                    bad += 1;
                    println!("{name}: {} actors, oracle has {}", rows.len(), oracle.len());
                }
            }
            println!("checked {checked:?}, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "specs" => {
            // hk-cook-parity specs <source dir> <oracle dir> <regions.json>: the ActorSpec text,
            // placements and scene bank against actors.py, over synthetic clip bindings and corpses.
            use hk_cook::actor_specs::{
                generated_actor_records, scene_actor_bank, Region, SpecActor,
            };
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions {
                f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone())
            } else {
                None
            }) else {
                panic!("no scenes")
            };
            let get = |j: &Json, k: &str| -> Option<Json> {
                if let Json::Obj(f) = j {
                    f.iter().find(|x| x.0 == k).map(|x| x.1.clone())
                } else {
                    None
                }
            };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let Some(Json::Str(file)) = get(s, "file") else {
                        panic!("scene file")
                    };
                    let Some(Json::Str(name)) = get(s, "scene_name") else {
                        panic!("scene name")
                    };
                    let Some(Json::List(b)) = get(s, "runtime_bounds") else {
                        panic!("runtime_bounds")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let Json::List(keys) =
                parse(&std::fs::read_to_string(format!("{}/_keys.json", args[3])).unwrap())
                    .unwrap()
            else {
                panic!("keys")
            };
            let keys: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    if let Json::Str(s) = k {
                        s
                    } else {
                        panic!("key")
                    }
                })
                .collect();
            // The oracle's synthetic bindings (oracle_specs.py): deterministic in the actor index.
            let v = |idx: i64, key: &str| {
                (idx * 131 + key.bytes().map(i64::from).sum::<i64>() * 7 + key.len() as i64) % 60000
            };
            let corpse = |idx: i64| {
                let mut c = vec![
                    ("air_clip", Json::Int(v(idx, "air"))),
                    ("land_clip", Json::Int(v(idx, "land"))),
                    (
                        "bounds",
                        Json::List(
                            ["b0", "b1", "b2", "b3"]
                                .iter()
                                .map(|k| Json::Int(v(idx, k)))
                                .collect(),
                        ),
                    ),
                    (
                        "spawn_offset",
                        Json::List(["s0", "s1"].iter().map(|k| Json::Int(v(idx, k))).collect()),
                    ),
                    ("bounce_factor", Json::Int((idx * 977) % 65537)),
                ];
                if idx % 3 == 1 {
                    c.extend([
                        ("breaker", Json::Bool(true)),
                        ("smash_bounces", Json::Int(2)),
                        ("hold_ticks", Json::Int(7)),
                    ]);
                }
                if idx % 3 == 2 {
                    c.extend([
                        ("fling_speed", Json::Int(983040)),
                        ("gravity", Json::Int(3145728)),
                        ("remove_after_land", Json::Int(1)),
                    ]);
                }
                Json::Obj(c.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
            };
            let nocorpse = [
                "WalkLeftRight",
                "GruzMother",
                "Hatcher",
                "HatcherBaby",
                "ZombieShield",
                "Blocker",
                "Pigeon",
                "Mawlek",
                "FalseKnight",
            ];
            let (mut checked, mut bad) =
                (std::collections::BTreeMap::<String, usize>::new(), 0usize);
            for s in &scenes {
                let Some(Json::Str(name)) = get(s, "scene_name") else {
                    panic!("scene row")
                };
                let Some(Json::Str(file)) = get(s, "file") else {
                    panic!("scene row")
                };
                let Some(oracle) =
                    parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap())
                        .ok()
                else {
                    panic!("oracle")
                };
                let sc = Scene::new(&source, &file).unwrap();
                let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
                let clips: Vec<Vec<(String, i64)>> = (0..rows.len())
                    .map(|i| keys.iter().map(|k| (k.clone(), v(i as i64, k))).collect())
                    .collect();
                let corpses: Vec<Option<Json>> = rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let kind = r.control.as_ref().map(|c| c.0.as_str());
                        if i % 2 == 0 && kind.is_some_and(|k| nocorpse.contains(&k)) {
                            None
                        } else {
                            Some(corpse(i as i64))
                        }
                    })
                    .collect();
                let actor = |i: usize| SpecActor {
                    row: &rows[i],
                    clips: &clips[i],
                    corpse: corpses[i].as_ref(),
                };
                let mut chunks: Vec<(i64, Vec<usize>)> = Vec::new();
                for i in 0..rows.len() {
                    let c = (i as i64 * 5) % 4;
                    match chunks.iter_mut().find(|k| k.0 == c) {
                        Some(k) => k.1.push(i),
                        None => chunks.push((c, vec![i])),
                    }
                }
                let Some(Json::List(want_regions)) = get(&oracle, "regions") else {
                    panic!("regions")
                };
                for (chunk, members) in &chunks {
                    let want = want_regions
                        .iter()
                        .find(|r| get(r, "chunk_id") == Some(Json::Int(*chunk)))
                        .expect("oracle region");
                    let actors: Vec<SpecActor> = members.iter().map(|&i| actor(i)).collect();
                    let got = generated_actor_records(&actors);
                    let python_error = !matches!(get(want, "error"), Some(Json::Null));
                    match (&got, python_error) {
                        (Ok(g), false) => {
                            let mine = Json::List(
                                g.iter()
                                    .map(|(t, p)| {
                                        Json::List(vec![Json::Str(t.clone()), p.to_json()])
                                    })
                                    .collect(),
                            );
                            *checked.entry("records".into()).or_default() += g.len();
                            if get(want, "records") != Some(mine) {
                                bad += 1;
                                println!("{name} chunk {chunk}: records differ");
                            }
                        }
                        (Err(_), true) => *checked.entry("agreed errors".into()).or_default() += 1,
                        _ => {
                            bad += 1;
                            println!("{name} chunk {chunk}: error disagreement (rust ok: {}, python error: {python_error})", got.is_ok());
                        }
                    }
                }
                let regions: Vec<Region> = chunks
                    .iter()
                    .map(|(c, m)| Region {
                        chunk_id: *c,
                        actors: m.iter().map(|&i| actor(i)).collect(),
                    })
                    .collect();
                let want = get(&oracle, "bank").unwrap();
                match (scene_actor_bank(&regions), get(&want, "error")) {
                    (Ok((specs, placed)), None) => {
                        let mine = Json::Obj(vec![
                            (
                                "specs".into(),
                                Json::List(specs.into_iter().map(Json::Str).collect()),
                            ),
                            (
                                "placed".into(),
                                Json::Obj(
                                    placed
                                        .into_iter()
                                        .map(|(k, (i, p))| {
                                            (k, Json::List(vec![Json::Int(i as i64), p.to_json()]))
                                        })
                                        .collect(),
                                ),
                            ),
                        ]);
                        *checked.entry("banks".into()).or_default() += 1;
                        if want != mine {
                            bad += 1;
                            println!("{name}: bank differs");
                        }
                    }
                    (Err(_), Some(_)) => {
                        *checked.entry("agreed bank errors".into()).or_default() += 1
                    }
                    (r, _) => {
                        bad += 1;
                        println!("{name}: bank error disagreement (rust ok: {})", r.is_ok());
                    }
                }
            }
            println!("checked {checked:?}, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "vitals" => {
            // hk-cook-parity vitals <source dir> <oracle-vitals.json>: the CIL-read vital values and
            // the parameter blocks generated from them, against actors.py.
            use hk_cook::pyjson::{parse, Json};
            let source = lazy_source();
            let oracle = parse(&std::fs::read_to_string(&args[3]).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let Json::Obj(consts) = field(&oracle, "constants") else {
                panic!("constants")
            };
            let constants: Vec<(String, f64)> = consts
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        match v {
                            Json::Int(i) => i as f64,
                            Json::Float(f) => f,
                            _ => panic!("constant"),
                        },
                    )
                })
                .collect();
            let mut bad = 0;
            let values = hk_cook::vitals::source_vital_values(&source, &constants).unwrap();
            if values != field(&oracle, "values") {
                bad += 1;
                println!("vital values differ");
            }
            if Json::Str(hk_cook::vitals::generated_vital_params(&values).unwrap())
                != field(&oracle, "vital")
            {
                bad += 1;
                println!("vital params differ");
            }
            let Json::Obj(nails) = field(&oracle, "nail") else {
                panic!("nail")
            };
            for (dt, text) in nails {
                if Json::Str(
                    hk_cook::vitals::generated_nail_response_params(
                        &constants,
                        dt.parse().unwrap(),
                    )
                    .unwrap(),
                ) != text
                {
                    bad += 1;
                    println!("nail params differ at dt {dt}");
                }
            }
            println!("checked vitals, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "polygons" => {
            // hk-cook-parity polygons <oracle-polygons.json>: bounded_polygons against host/polygons.py.
            use hk_cook::pyjson::{parse, Json};
            let Json::List(cases) = parse(&std::fs::read_to_string(&args[2]).unwrap()).unwrap()
            else {
                panic!("cases")
            };
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap()
                } else {
                    panic!("not an object")
                }
            };
            let f = |j: &Json| match j {
                Json::Int(i) => *i as f64,
                Json::Float(x) => *x,
                _ => panic!("coordinate"),
            };
            let (mut ok, mut agreed, mut bad) = (0, 0, 0);
            for (n, case) in cases.iter().enumerate() {
                let Json::List(points) = field(case, "points") else {
                    panic!("points")
                };
                let points: Vec<(f64, f64)> = points
                    .iter()
                    .map(|p| {
                        let Json::List(p) = p else { panic!("pt") };
                        (f(&p[0]), f(&p[1]))
                    })
                    .collect();
                let Json::Int(limit) = field(case, "limit") else {
                    panic!("limit")
                };
                let got = hk_cook::polygons::bounded_polygons(&points, limit as usize);
                match (got, field(case, "error")) {
                    (Ok(pieces), Json::Null) => {
                        let mine = Json::List(
                            pieces
                                .iter()
                                .map(|p| {
                                    Json::List(
                                        p.iter()
                                            .map(|q| {
                                                Json::List(vec![Json::Float(q.0), Json::Float(q.1)])
                                            })
                                            .collect(),
                                    )
                                })
                                .collect(),
                        );
                        if mine == field(case, "pieces") {
                            ok += 1
                        } else {
                            bad += 1;
                            println!("case {n}: pieces differ")
                        }
                    }
                    (Err(e), Json::Str(want)) => {
                        if e == want || (want == "KeyError" && e.starts_with("KeyError")) {
                            agreed += 1
                        } else {
                            bad += 1;
                            println!("case {n}: error {e:?}, python {want:?}")
                        }
                    }
                    (r, want) => {
                        bad += 1;
                        println!("case {n}: rust ok {}, python error {want:?}", r.is_ok())
                    }
                }
            }
            println!("checked {ok} splits and {agreed} refusals, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "static" => {
            // hk-cook-parity static <source dir> <oracle dir> <regions.json>: hazard_sources,
            // shroom_sources and pogo_sources against actors.py.
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions {
                f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone())
            } else {
                None
            }) else {
                panic!("no scenes")
            };
            let get = |j: &Json, k: &str| -> Option<Json> {
                if let Json::Obj(f) = j {
                    f.iter().find(|x| x.0 == k).map(|x| x.1.clone())
                } else {
                    None
                }
            };
            let (mut checked, mut bad) =
                (std::collections::BTreeMap::<String, usize>::new(), 0usize);
            for s in &scenes {
                let (Some(Json::Str(name)), Some(Json::Str(file))) =
                    (get(s, "scene_name"), get(s, "file"))
                else {
                    panic!("scene row")
                };
                let oracle =
                    parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap())
                        .unwrap();
                let sc = Scene::new(&source, &file).unwrap();
                let results = [
                    (
                        "hazards",
                        hk_cook::static_sources::hazard_sources(&sc, None).map(Json::List),
                    ),
                    (
                        "shrooms",
                        hk_cook::static_sources::shroom_sources(&sc, None).map(Json::List),
                    ),
                    ("pogo", hk_cook::static_sources::pogo_sources(&sc)),
                ];
                for (key, got) in results {
                    let want = get(&oracle, key).unwrap();
                    let python_error = get(&want, "error");
                    match (got, python_error) {
                        (Ok(g), None) => {
                            *checked.entry(key.into()).or_default() += 1;
                            if g != want {
                                bad += 1;
                                println!("{name}: {key} differs");
                            }
                        }
                        (Err(_), Some(_)) => {
                            *checked.entry(format!("{key} agreed errors")).or_default() += 1
                        }
                        (r, _) => {
                            bad += 1;
                            println!("{name}: {key} error disagreement (rust ok: {})", r.is_ok());
                        }
                    }
                }
            }
            println!("checked {checked:?}, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "fkart" => {
            // hk-cook-parity fkart <source dir> <oracle-fkart.json> <regions.json>: the pure parts of
            // false_knight_art.py (frame decomposition, shockwave and placement facts, generated tables).
            use hk_cook::false_knight_art as fka;
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let oracle = parse(&std::fs::read_to_string(&args[3]).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let ints = |j: &Json| -> Vec<i64> {
                let Json::List(l) = j else { panic!("list") };
                l.iter()
                    .map(|v| {
                        if let Json::Int(i) = v {
                            *i
                        } else {
                            panic!("int")
                        }
                    })
                    .collect()
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let mut bad = 0;
            let (mut cut, mut refused) = (0, 0);
            for case in list(field(&oracle, "decompose")) {
                let (Json::Int(w), Json::Int(h), Json::Bool(streamed), Json::Str(plane)) = (
                    field(&case, "w"),
                    field(&case, "h"),
                    field(&case, "streamed"),
                    field(&case, "plane"),
                ) else {
                    panic!("case")
                };
                let plane: Vec<u8> = plane.bytes().map(|b| b - b'0').collect();
                match (
                    fka::decompose(&plane, w as usize, h as usize, streamed),
                    field(&case, "error"),
                ) {
                    (Ok(parts), Json::Null) => {
                        let mine = Json::List(
                            parts
                                .iter()
                                .map(|p| Json::List(p.iter().map(|&v| Json::Int(v)).collect()))
                                .collect(),
                        );
                        if mine == field(&case, "parts") {
                            cut += 1
                        } else {
                            bad += 1;
                            println!("decompose {w}x{h} streamed={streamed}: parts differ")
                        }
                    }
                    (Err(e), Json::Str(want)) if e == want => refused += 1,
                    (r, want) => {
                        bad += 1;
                        println!(
                            "decompose {w}x{h}: rust ok {}, python error {want:?}",
                            r.is_ok()
                        )
                    }
                }
            }
            for case in list(field(&oracle, "part_box")) {
                let b = ints(&field(&case, "box"));
                let (Json::Int(w), Json::Int(h)) = (field(&case, "w"), field(&case, "h")) else {
                    panic!("w h")
                };
                let r = ints(&field(&case, "rect"));
                let mine = fka::part_box([b[0], b[1], b[2], b[3]], w, h, [r[0], r[1], r[2], r[3]]);
                if mine.to_vec() != ints(&field(&case, "out")) {
                    bad += 1;
                    println!("part_box differs")
                }
            }
            // The False Knight's scene.
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Json::List(scenes) = field(&regions, "scenes") else {
                panic!("scenes")
            };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let (Json::Str(file), Json::Str(name)) =
                        (field(s, "file"), field(s, "scene_name"))
                    else {
                        panic!("scene")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    let Json::List(b) = field(s, "runtime_bounds") else {
                        panic!("bounds")
                    };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let sc = Scene::new(&source, "level46").unwrap();
            let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
            let fk = rows
                .iter()
                .find(|r| r.control.as_ref().is_some_and(|c| c.0 == "FalseKnight"))
                .expect("False Knight");
            let wave = fka::shockwave_source(&sc, &source, fk.game_object).unwrap();
            let want_wave = field(&oracle, "wave");
            if wave.params != field(&want_wave, "params")
                || Json::Str(wave.clip_name.clone()) != field(&want_wave, "clip")
                || Json::Str(wave.library.sid()) != field(&want_wave, "library")
            {
                bad += 1;
                println!("shockwave differs");
            }
            let objects = fka::source_objects(&sc, &source, fk).unwrap();
            if objects != field(&oracle, "objects") {
                bad += 1;
                println!("objects differ: {}", hk_cook::pyjson::dumps(&objects));
            }
            let ti = field(&oracle, "table_inputs");
            let sprite_rows: Vec<(i64, i64, bool)> = list(field(&ti, "sprite_rows"))
                .iter()
                .map(|r| {
                    let v = ints(r);
                    (v[0], v[1], v[2] != 0)
                })
                .collect();
            let clip_rows: Vec<fka::ClipRow> = list(field(&ti, "clip_rows"))
                .iter()
                .map(|r| {
                    let Json::List(l) = r else { panic!("row") };
                    let v = ints(&Json::List(l[..5].to_vec()));
                    let Json::Str(n) = &l[5] else { panic!("name") };
                    (v[0], v[1], v[2], v[3], v[4], n.clone())
                })
                .collect();
            let floor_rows: Vec<(String, Vec<i64>)> = list(field(&ti, "floor_rows"))
                .iter()
                .map(|r| {
                    let Json::List(l) = r else { panic!("row") };
                    let Json::Str(n) = &l[0] else { panic!("state") };
                    (n.clone(), ints(&l[1]))
                })
                .collect();
            let table = fka::rust_table(
                123,
                &sprite_rows,
                &clip_rows,
                &ints(&field(&ti, "sequence")),
                &floor_rows,
                &objects,
            )
            .unwrap();
            if Json::Str(table) != field(&oracle, "rust_table") {
                bad += 1;
                println!("rust_table differs");
            }
            let bindings = vec![
                fka::Binding {
                    name: "FK_A".into(),
                    doc: "first doc".into(),
                    rows: vec![(1, 2), (3, 4)],
                    boxes: None,
                },
                fka::Binding {
                    name: "FK_B".into(),
                    doc: "boxes doc".into(),
                    rows: vec![],
                    boxes: Some(vec![[1, 2, 3, 4], [5, 6, 7, 8]]),
                },
            ];
            if Json::Str(fka::rust_bindings(&bindings, 7)) != field(&oracle, "rust_bindings") {
                bad += 1;
                println!("rust_bindings differs");
            }
            let rb = field(&oracle, "region_bindings");
            let rows_in: Vec<(i64, Vec<String>)> = list(field(&rb, "rows"))
                .iter()
                .map(|r| {
                    let Json::Int(c) = field(r, "chunk_id") else {
                        panic!("chunk")
                    };
                    (
                        c,
                        list(field(r, "edge_sources"))
                            .into_iter()
                            .map(|s| {
                                if let Json::Str(s) = s {
                                    s
                                } else {
                                    panic!("src")
                                }
                            })
                            .collect(),
                    )
                })
                .collect();
            let draws = field(&rb, "draws");
            let result = fka::region_bindings(&rows_in, &objects, &|c| {
                Ok(list(field(&draws, &c.to_string()))
                    .iter()
                    .map(|d| {
                        if let Json::Str(s) = field(d, "source") {
                            s
                        } else {
                            panic!("src")
                        }
                    })
                    .collect())
            })
            .unwrap();
            let mine = Json::Obj(
                result
                    .iter()
                    .map(|b| {
                        (
                            b.name.clone(),
                            match &b.boxes {
                                Some(boxes) => Json::Obj(vec![
                                    ("doc".into(), Json::Str(b.doc.clone())),
                                    (
                                        "boxes".into(),
                                        Json::List(
                                            boxes
                                                .iter()
                                                .map(|x| {
                                                    Json::List(
                                                        x.iter().map(|&v| Json::Int(v)).collect(),
                                                    )
                                                })
                                                .collect(),
                                        ),
                                    ),
                                ]),
                                None => Json::Obj(vec![
                                    ("doc".into(), Json::Str(b.doc.clone())),
                                    (
                                        "rows".into(),
                                        Json::List(
                                            b.rows
                                                .iter()
                                                .map(|&(a, c)| {
                                                    Json::List(vec![Json::Int(a), Json::Int(c)])
                                                })
                                                .collect(),
                                        ),
                                    ),
                                ]),
                            },
                        )
                    })
                    .collect(),
            );
            if mine != field(&rb, "result") {
                bad += 1;
                println!("region_bindings differ");
            }
            println!("checked {cut} cuts, {refused} refusals, wave, objects and tables, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "pogo" => {
            // hk-cook-parity pogo <source dir> <regions.json>: postpack_pogo over the report with its
            // pogo fields removed, against the fields the Python pass left in the report.
            use hk_cook::pyjson::{parse, Json};
            let source = lazy_source();
            let original = parse(&std::fs::read_to_string(&args[3]).unwrap()).unwrap();
            let mut report = original.clone();
            let strip = |report: &mut Json| {
                let Json::Obj(top) = report else {
                    panic!("report")
                };
                for (key, list) in top.iter_mut() {
                    if key == "scenes" || key == "regions" {
                        let Json::List(items) = list else {
                            panic!("list")
                        };
                        for item in items {
                            let Json::Obj(f) = item else { panic!("item") };
                            f.retain(|x| x.0 != "static_pogo" && x.0 != "pogo_targets");
                        }
                    }
                }
            };
            strip(&mut report);
            let scene_count = {
                let Json::Obj(top) = &report else {
                    panic!("report")
                };
                let Some((_, Json::List(s))) = top.iter().find(|x| x.0 == "scenes") else {
                    panic!("scenes")
                };
                s.len()
            };
            hk_cook::static_sources::postpack_pogo(&mut report, &source, scene_count).unwrap();
            let pick = |report: &Json, list: &str, key: &str| -> Vec<Option<Json>> {
                let Json::Obj(top) = report else {
                    panic!("report")
                };
                let Some((_, Json::List(items))) = top.iter().find(|x| x.0 == list) else {
                    panic!("list")
                };
                items
                    .iter()
                    .map(|i| {
                        if let Json::Obj(f) = i {
                            f.iter().find(|x| x.0 == key).map(|x| x.1.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            let mut bad = 0;
            for (list, key) in [("scenes", "static_pogo"), ("regions", "pogo_targets")] {
                let (mine, want) = (pick(&report, list, key), pick(&original, list, key));
                bad += mine.iter().zip(&want).filter(|(a, b)| a != b).count();
                if let Some(i) = mine.iter().zip(&want).position(|(a, b)| a != b) {
                    println!(
                        "first difference at {list}[{i}]:\n rust:   {}\n python: {}",
                        mine[i]
                            .as_ref()
                            .map(hk_cook::pyjson::dumps_sorted_compact)
                            .unwrap_or_default(),
                        want[i]
                            .as_ref()
                            .map(hk_cook::pyjson::dumps_sorted_compact)
                            .unwrap_or_default()
                    );
                }
                println!(
                    "{list}.{key}: {} entries, {} differ",
                    want.len(),
                    mine.iter().zip(&want).filter(|(a, b)| a != b).count()
                );
            }
            println!("{bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "bounded" => {
            // hk-cook-parity bounded <source dir> <oracle dir> <regions.json>: actor_sources(sc, bounds)
            // over every region's interaction bounds.
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions {
                f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone())
            } else {
                None
            }) else {
                panic!("no scenes")
            };
            let get = |j: &Json, k: &str| -> Option<Json> {
                if let Json::Obj(f) = j {
                    f.iter().find(|x| x.0 == k).map(|x| x.1.clone())
                } else {
                    None
                }
            };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let (Some(Json::Str(file)), Some(Json::Str(name)), Some(Json::List(b))) = (
                        get(s, "file"),
                        get(s, "scene_name"),
                        get(s, "runtime_bounds"),
                    ) else {
                        panic!("scene row")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let (mut regions_checked, mut actors_checked, mut bad) = (0, 0, 0);
            for s in &scenes {
                let (Some(Json::Str(name)), Some(Json::Str(file))) =
                    (get(s, "scene_name"), get(s, "file"))
                else {
                    panic!("scene row")
                };
                let Json::List(oracle) =
                    parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap())
                        .unwrap()
                else {
                    panic!("oracle")
                };
                let sc = Scene::new(&source, &file).unwrap();
                for region in &oracle {
                    let Some(Json::List(b)) = get(region, "bounds") else {
                        panic!("bounds")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    let rows = hk_cook::actors::scan_in(
                        &sc,
                        &source,
                        &catalogue,
                        [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])],
                    )
                    .unwrap();
                    let mine = Json::List(
                        rows.iter()
                            .map(|r| {
                                Json::List(vec![
                                    Json::Str(r.source.clone()),
                                    Json::Bool(r.supported),
                                    r.control
                                        .as_ref()
                                        .map_or(Json::Null, |c| Json::Str(c.0.clone())),
                                ])
                            })
                            .collect(),
                    );
                    regions_checked += 1;
                    actors_checked += rows.len();
                    if get(region, "actors") != Some(mine) {
                        bad += 1;
                        println!("{name} chunk {:?}: actors differ", get(region, "chunk_id"));
                    }
                }
            }
            println!(
                "checked {regions_checked} regions, {actors_checked} actors, {bad} mismatches"
            );
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "atlas" => {
            // hk-cook-parity atlas <oracle-atlas.json>: dense_pack, alpha covers, canonical_black and Atlas.pack.
            use hk_cook::atlas::{canonical_black, Atlas};
            use hk_cook::pyjson::{parse, Json};
            use hk_cook::{alpha_cover, packer};
            let oracle = parse(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let int = |j: &Json| -> i64 {
                if let Json::Int(i) = j {
                    *i
                } else {
                    panic!("int")
                }
            };
            let hex = |j: &Json| -> Vec<u8> {
                let Json::Str(s) = j else { panic!("hex") };
                (0..s.len() / 2)
                    .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
                    .collect()
            };
            let tohex = |b: &[u8]| Json::Str(b.iter().map(|x| format!("{x:02x}")).collect());
            let mut bad = 0;
            let mut counts = std::collections::BTreeMap::<&str, usize>::new();
            for case in list(field(&oracle, "dense_pack")) {
                let rects: Vec<(i64, i64, usize)> = list(field(&case, "rects"))
                    .iter()
                    .map(|r| {
                        let v = list(r.clone());
                        (int(&v[0]), int(&v[1]), int(&v[2]) as usize)
                    })
                    .collect();
                match (packer::dense_pack(&rects), field(&case, "error")) {
                    (Ok((pages, placements)), Json::Null) => {
                        let mine = Json::List(
                            placements
                                .iter()
                                .map(|p| {
                                    Json::List(vec![
                                        Json::Int(p.0 as i64),
                                        Json::Int(p.1 as i64),
                                        Json::Int(p.2),
                                        Json::Int(p.3),
                                        Json::Int(p.4),
                                        Json::Int(p.5),
                                    ])
                                })
                                .collect(),
                        );
                        *counts.entry("dense_pack").or_default() += 1;
                        if int(&field(&case, "pages")) != pages as i64
                            || mine != field(&case, "placements")
                        {
                            bad += 1;
                            println!("dense_pack differs");
                        }
                    }
                    (Err(_), Json::Str(_)) => {
                        *counts.entry("dense_pack refusals").or_default() += 1
                    }
                    (r, _) => {
                        bad += 1;
                        println!("dense_pack error disagreement (rust ok {})", r.is_ok());
                    }
                }
            }
            for case in list(field(&oracle, "covers")) {
                let (w, h) = (
                    int(&field(&case, "w")) as usize,
                    int(&field(&case, "h")) as usize,
                );
                match (
                    alpha_cover::compute_record(
                        w,
                        h,
                        &hex(&field(&case, "palette")),
                        &hex(&field(&case, "pixels")),
                    ),
                    field(&case, "error"),
                ) {
                    (Ok(rec), Json::Null) => {
                        *counts.entry("covers").or_default() += 1;
                        if tohex(&rec) != field(&case, "record") {
                            bad += 1;
                            println!("cover {w}x{h} differs");
                        }
                    }
                    (Err(_), Json::Str(_)) => *counts.entry("cover refusals").or_default() += 1,
                    (r, _) => {
                        bad += 1;
                        println!("cover error disagreement (rust ok {})", r.is_ok());
                    }
                }
            }
            for case in list(field(&oracle, "canonical_black")) {
                let (p, x) = canonical_black(
                    &hex(&field(&case, "palette")),
                    &hex(&field(&case, "pixels")),
                );
                *counts.entry("canonical_black").or_default() += 1;
                if tohex(&p) != field(&case, "out_palette")
                    || tohex(&x) != field(&case, "out_pixels")
                {
                    bad += 1;
                    println!("canonical_black differs");
                }
            }
            for (n, case) in list(field(&oracle, "atlas")).iter().enumerate() {
                let params = field(case, "params");
                let max_cluts = match field(&params, "max_cluts") {
                    Json::Int(i) => Some(i as usize),
                    _ => None,
                };
                let b = |k: &str| matches!(field(&params, k), Json::Bool(true));
                let mut atlas = Atlas::new(
                    b("deduplicate"),
                    int(&field(&params, "max_pages")) as usize,
                    int(&field(&params, "max_textures")) as usize,
                    b("alpha_covers"),
                    max_cluts,
                );
                for op in list(field(case, "ops")) {
                    let r = atlas.add_quantized(
                        int(&field(&op, "w")) as usize,
                        int(&field(&op, "h")) as usize,
                        &hex(&field(&op, "palette")),
                        &hex(&field(&op, "pixels")),
                        matches!(field(&op, "streamed"), Json::Bool(true)),
                        matches!(field(&op, "unique"), Json::Bool(true)),
                    );
                    match (r, field(&op, "index")) {
                        (Ok(i), Json::Int(want)) if i as i64 == want => {}
                        (Err(_), Json::Null) => {}
                        _ => {
                            bad += 1;
                            println!("atlas {n}: add_quantized differs");
                        }
                    }
                }
                if let Json::Obj(grids) = field(case, "grids") {
                    for (k, v) in grids {
                        let v = list(v);
                        atlas.grids.insert(
                            k.parse().unwrap(),
                            (int(&v[0]) as usize, int(&v[1]) as usize),
                        );
                    }
                }
                let rm = Json::List(
                    atlas
                        .request_map
                        .iter()
                        .map(|&i| Json::Int(i as i64))
                        .collect(),
                );
                let qz = Json::List(
                    atlas
                        .quantized
                        .iter()
                        .map(|q| {
                            Json::List(vec![
                                Json::Int(q.0 as i64),
                                Json::Int(q.1 as i64),
                                tohex(&q.2),
                                tohex(&q.3),
                            ])
                        })
                        .collect(),
                );
                if rm != field(case, "request_map") || qz != field(case, "quantized") {
                    bad += 1;
                    println!("atlas {n}: canonical textures differ");
                }
                match (atlas.pack(), field(case, "error")) {
                    (Ok(()), Json::Null) => {
                        *counts.entry("atlas packs").or_default() += 1;
                        let entries = Json::List(
                            atlas
                                .entries
                                .iter()
                                .map(|e| {
                                    Json::List(e.unwrap().iter().map(|&v| Json::Int(v)).collect())
                                })
                                .collect(),
                        );
                        let palettes =
                            Json::List(atlas.palettes.iter().map(|p| tohex(p)).collect());
                        let sha = |d: &[u8]| {
                            use sha2::{Digest, Sha256};
                            Json::Str(
                                Sha256::digest(d)
                                    .iter()
                                    .map(|x| format!("{x:02x}"))
                                    .collect(),
                            )
                        };
                        let pages = Json::List(atlas.pages.iter().map(|p| sha(p)).collect());
                        let ok = entries == field(case, "entries")
                            && palettes == field(case, "palettes")
                            && pages == field(case, "pages")
                            && sha(&atlas.stream) == field(case, "stream")
                            && int(&field(case, "stream_len")) as usize == atlas.stream.len()
                            && int(&field(case, "cluts")) as usize == atlas.cluts
                            && int(&field(case, "animation_bytes")) as usize
                                == atlas.animation_bytes
                            && int(&field(case, "alpha_cover_bytes")) as usize
                                == atlas.alpha_cover_bytes;
                        if !ok {
                            bad += 1;
                            println!("atlas {n}: pack differs");
                        }
                    }
                    (Err(e), Json::Str(want)) => {
                        *counts.entry("atlas refusals").or_default() += 1;
                        if e != want {
                            bad += 1;
                            println!("atlas {n}: refusal {e:?}, python {want:?}");
                        }
                    }
                    (r, want) => {
                        bad += 1;
                        println!("atlas {n}: rust ok {}, python error {want:?}", r.is_ok());
                    }
                }
            }
            println!("checked {counts:?}, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "rng" => {
            // hk-cook-parity rng <oracle-rng.json>: numpy's default_rng stream and choice().
            use hk_cook::numpy_rng::Pcg64;
            use hk_cook::pyjson::{parse, Json};
            let Json::List(cases) = parse(&std::fs::read_to_string(&args[2]).unwrap()).unwrap()
            else {
                panic!("cases")
            };
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let (mut ok, mut bad) = (0, 0);
            for case in &cases {
                let Json::Str(seed) = field(case, "seed") else {
                    panic!("seed")
                };
                let seed: u64 = seed.parse().unwrap();
                let mut rng = Pcg64::new(seed);
                let randoms = Json::List((0..6).map(|_| Json::Float(rng.random())).collect());
                if randoms == field(case, "randoms") {
                    ok += 1
                } else {
                    bad += 1;
                    println!("seed {seed}: random() differs");
                }
                let mut rng = Pcg64::new(seed);
                let probs = list(field(case, "probs"));
                let picks = list(field(case, "picks"));
                for (p, want) in probs.iter().zip(&picks) {
                    let p: Vec<f64> = list(p.clone())
                        .iter()
                        .map(|v| {
                            if let Json::Float(f) = v {
                                *f
                            } else {
                                panic!("p")
                            }
                        })
                        .collect();
                    let mine =
                        Json::List((0..5).map(|_| Json::Int(rng.choice_p(&p) as i64)).collect());
                    if &mine == want {
                        ok += 1
                    } else {
                        bad += 1;
                        println!("seed {seed}: choice(p) over {} differs", p.len());
                    }
                }
                let Json::Obj(wor) = field(case, "wor") else {
                    panic!("wor")
                };
                for (key, want) in wor {
                    let (n, size) = key.split_once(',').unwrap();
                    let mut rng = Pcg64::new(seed);
                    let r =
                        rng.choice_without_replacement(n.parse().unwrap(), size.parse().unwrap());
                    let mut mine: Vec<Json> = r.iter().take(40).map(|&v| Json::Int(v)).collect();
                    mine.push(Json::Int(r.iter().sum()));
                    mine.push(Json::Int(*r.last().unwrap()));
                    if Json::List(mine) == want {
                        ok += 1
                    } else {
                        bad += 1;
                        println!("seed {seed}: choice({key}, replace=False) differs");
                    }
                }
            }
            println!("checked {ok} streams, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "numeric" => {
            use hk_cook::numpy_math as nm;
            use hk_cook::pyjson::{parse, Json};
            let oracle = parse(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let f = |j: &Json| -> f64 {
                match j {
                    Json::Float(x) => *x,
                    Json::Int(i) => *i as f64,
                    _ => panic!("num"),
                }
            };
            let fl = |j: Json| -> Vec<f64> { list(j).iter().map(f).collect() };
            let rows3 = |j: Json| -> Vec<[f64; 3]> {
                list(j)
                    .iter()
                    .map(|r| {
                        let v = fl(r.clone());
                        [v[0], v[1], v[2]]
                    })
                    .collect()
            };
            let luma = [0.299, 0.587, 0.114];
            let report = |name: &str, hits: usize, total: usize| println!("{name}: {hits}/{total}");
            let (mut h, mut t) = (0, 0);
            for c in list(field(&oracle, "sum1d")) {
                t += 1;
                if nm::sum(&fl(field(&c, "x"))) == f(&field(&c, "sum")) {
                    h += 1
                }
            }
            report("np.sum 1-D (pairwise)", h, t);
            let (mut h, mut t) = (0, 0);
            for c in list(field(&oracle, "rowsum")) {
                let x = rows3(field(&c, "x"));
                for (r, w) in x.iter().zip(fl(field(&c, "sum"))) {
                    t += 1;
                    if nm::row_sum3([r[0] * r[0], r[1] * r[1], r[2] * r[2]]) == w {
                        h += 1
                    }
                }
            }
            report("(x*x).sum(1)", h, t);
            let (mut h, mut t) = (0, 0);
            for c in list(field(&oracle, "colsum")) {
                let x = rows3(field(&c, "x"));
                let want = fl(field(&c, "sum"));
                for k in 0..3 {
                    t += 1;
                    let mut acc = x[0][k];
                    for r in &x[1..] {
                        acc += r[k];
                    }
                    if acc == want[k] {
                        h += 1
                    }
                }
            }
            report("x.sum(0) sequential", h, t);
            let (mut h, mut t) = (0, 0);
            for c in list(field(&oracle, "average")) {
                let x = rows3(field(&c, "x"));
                let w = fl(field(&c, "w"));
                let want = fl(field(&c, "avg"));
                let got = nm::average3(&x, &w);
                for k in 0..3 {
                    t += 1;
                    if got[k] == want[k] {
                        h += 1
                    }
                }
            }
            report("np.average(axis 0, weights)", h, t);
            for (name, g) in [
                (
                    "x @ LUMA plain",
                    nm::dot3_plain as fn([f64; 3], [f64; 3]) -> f64,
                ),
                ("x @ LUMA fma chain", nm::dot3),
            ] {
                let (mut h, mut t) = (0, 0);
                for c in list(field(&oracle, "luma")) {
                    let x = rows3(field(&c, "x"));
                    for (r, w) in x.iter().zip(fl(field(&c, "dot"))) {
                        t += 1;
                        if g(*r, luma) == w {
                            h += 1
                        }
                    }
                }
                let gray = field(&oracle, "gray");
                let x = rows3(field(&gray, "x"));
                let (mut gh, mut gt) = (0, 0);
                for (r, w) in x.iter().zip(fl(field(&gray, "dot"))) {
                    gt += 1;
                    if g(*r, luma) == w {
                        gh += 1
                    }
                }
                report(name, h, t);
                report(&format!("{name} (gray ramp)"), gh, gt);
            }
            let (mut h, mut t) = (0, 0);
            let mut by_k = std::collections::BTreeMap::<usize, (usize, usize)>::new();
            for c in list(field(&oracle, "dot")) {
                let x = rows3(field(&c, "x"));
                let cc = rows3(field(&c, "c"));
                let r = list(field(&c, "r"));
                for (i, row) in x.iter().enumerate() {
                    let want = fl(r[i].clone());
                    for (j, cj) in cc.iter().enumerate() {
                        t += 1;
                        let e = by_k.entry(cc.len()).or_default();
                        e.1 += 1;
                        if nm::matmul_entry(
                            [2.0 * row[0], 2.0 * row[1], 2.0 * row[2]],
                            *cj,
                            cc.len(),
                            j,
                        ) == want[j]
                        {
                            h += 1;
                            e.0 += 1
                        }
                    }
                }
            }
            report("(2x) @ c.T by observed rule", h, t);
            println!("by k: {by_k:?}");
            let (mut h, mut t) = (0, 0);
            for c in list(field(&oracle, "dot")) {
                let x = rows3(field(&c, "x"));
                let cc = rows3(field(&c, "c"));
                let r = list(field(&c, "r"));
                let doubled: Vec<[f64; 3]> = x
                    .iter()
                    .map(|v| [2.0 * v[0], 2.0 * v[1], 2.0 * v[2]])
                    .collect();
                let got = hk_cook::blas::matmul_abt(&doubled, &cc);
                for i in 0..x.len() {
                    let want = fl(r[i].clone());
                    for j in 0..cc.len() {
                        t += 1;
                        if got[i * cc.len() + j] == want[j] {
                            h += 1
                        }
                    }
                }
            }
            report("(2x) @ c.T via cblas_dgemm", h, t);
            let (mut h, mut t) = (0, 0);
            for c in list(field(&oracle, "luma")) {
                let x = rows3(field(&c, "x"));
                let want = fl(field(&c, "dot"));
                let got = hk_cook::blas::matvec(&x, luma);
                for (g, w) in got.iter().zip(&want) {
                    t += 1;
                    if g == w {
                        h += 1
                    }
                }
            }
            report("x @ LUMA via cblas_dgemv", h, t);
        }
        "quant" => {
            // hk-cook-parity quant <oracle-quant dir>: host/quantize.py over real actor sprites.
            use hk_cook::pyjson::{parse, Json};
            use hk_pil::{Image, Mode};
            use sha2::{Digest, Sha256};
            let dir = &args[2];
            let index =
                parse(&std::fs::read_to_string(format!("{dir}/index.json")).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Option<Json> {
                if let Json::Obj(f) = j {
                    f.iter().find(|x| x.0 == k).map(|x| x.1.clone())
                } else {
                    None
                }
            };
            let Some(Json::List(cases)) = field(&index, "cases") else {
                panic!("cases")
            };
            let int = |j: Option<Json>| -> usize {
                if let Some(Json::Int(i)) = j {
                    i as usize
                } else {
                    panic!("int")
                }
            };
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let (mut ok, mut fallback, mut skipped, mut bad) = (0, 0, 0, 0);
            for case in &cases {
                if field(case, "error").is_some() {
                    skipped += 1;
                    continue;
                }
                let idx = int(field(case, "idx"));
                let (iw, ih, w, h) = (
                    int(field(case, "iw")),
                    int(field(case, "ih")),
                    int(field(case, "w")),
                    int(field(case, "h")),
                );
                let data = std::fs::read(format!("{dir}/{idx}.rgba")).unwrap();
                let image = Image {
                    mode: Mode::Rgba,
                    width: iw,
                    height: ih,
                    data,
                };
                let marker =
                    |_: &Image, _: usize, _: usize| -> Result<hk_cook::atlas::Quantized, String> {
                        Err("fallback".to_string())
                    };
                let got = hk_cook::quantize::quantize(&image, w, h, Some(&marker));
                match (got, field(case, "fallback")) {
                    (Err(e), Some(Json::Bool(true))) if e == "fallback" => fallback += 1,
                    (Ok(q), Some(Json::Bool(false))) => {
                        let same = Json::Str(hexs(&q.palette)) == field(case, "palette").unwrap()
                            && Json::Str(hexs(&Sha256::digest(&q.plane)))
                                == field(case, "plane").unwrap()
                            && Json::Str(hexs(&Sha256::digest(&q.image.data)))
                                == field(case, "image").unwrap();
                        if same {
                            ok += 1
                        } else {
                            bad += 1;
                            println!("case {idx} ({iw}x{ih} -> {w}x{h}): output differs");
                        }
                    }
                    (r, _) => {
                        bad += 1;
                        println!("case {idx}: rust {:?}", r.map(|_| "ok"));
                    }
                }
            }
            println!("checked {ok} quantizations, {fallback} fallbacks, {skipped} skipped, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "octree" => {
            // hk-cook-parity octree <oracle-octree dir>: Pillow's FASTOCTREE through
            // Atlas._quantize_octree over real sprite masks, and the raw quantiser over random images.
            use hk_cook::pyjson::{parse, Json};
            use hk_pil::{Image, Mode};
            use sha2::{Digest, Sha256};
            let dir = &args[2];
            let index =
                parse(&std::fs::read_to_string(format!("{dir}/index.json")).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let int = |j: Json| -> usize {
                if let Json::Int(i) = j {
                    i as usize
                } else {
                    panic!("int")
                }
            };
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let (mut ok, mut bad) = (0, 0);
            for case in list(field(&index, "cases")) {
                let idx = int(field(&case, "idx"));
                let (iw, ih, w, h) = (
                    int(field(&case, "iw")),
                    int(field(&case, "ih")),
                    int(field(&case, "w")),
                    int(field(&case, "h")),
                );
                let image = Image {
                    mode: Mode::Rgba,
                    width: iw,
                    height: ih,
                    data: std::fs::read(format!("{dir}/{idx}.rgba")).unwrap(),
                };
                let q = hk_cook::quantize::octree_fallback(&image, w, h).unwrap();
                let same = Json::Str(hexs(&q.palette)) == field(&case, "palette")
                    && Json::Str(hexs(&Sha256::digest(&q.plane))) == field(&case, "plane")
                    && Json::Str(hexs(&Sha256::digest(&q.image.data))) == field(&case, "image");
                if same {
                    ok += 1
                } else {
                    bad += 1;
                    println!("case {idx} ({iw}x{ih} -> {w}x{h}): output differs");
                }
            }
            let sprites = ok;
            for case in list(field(&index, "raw")) {
                let idx = int(field(&case, "idx"));
                let data = std::fs::read(format!("{dir}/raw{idx}.rgba")).unwrap();
                let pixels: Vec<[u8; 4]> = data
                    .chunks_exact(4)
                    .map(|p| [p[0], p[1], p[2], p[3]])
                    .collect();
                let (palette, indices) = hk_pil::octree::fast_octree(&pixels, 15);
                let palette: Vec<u8> = palette.iter().flatten().copied().collect();
                if Json::Str(hexs(&palette)) == field(&case, "palette")
                    && Json::Str(hexs(&indices)) == field(&case, "indices")
                {
                    ok += 1
                } else {
                    bad += 1;
                    println!("raw case {idx}: output differs");
                }
            }
            println!(
                "checked {sprites} sprite masks and {} random images, {bad} mismatches",
                ok - sprites
            );
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "scenery" => {
            // hk-cook-parity scenery <data dir> <oracle-scenery.json>: cook.py's scenery layer
            // (dimensions, draw ranges, edges, black members, remaskers, texels, decor and wall draws).
            use hk_cook::atlas::{Atlas, MAX_ROOM_TEXTURES};
            use hk_cook::pyjson::{parse, Json};
            use hk_cook::scenery::*;
            use hk_pil::{Image, Mode};
            use hk_unity::scene::Scene;
            use sha2::{Digest, Sha256};
            use std::collections::HashMap;
            let source = lazy_source();
            let oracle = parse(&std::fs::read_to_string(&args[3]).unwrap()).unwrap();
            fn norm(j: Json) -> Json {
                match j {
                    Json::Obj(mut f) => {
                        for e in &mut f {
                            e.1 = norm(std::mem::replace(&mut e.1, Json::Null));
                        }
                        f.sort_by(|a, b| a.0.cmp(&b.0));
                        Json::Obj(f)
                    }
                    Json::List(l) => Json::List(l.into_iter().map(norm).collect()),
                    other => other,
                }
            }
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let num = |j: &Json| -> f64 {
                match j {
                    Json::Float(f) => *f,
                    Json::Int(i) => *i as f64,
                    other => panic!("number {other:?}"),
                }
            };
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let sha = |b: &[u8]| Json::Str(hexs(&Sha256::digest(b)));
            let (mut ok, mut bad) = (0usize, 0usize);
            let mut check = |name: &str, same: bool| {
                if same {
                    ok += 1
                } else {
                    bad += 1;
                    println!("{name}: differs");
                }
            };
            let pure = field(&oracle, "pure");
            for (n, c) in list(field(&pure, "dimensions")).iter().enumerate() {
                let Json::Str(w) = field(c, "w") else {
                    panic!("w")
                };
                let got = scenery_dimensions(
                    w.parse::<f64>().unwrap(),
                    num(&field(c, "h")),
                    num(&field(c, "cap")) as i64,
                );
                let want = field(c, "r");
                let same = match (&got, &want) {
                    (Ok(g), Json::List(l)) => {
                        l.iter().map(|v| num(v) as usize).collect::<Vec<_>>() == vec![g.0, g.1]
                    }
                    (Err(_), Json::Null) => true,
                    _ => false,
                };
                check(&format!("dimensions {n}"), same);
            }
            for (n, c) in list(field(&pure, "range")).iter().enumerate() {
                let points: Vec<[f64; 3]> = list(field(c, "points"))
                    .iter()
                    .map(|p| {
                        let v = list(p.clone());
                        [num(&v[0]), num(&v[1]), num(&v[2])]
                    })
                    .collect();
                let got = native_draw_range(num(&field(c, "scale")), &points);
                let want = match field(c, "r") {
                    Json::Str(s) => Some(s),
                    _ => None,
                };
                check(&format!("range {n}"), got == want);
            }
            for (n, c) in list(field(&pure, "edge")).iter().enumerate() {
                let two = |j: Json| -> [f64; 2] {
                    let v = list(j);
                    [num(&v[0]), num(&v[1])]
                };
                let region = field(c, "region");
                let b = list(field(&region, "collision_bounds"));
                let slopes = matches!(
                    region.clone(),
                    Json::Obj(ref f) if f.iter().any(|e| e.0 == "allow_slopes")
                );
                let got = cooked_edge(
                    two(field(c, "a")),
                    two(field(c, "b")),
                    [num(&b[0]), num(&b[1]), num(&b[2]), num(&b[3])],
                    slopes,
                );
                let same = match (got, field(c, "seg"), field(c, "why")) {
                    (Ok((a, b)), Json::List(s), Json::Null) => {
                        two(s[0].clone()) == a && two(s[1].clone()) == b
                    }
                    (Err(why), Json::Null, Json::Str(w)) => why == w,
                    _ => false,
                };
                check(&format!("edge {n}"), same);
            }
            for (n, c) in list(field(&pure, "black")).iter().enumerate() {
                let Json::Str(px) = field(c, "px") else {
                    panic!("px")
                };
                let data: Vec<u8> = (0..px.len() / 2)
                    .map(|i| u8::from_str_radix(&px[i * 2..i * 2 + 2], 16).unwrap())
                    .collect();
                let image = Image {
                    mode: Mode::Rgba,
                    width: num(&field(c, "w")) as usize,
                    height: num(&field(c, "h")) as usize,
                    data,
                };
                let color = field(c, "color");
                let got = black_member_image(
                    &image,
                    [
                        num(&field(&color, "r")),
                        num(&field(&color, "g")),
                        num(&field(&color, "b")),
                    ],
                );
                let same = match (got, field(c, "r")) {
                    (Some(g), Json::Str(h)) => hexs(&Sha256::digest(&g.data)) == h,
                    (None, Json::Null) => true,
                    _ => false,
                };
                check(&format!("black {n}"), same);
            }
            for c in list(field(&pure, "behavior")) {
                let v = list(c);
                let types: Vec<String> = list(v[2].clone())
                    .into_iter()
                    .map(|t| {
                        if let Json::Str(s) = t {
                            s
                        } else {
                            panic!("type")
                        }
                    })
                    .collect();
                let (Json::Str(a), Json::Str(b)) = (&v[0], &v[1]) else {
                    panic!("names")
                };
                let got = unsupported_sprite_behavior(a, b, &types);
                let same = match (&v[3], got) {
                    (Json::Str(w), Some(g)) => w == g,
                    (Json::Null, None) => true,
                    _ => false,
                };
                check("behavior", same);
            }
            let draw_json = |d: &Draw| -> Json {
                Json::Obj(vec![
                    ("source".into(), Json::Str(d.source.clone())),
                    ("sprite".into(), Json::Str(d.sprite.clone())),
                    ("name".into(), Json::Str(d.name.clone())),
                    ("texture".into(), Json::Int(d.texture as i64)),
                    (
                        "points".into(),
                        Json::List(
                            d.points
                                .iter()
                                .map(|p| Json::List(p.iter().map(|&v| Json::Float(v)).collect()))
                                .collect(),
                        ),
                    ),
                    ("scale".into(), Json::Float(d.scale)),
                    (
                        "tint".into(),
                        Json::List(d.tint.iter().map(|&v| Json::Int(v)).collect()),
                    ),
                    ("z".into(), Json::Float(d.z)),
                    ("order".into(), Json::Int(d.order)),
                    ("layer".into(), Json::Int(d.layer)),
                ])
            };
            for scene in list(field(&oracle, "scenes")) {
                let Json::Str(file) = field(&scene, "file") else {
                    panic!("file")
                };
                let Json::Str(name) = field(&scene, "name") else {
                    panic!("name")
                };
                let cap = scenery_cap(&name);
                assert_eq!(cap, num(&field(&scene, "cap")) as i64, "cap of {name}");
                let sc = Scene::new(&source, &file).unwrap();
                let tag = |what: &str| format!("{file} {what}");
                // texels
                let mut shared = HashMap::new();
                let texels = scene_sprite_texels(&source, &sc, cap, &mut shared).unwrap();
                let got = Json::List(
                    texels
                        .iter()
                        .map(|((sid, a), (w, h))| {
                            Json::List(vec![
                                Json::Str(sid.clone()),
                                Json::Float(f64::from_bits(*a)),
                                Json::Int(*w as i64),
                                Json::Int(*h as i64),
                            ])
                        })
                        .collect(),
                );
                check(&tag("texels"), got == field(&scene, "texels"));
                // remaskers and self-disabling effects, by object id
                let mut want: Vec<(i64, Json)> = list(field(&scene, "remasker"))
                    .into_iter()
                    .map(|p| {
                        let v = list(p);
                        (num(&v[0]) as i64, norm(v[1].clone()))
                    })
                    .collect();
                want.sort_by_key(|p| p.0);
                let mut got: Vec<(i64, Json)> = Vec::new();
                for o in &sc.objects {
                    if sc.gos.contains_key(&o.id)
                        && o.tree.get("m_Name").and_then(|n| n.str()).as_deref()
                            == Some("Inverse Remasker")
                    {
                        match unsupported_remasker(&source, &sc, o.id) {
                            Ok(Some(j)) => got.push((o.id, norm(j))),
                            other => panic!("{file} remasker {}: {:?}", o.id, other.map(|_| ())),
                        }
                    }
                }
                got.sort_by_key(|p| p.0);
                check(&tag("remasker"), got == want);
                let mut want: Vec<i64> = list(field(&scene, "self_disabling"))
                    .iter()
                    .map(|v| num(v) as i64)
                    .collect();
                want.sort_unstable();
                let mut got: Vec<i64> = sc
                    .gos
                    .keys()
                    .copied()
                    .filter(|&g| self_disabling_effect(&sc, g))
                    .collect();
                got.sort_unstable();
                check(&tag("self_disabling"), got == want);
                // decor sources
                let sources = decor_sources(&sc);
                let got = Json::List(
                    sources
                        .iter()
                        .map(|d| {
                            norm(Json::Obj(vec![
                                ("sprite_id".into(), Json::Int(d.sprite_id)),
                                ("gid".into(), Json::Int(d.gid)),
                                ("source".into(), Json::Str(d.renderer_source.clone())),
                                ("family".into(), Json::Str(d.family.to_string())),
                                ("animator".into(), Json::Bool(d.animator.is_some())),
                            ]))
                        })
                        .collect(),
                );
                check(
                    &tag("decor_sources"),
                    got == norm(field(&scene, "decor_sources")),
                );
                // black members
                let mut got = Vec::new();
                for o in &sc.objects {
                    if o.typename != "SpriteRenderer" || got.len() >= 25 {
                        continue;
                    }
                    let t = &o.tree;
                    if t.get("m_Sprite")
                        .and_then(|p| p.get("m_PathID"))
                        .and_then(|v| v.int())
                        .unwrap_or(0)
                        == 0
                    {
                        continue;
                    }
                    let Ok(obj) = sc.deref(t.get("m_Sprite").unwrap()) else {
                        continue;
                    };
                    let Ok((im, _)) = hk_cook::cook::native_sprite(&source, &obj) else {
                        continue;
                    };
                    let c = t.get("m_Color").unwrap();
                    let col = |k: &str| c.get(k).and_then(|v| v.float()).unwrap();
                    let r = black_member_image(&im, [col("r"), col("g"), col("b")]);
                    got.push(norm(Json::Obj(vec![
                        ("renderer".into(), Json::Int(o.id)),
                        ("sprite".into(), Json::Str(obj.sid())),
                        (
                            "color".into(),
                            Json::Obj(vec![
                                ("r".into(), Json::Float(col("r"))),
                                ("g".into(), Json::Float(col("g"))),
                                ("b".into(), Json::Float(col("b"))),
                            ]),
                        ),
                        ("r".into(), r.map_or(Json::Null, |g| sha(&g.data))),
                    ])));
                }
                check(
                    &tag("black"),
                    Json::List(got) == norm(field(&scene, "black")),
                );
                // the black members through a fresh atlas (the octree path of Atlas.add)
                {
                    let mut batlas = Atlas::new(
                        true,
                        STATIC_PAGE_BUDGET,
                        MAX_ROOM_TEXTURES,
                        true,
                        Some(TEXTURE_BUDGET),
                    );
                    let mut adds = Vec::new();
                    for b in list(field(&scene, "black")) {
                        if field(&b, "r") == Json::Null {
                            continue;
                        }
                        let renderer = num(&field(&b, "renderer")) as i64;
                        let t = &sc.object(renderer).unwrap().tree;
                        let obj = sc.deref(t.get("m_Sprite").unwrap()).unwrap();
                        let (im, _) = hk_cook::cook::native_sprite(&source, &obj).unwrap();
                        let c = t.get("m_Color").unwrap();
                        let col = |k: &str| c.get(k).and_then(|v| v.float()).unwrap();
                        let black =
                            black_member_image(&im, [col("r"), col("g"), col("b")]).unwrap();
                        let (w, h) = (im.width.clamp(1, 96), im.height.clamp(1, 96));
                        let index = batlas
                            .add(
                                &black,
                                w as f64,
                                h as f64,
                                false,
                                &hk_cook::quantize::atlas_quantizer,
                            )
                            .unwrap();
                        adds.push(norm(Json::Obj(vec![
                            ("renderer".into(), Json::Int(renderer)),
                            ("w".into(), Json::Int(w as i64)),
                            ("h".into(), Json::Int(h as i64)),
                            ("index".into(), Json::Int(index as i64)),
                        ])));
                    }
                    check(
                        &tag("black_adds"),
                        Json::List(adds) == norm(field(&scene, "black_adds")),
                    );
                    let quantized = Json::List(
                        batlas
                            .quantized
                            .iter()
                            .map(|q| {
                                Json::List(vec![
                                    Json::Int(q.0 as i64),
                                    Json::Int(q.1 as i64),
                                    Json::Str(hexs(&q.2)),
                                    sha(&q.3),
                                ])
                            })
                            .collect(),
                    );
                    let request = Json::List(
                        batlas
                            .request_map
                            .iter()
                            .map(|&i| Json::Int(i as i64))
                            .collect(),
                    );
                    let want = field(&scene, "black_atlas");
                    check(
                        &tag("black_atlas"),
                        quantized == field(&want, "quantized")
                            && request == field(&want, "request_map"),
                    );
                }
                // decor and wall draws share one atlas and its caches
                let (cx, cy) = {
                    let cam = field(&scene, "cull");
                    let v = list(cam);
                    let a = list(v[0].clone());
                    let b = list(v[1].clone());
                    ((num(&a[0]), num(&a[1])), (num(&b[0]), num(&b[1])))
                };
                let mut atlas = Atlas::new(
                    true,
                    STATIC_PAGE_BUDGET,
                    MAX_ROOM_TEXTURES,
                    true,
                    Some(TEXTURE_BUDGET),
                );
                let mut caches = SceneryCaches::default();
                let view = View {
                    cull: Some((cx, cy)),
                    cap,
                };
                let mut got = Vec::new();
                for d in &sources {
                    got.push(
                        match decor_draws(&source, &sc, d, &mut atlas, &mut caches, &view) {
                            Ok(None) => Json::Obj(vec![
                                ("source".into(), Json::Str(d.renderer_source.clone())),
                                ("none".into(), Json::Bool(true)),
                            ]),
                            Ok(Some(c)) => Json::Obj(vec![
                                ("source".into(), Json::Str(c.source.clone())),
                                ("family".into(), Json::Str(c.family.to_string())),
                                ("fps".into(), Json::Float(c.fps)),
                                ("wrap".into(), Json::Int(c.wrap)),
                                ("loop_start".into(), Json::Int(c.loop_start)),
                                (
                                    "sequence".into(),
                                    Json::List(
                                        c.sequence.iter().map(|&v| Json::Int(v as i64)).collect(),
                                    ),
                                ),
                                (
                                    "draws".into(),
                                    Json::List(c.draws.iter().map(&draw_json).collect()),
                                ),
                            ]),
                            Err(_) => Json::Obj(vec![
                                ("source".into(), Json::Str(d.renderer_source.clone())),
                                ("error".into(), Json::Bool(true)),
                            ]),
                        },
                    );
                }
                // the oracle records the Python repr of an error; only that it failed is compared
                let want: Vec<Json> = list(field(&scene, "decor"))
                    .into_iter()
                    .map(|d| match d {
                        Json::Obj(f) if f.iter().any(|e| e.0 == "error") => Json::Obj(vec![
                            (
                                "source".into(),
                                f.iter().find(|e| e.0 == "source").unwrap().1.clone(),
                            ),
                            ("error".into(), Json::Bool(true)),
                        ]),
                        other => other,
                    })
                    .collect();
                if norm(Json::List(got.clone())) != norm(Json::List(want.clone())) {
                    for (g, w) in got.iter().zip(&want) {
                        if norm(g.clone()) != norm(w.clone()) {
                            println!("{file} decor: differs at {:?}", field(w, "source"));
                            break;
                        }
                    }
                }
                check(
                    &tag("decor"),
                    norm(Json::List(got)) == norm(Json::List(want)),
                );
                let mut got = Vec::new();
                for w in list(field(&scene, "walls")) {
                    let Json::Str(src) = field(&w, "source") else {
                        panic!("source")
                    };
                    let culled = field(&w, "cull") == Json::Bool(true);
                    let v = View {
                        cull: culled.then_some((cx, cy)),
                        cap,
                    };
                    got.push(
                        match tk2d_wall_draw(&source, &sc, &src, &mut atlas, &mut caches, &v) {
                            Ok(d) => Json::Obj(vec![
                                ("source".into(), Json::Str(src.clone())),
                                ("cull".into(), Json::Bool(culled)),
                                ("draw".into(), d.as_ref().map_or(Json::Null, &draw_json)),
                            ]),
                            Err(_) => Json::Obj(vec![
                                ("source".into(), Json::Str(src.clone())),
                                ("cull".into(), Json::Bool(culled)),
                                ("error".into(), Json::Bool(true)),
                            ]),
                        },
                    );
                }
                let want: Vec<Json> = list(field(&scene, "walls"))
                    .into_iter()
                    .map(|d| match d {
                        Json::Obj(f) if f.iter().any(|e| e.0 == "error") => Json::Obj(vec![
                            (
                                "source".into(),
                                f.iter().find(|e| e.0 == "source").unwrap().1.clone(),
                            ),
                            (
                                "cull".into(),
                                f.iter().find(|e| e.0 == "cull").unwrap().1.clone(),
                            ),
                            ("error".into(), Json::Bool(true)),
                        ]),
                        other => other,
                    })
                    .collect();
                check(
                    &tag("walls"),
                    norm(Json::List(got)) == norm(Json::List(want)),
                );
                // the atlas those draws filled
                let quantized = Json::List(
                    atlas
                        .quantized
                        .iter()
                        .map(|q| {
                            Json::List(vec![
                                Json::Int(q.0 as i64),
                                Json::Int(q.1 as i64),
                                Json::Str(hexs(&q.2)),
                                sha(&q.3),
                            ])
                        })
                        .collect(),
                );
                let images = Json::List(
                    atlas
                        .images
                        .iter()
                        .map(|i| i.as_ref().map_or(Json::Null, |im| sha(&im.data)))
                        .collect(),
                );
                let request = Json::List(
                    atlas
                        .request_map
                        .iter()
                        .map(|&i| Json::Int(i as i64))
                        .collect(),
                );
                let want = field(&scene, "atlas");
                check(
                    &tag("atlas"),
                    quantized == field(&want, "quantized")
                        && images == field(&want, "images")
                        && request == field(&want, "request_map"),
                );
            }
            println!("checked {ok} results, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "quality" => {
            // hk-cook-parity quality <oracle-quality.json>: host/quality.py's tables, value for value.
            use hk_cook::pyjson::{parse, Json};
            use hk_cook::quality::*;
            let oracle = parse(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
            let want = |k: &str| -> Json {
                if let Json::Obj(f) = &oracle {
                    f.iter().find(|x| x.0 == k).unwrap().1.clone()
                } else {
                    panic!("object")
                }
            };
            let n = |v: &Num| match v {
                Num::I(i) => Json::Int(*i),
                Num::F(f) => Json::Float(*f),
            };
            let b = |v: &Bounds| Json::List(v.iter().map(n).collect());
            let sorted = |j: Json| -> Json {
                // Object keys in key order, so the comparison does not depend on the writer's order.
                fn go(j: Json) -> Json {
                    match j {
                        Json::Obj(mut f) => {
                            f.sort_by(|a, c| a.0.cmp(&c.0));
                            Json::Obj(f.into_iter().map(|(k, v)| (k, go(v))).collect())
                        }
                        Json::List(l) => Json::List(l.into_iter().map(go).collect()),
                        o => o,
                    }
                }
                go(j)
            };
            let mut bad = 0;
            let mut check = |name: &str, got: Json, k: &str| {
                if sorted(got) != sorted(want(k)) {
                    bad += 1;
                    println!("{name} differs");
                }
            };
            check(
                "scene_table",
                Json::List(
                    SCENE_TABLE
                        .iter()
                        .map(|r| {
                            Json::Obj(vec![
                                ("scene_id".into(), Json::Int(r.scene_id as i64)),
                                ("scene_name".into(), Json::Str(r.scene_name.into())),
                                ("file".into(), Json::Str(r.file.into())),
                                ("runtime_bounds".into(), b(&r.runtime_bounds)),
                                ("camera_global_bounds".into(), b(&r.camera_global_bounds)),
                            ])
                        })
                        .collect(),
                ),
                "scene_table",
            );
            check(
                "consts",
                Json::List(
                    [
                        SCENERY_MAX_AXIS,
                        SCENERY_TEXEL_CAP,
                        STATIC_PAGE_BUDGET as i64,
                        TEXTURE_BUDGET as i64,
                        ROOM_BYTE_BUDGET as i64,
                    ]
                    .iter()
                    .map(|&v| Json::Int(v))
                    .collect(),
                ),
                "consts",
            );
            check(
                "caps",
                Json::Obj(
                    SCENERY_SCENE_CAPS
                        .iter()
                        .map(|(k, v)| (k.to_string(), Json::Int(*v)))
                        .collect(),
                ),
                "caps",
            );
            check(
                "region_layout",
                Json::List(
                    REGION_LAYOUT
                        .iter()
                        .map(|(s, v)| Json::List(vec![Json::Int(*s as i64), b(v)]))
                        .collect(),
                ),
                "region_layout",
            );
            check(
                "measured",
                Json::Obj(
                    MEASURED_VIEW_LAYOUTS
                        .iter()
                        .map(|(k, v)| (k.to_string(), Json::List(v.iter().map(b).collect())))
                        .collect(),
                ),
                "measured",
            );
            check(
                "town",
                Json::List(
                    TOWN_EXTENSION_LAYOUT
                        .iter()
                        .map(|(x, c)| Json::List(vec![b(x), b(c)]))
                        .collect(),
                ),
                "town",
            );
            check(
                "grid",
                Json::Obj(
                    grid_scene_layouts()
                        .iter()
                        .map(|(k, v)| (k.to_string(), Json::List(v.iter().map(b).collect())))
                        .collect(),
                ),
                "grid",
            );
            println!("checked the quality tables, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "breakables" => {
            // hk-cook-parity breakables <data dir> <oracle-breakables.json>: host/breakables.py,
            // secret_breaks.py and reveal_masks.py over real scenes, record for record.
            use hk_cook::breakables as bk;
            use hk_cook::pyjson::{parse, Json};
            use hk_cook::{reveal_masks, secret_breaks};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let oracle = parse(&std::fs::read_to_string(&args[3]).unwrap()).unwrap();
            let Json::List(scenes) = oracle else {
                panic!("scenes")
            };
            let only: Vec<&String> = args.iter().skip(4).collect();
            fn first_diff(a: &Json, b: &Json, path: &str) -> Option<String> {
                match (a, b) {
                    (Json::Obj(x), Json::Obj(y)) => {
                        for (i, ((kx, vx), (ky, vy))) in x.iter().zip(y).enumerate() {
                            if kx != ky {
                                return Some(format!("{path}: key #{i} {kx:?} vs {ky:?}"));
                            }
                            if let Some(d) = first_diff(vx, vy, &format!("{path}.{kx}")) {
                                return Some(d);
                            }
                        }
                        (x.len() != y.len())
                            .then(|| format!("{path}: {} keys vs {}", x.len(), y.len()))
                    }
                    (Json::List(x), Json::List(y)) => {
                        for (i, (vx, vy)) in x.iter().zip(y).enumerate() {
                            if let Some(d) = first_diff(vx, vy, &format!("{path}[{i}]")) {
                                return Some(d);
                            }
                        }
                        (x.len() != y.len())
                            .then(|| format!("{path}: {} items vs {}", x.len(), y.len()))
                    }
                    _ => (a != b).then(|| format!("{path}: {a:?} vs {b:?}")),
                }
            }
            let wrap = |r: hk_cook::common::Result<(Vec<Json>, Vec<Json>)>| -> Json {
                match r {
                    Ok((records, errors)) => Json::Obj(vec![
                        ("records".into(), Json::List(records)),
                        ("errors".into(), Json::List(errors)),
                    ]),
                    Err(e) => Json::Obj(vec![("raised".into(), Json::Str(e))]),
                }
            };
            let (mut ok, mut bad) = (0, 0);
            for rec in scenes {
                let Json::Str(file) = hk_cook::music::jget(&rec, "file").cloned().unwrap() else {
                    panic!("file")
                };
                if !only.is_empty() && !only.iter().any(|o| **o == file) {
                    continue;
                }
                let sc = Scene::new(&source, &file).unwrap();
                macro_rules! check {
                    ($what:expr, $got:expr $(,)?) => {{
                        let what: &str = $what;
                        let got: Json = $got;
                        let want = hk_cook::music::jget(&rec, what)
                            .cloned()
                            .unwrap_or(Json::Null);
                        // "raised" cases compare only that the call raised.
                        let raised = |j: &Json| hk_cook::music::jget(j, "raised").is_some();
                        if raised(&got) && raised(&want) {
                            ok += 1;
                        } else {
                            match first_diff(&got, &want, what) {
                                None => ok += 1,
                                Some(d) => {
                                    bad += 1;
                                    println!("{file}: {d}");
                                }
                            }
                        }
                    }};
                }
                macro_rules! family {
                    ($name:expr, $call:expr) => {{
                        let mut errors = Vec::new();
                        let r = $call(&mut errors);
                        check!($name, wrap(r.map(|records| (records, errors))));
                    }};
                }
                family!("battle_gates", |e: &mut Vec<Json>| bk::battle_gates(
                    &sc,
                    Some(e)
                ));
                family!("hidden_walls", |e: &mut Vec<Json>| bk::hidden_walls(
                    &sc,
                    Some(e),
                    true,
                    None
                ));
                family!("hidden_walls_open", |e: &mut Vec<Json>| bk::hidden_walls(
                    &sc,
                    Some(e),
                    false,
                    None
                ));
                family!("cracked_floors", |e: &mut Vec<Json>| bk::cracked_floors(
                    &sc,
                    Some(e),
                    true
                ));
                family!("cracked_floors_open", |e: &mut Vec<Json>| {
                    bk::cracked_floors(&sc, Some(e), false)
                });
                family!("vines", |e: &mut Vec<Json>| bk::infected_vines(
                    &sc,
                    Some(e),
                    true
                ));
                family!("vines_open", |e: &mut Vec<Json>| bk::infected_vines(
                    &sc,
                    Some(e),
                    false
                ));
                family!("breakables", |e: &mut Vec<Json>| bk::breakable_sources(
                    &sc,
                    None,
                    Some(e),
                    true
                ));
                family!(
                    "breakables_open",
                    |e: &mut Vec<Json>| bk::breakable_sources(&sc, None, Some(e), false)
                );
                family!(
                    "secrets",
                    |e: &mut Vec<Json>| secret_breaks::secret_sources(&source, &sc, None, Some(e))
                );
                let pairs = |v: Vec<(i64, Json)>| {
                    Json::List(
                        v.into_iter()
                            .map(|(k, d)| Json::List(vec![Json::Int(k), d]))
                            .collect(),
                    )
                };
                check!("uncover_drivers", pairs(bk::uncover_drivers(&sc).unwrap()));
                match secret_breaks::secret_states(&sc) {
                    Ok(states) => {
                        check!(
                            "secret_states",
                            Json::List(
                                states
                                    .iter()
                                    .map(|(s, v)| {
                                        Json::List(vec![Json::Str(s.clone()), Json::Int(*v)])
                                    })
                                    .collect(),
                            ),
                        );
                        let mut drivers: Vec<(i64, i64)> = secret_breaks::secret_drivers(&sc)
                            .unwrap()
                            .into_iter()
                            .collect();
                        drivers.sort_unstable();
                        let mut want: Vec<(i64, i64)> =
                            match hk_cook::music::jget(&rec, "secret_drivers") {
                                Some(Json::List(l)) => l
                                    .iter()
                                    .map(|p| {
                                        let Json::List(p) = p else { panic!("pair") };
                                        let (Json::Int(a), Json::Int(b)) = (&p[0], &p[1]) else {
                                            panic!("ints")
                                        };
                                        (*a, *b)
                                    })
                                    .collect(),
                                _ => vec![],
                            };
                        want.sort_unstable();
                        let same = drivers == want;
                        if same {
                            ok += 1
                        } else {
                            bad += 1;
                            println!("{file}: secret_drivers differ");
                        }
                    }
                    Err(_) => check!(
                        "secret_states",
                        Json::Obj(vec![("raised".into(), Json::Null)]),
                    ),
                }
                let rm = reveal_masks::reveal_mask_sources(&sc).unwrap();
                check!(
                    "reveal",
                    Json::Obj(vec![
                        ("controllers".into(), Json::List(rm.controllers)),
                        ("unsupported".into(), Json::List(rm.unsupported)),
                    ]),
                );
            }
            println!("checked {ok} results, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "atlas2" => {
            // hk-cook-parity atlas2 <oracle-atlas2.json> <oracle-quant dir>: Atlas.add, add_tiled,
            // add_frames_shared and pack over real sprite images.
            use hk_cook::atlas::{Atlas, Quantized};
            use hk_cook::pyjson::{parse, Json};
            use hk_pil::{Image, Mode};
            use sha2::{Digest, Sha256};
            let trials = parse(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
            let qdir = &args[3];
            let index =
                parse(&std::fs::read_to_string(format!("{qdir}/index.json")).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let int = |j: &Json| -> i64 {
                if let Json::Int(i) = j {
                    *i
                } else {
                    panic!("int")
                }
            };
            let flt = |j: &Json| -> f64 {
                match j {
                    Json::Int(i) => *i as f64,
                    Json::Float(f) => *f,
                    _ => panic!("num"),
                }
            };
            let cases = list(field(&index, "cases"));
            let load = |idx: i64| -> Image {
                let c = cases
                    .iter()
                    .find(|c| matches!(field(c, "idx"), Json::Int(i) if i == idx))
                    .unwrap();
                Image {
                    mode: Mode::Rgba,
                    width: int(&field(c, "iw")) as usize,
                    height: int(&field(c, "ih")) as usize,
                    data: std::fs::read(format!("{qdir}/{idx}.rgba")).unwrap(),
                }
            };
            let sha = |d: &[u8]| {
                Json::Str(
                    Sha256::digest(d)
                        .iter()
                        .map(|x| format!("{x:02x}"))
                        .collect(),
                )
            };
            let quantize = |im: &Image, w: usize, h: usize| -> Result<Quantized, String> {
                let fallback = |_: &Image, _: usize, _: usize| -> Result<Quantized, String> {
                    Err("octree fallback is not ported".into())
                };
                hk_cook::quantize::quantize(im, w, h, Some(&fallback))
            };
            let (mut ok, mut bad) = (0, 0);
            for (n, trial) in list(trials).iter().enumerate() {
                let mut atlas = Atlas::new(true, 20, 640, true, None);
                let mut firsts = Vec::new();
                let mut failure = None;
                for op in list(field(trial, "ops")) {
                    let Json::Str(kind) = field(&op, "kind") else {
                        panic!("kind")
                    };
                    let result = match kind.as_str() {
                        "add" => atlas
                            .add(
                                &load(int(&field(&op, "idx"))),
                                flt(&field(&op, "w")),
                                flt(&field(&op, "h")),
                                matches!(field(&op, "streamed"), Json::Bool(true)),
                                &quantize,
                            )
                            .map(|i| vec![i]),
                        "tiled" => atlas
                            .add_tiled(
                                &load(int(&field(&op, "idx"))),
                                flt(&field(&op, "w")),
                                flt(&field(&op, "h")),
                                &quantize,
                            )
                            .map(|i| vec![i]),
                        _ => {
                            let items: Vec<(Image, f64, f64)> = list(field(&op, "items"))
                                .iter()
                                .map(|it| {
                                    (
                                        load(int(&field(it, "idx"))),
                                        flt(&field(it, "w")),
                                        flt(&field(it, "h")),
                                    )
                                })
                                .collect();
                            atlas.add_frames_shared(&items, &quantize)
                        }
                    };
                    match result {
                        Ok(r) => firsts.push(r),
                        Err(e) => {
                            failure = Some(e);
                            break;
                        }
                    }
                }
                let want_error = field(trial, "error");
                if failure.is_none() {
                    if let Err(e) = atlas.pack() {
                        failure = Some(e);
                    }
                }
                match (failure, want_error) {
                    (None, Json::Null) => {
                        let want_results: Vec<Vec<i64>> = list(field(trial, "ops"))
                            .iter()
                            .map(|op| match field(op, "result") {
                                Json::List(l) => l.iter().map(int).collect(),
                                v => vec![int(&v)],
                            })
                            .collect();
                        let got_results: Vec<Vec<i64>> = firsts
                            .iter()
                            .map(|f| f.iter().map(|&v| v as i64).collect())
                            .collect();
                        let entries = Json::List(
                            atlas
                                .entries
                                .iter()
                                .map(|e| {
                                    Json::List(e.unwrap().iter().map(|&v| Json::Int(v)).collect())
                                })
                                .collect(),
                        );
                        let palettes = Json::List(
                            atlas
                                .palettes
                                .iter()
                                .map(|p| Json::Str(p.iter().map(|x| format!("{x:02x}")).collect()))
                                .collect(),
                        );
                        let pages = Json::List(atlas.pages.iter().map(|p| sha(p)).collect());
                        let images = Json::List(
                            atlas
                                .images
                                .iter()
                                .map(|i| i.as_ref().map_or(Json::Null, |im| sha(&im.data)))
                                .collect(),
                        );
                        let grids = {
                            let mut g: Vec<_> = atlas.grids.iter().collect();
                            g.sort();
                            Json::Obj(
                                g.into_iter()
                                    .map(|(k, v)| {
                                        (
                                            k.to_string(),
                                            Json::List(vec![
                                                Json::Int(v.0 as i64),
                                                Json::Int(v.1 as i64),
                                            ]),
                                        )
                                    })
                                    .collect(),
                            )
                        };
                        let same_grids = {
                            let Json::Obj(want) = field(trial, "grids") else {
                                panic!("grids")
                            };
                            let Json::Obj(mine) = &grids else {
                                unreachable!()
                            };
                            want.len() == mine.len()
                                && want.iter().all(|w| mine.iter().any(|m| m == w))
                        };
                        let good = want_results.len() == got_results.len()
                            && want_results == got_results
                            && entries == field(trial, "entries")
                            && palettes == field(trial, "palettes")
                            && pages == field(trial, "pages")
                            && sha(&atlas.stream) == field(trial, "stream")
                            && images == field(trial, "images")
                            && same_grids
                            && int(&field(trial, "cluts")) as usize == atlas.cluts;
                        if good {
                            ok += 1
                        } else {
                            bad += 1;
                            println!("trial {n}: atlas differs (results {} entries {} palettes {} pages {} stream {} images {} grids {})", want_results == got_results, entries == field(trial, "entries"), palettes == field(trial, "palettes"), pages == field(trial, "pages"), sha(&atlas.stream) == field(trial, "stream"), images == field(trial, "images"), same_grids);
                        }
                    }
                    (Some(e), Json::Str(want)) if e == want => ok += 1,
                    (f, want) => {
                        bad += 1;
                        println!("trial {n}: rust error {f:?}, python {want:?}");
                    }
                }
            }
            println!("checked {ok} atlases, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "actorart" => {
            // hk-cook-parity actorart <source dir> <oracle dir> <regions.json>: append_actor_art (without
            // the corpse pass) and append_barrel_art over every scene's actors, then Atlas.pack.
            use hk_cook::actor_art::{
                append_actor_art_bodies, append_barrel_art, ArtActor, ArtBank,
            };
            use hk_cook::atlas::{Atlas, Quantized};
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            use sha2::{Digest, Sha256};
            let source = lazy_source();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions {
                f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone())
            } else {
                None
            }) else {
                panic!("no scenes")
            };
            let get = |j: &Json, k: &str| -> Option<Json> {
                if let Json::Obj(f) = j {
                    f.iter().find(|x| x.0 == k).map(|x| x.1.clone())
                } else {
                    None
                }
            };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let (Some(Json::Str(file)), Some(Json::Str(name)), Some(Json::List(b))) = (
                        get(s, "file"),
                        get(s, "scene_name"),
                        get(s, "runtime_bounds"),
                    ) else {
                        panic!("scene row")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let only: Option<String> = args.get(5).cloned();
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let sha = |d: &[u8]| Json::Str(hexs(&Sha256::digest(d)));
            let (mut checked, mut bad) = (0, 0);
            for s in &scenes {
                let (Some(Json::Str(name)), Some(Json::Str(file))) =
                    (get(s, "scene_name"), get(s, "file"))
                else {
                    panic!("scene row")
                };
                if only.as_ref().is_some_and(|o| *o != name) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(format!("{}/{name}.json", args[3])) else {
                    continue;
                };
                let oracle = parse(&text).unwrap();
                if get(&oracle, "error").is_some() {
                    println!("{name}: python failed: {:?}", get(&oracle, "error"));
                    continue;
                }
                let sc = Scene::new(&source, &file).unwrap();
                let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
                let mut actors: Vec<ArtActor> = rows.iter().map(ArtActor::new).collect();
                let mut bank = ArtBank::new(Atlas::new(true, 20, 640, true, None));
                let quantize =
                    |im: &hk_pil::Image, w: usize, h: usize| -> Result<Quantized, String> {
                        let fallback =
                            |_: &hk_pil::Image, _: usize, _: usize| -> Result<Quantized, String> {
                                Err("octree fallback is not ported".into())
                            };
                        hk_cook::quantize::quantize(im, w, h, Some(&fallback))
                    };
                let run = append_actor_art_bodies(&sc, &source, &mut actors, &mut bank, &quantize)
                    .and_then(|_| {
                        hk_cook::effects_art::append_corpse_art(
                            &source,
                            &sc,
                            &mut actors,
                            &mut bank,
                            &quantize,
                        )
                    })
                    .and_then(|_| append_barrel_art(&source, &mut actors, &mut bank, &quantize));
                if let Err(e) = run {
                    bad += 1;
                    println!("{name}: rust failed: {e}");
                    continue;
                }
                let mut mine_actors: Vec<(String, Json)> = Vec::new();
                for a in &actors {
                    let mut clips: Vec<(String, Json)> = a
                        .clips
                        .iter()
                        .map(|c| (c.0.clone(), Json::Int(c.1)))
                        .collect();
                    clips.sort_by(|x, y| x.0.cmp(&y.0));
                    mine_actors.push((
                        a.row.source.clone(),
                        Json::Obj(vec![
                            ("supported".into(), Json::Bool(a.supported)),
                            (
                                "limitations".into(),
                                Json::List(
                                    a.limitations.iter().map(|l| Json::Str(l.clone())).collect(),
                                ),
                            ),
                            ("clips".into(), Json::Obj(clips)),
                            (
                                "visual_scale".into(),
                                a.visual_scale.map_or(Json::Null, |v| {
                                    Json::List(vec![Json::Float(v[0]), Json::Float(v[1])])
                                }),
                            ),
                            ("corpse".into(), a.corpse.clone().unwrap_or(Json::Null)),
                        ]),
                    ));
                }
                let want_actors = get(&oracle, "actors").unwrap();
                let Json::Obj(want_list) = &want_actors else {
                    panic!("actors")
                };
                let sorted_clips = |j: &Json| -> Json {
                    let mut j = j.clone();
                    if let Json::Obj(f) = &mut j {
                        for x in f.iter_mut() {
                            if x.0 == "clips" {
                                if let Json::Obj(c) = &mut x.1 {
                                    c.sort_by(|a, b| a.0.cmp(&b.0));
                                }
                            }
                        }
                    }
                    j
                };
                let actors_ok = want_list.len() == mine_actors.len()
                    && mine_actors.iter().all(|(k, v)| {
                        want_list
                            .iter()
                            .find(|w| w.0 == *k)
                            .is_some_and(|w| sorted_clips(&w.1) == *v)
                    });
                if !actors_ok {
                    bad += 1;
                    println!("{name}: actor records differ");
                }
                let frames = Json::List(
                    bank.frames
                        .iter()
                        .map(|f| {
                            Json::Obj(vec![
                                ("texture".into(), Json::Int(f.texture as i64)),
                                (
                                    "box".into(),
                                    Json::List(f.box_.iter().map(|&v| Json::Float(v)).collect()),
                                ),
                                ("sprite".into(), Json::Str(f.sprite.clone())),
                                ("event".into(), hk_cook::music::value_json(&f.event)),
                            ])
                        })
                        .collect(),
                );
                if frames != get(&oracle, "frames").unwrap() {
                    bad += 1;
                    println!("{name}: frames differ");
                }
                let clips = Json::List(
                    bank.clips
                        .iter()
                        .map(|c| {
                            Json::Obj(vec![
                                ("name".into(), Json::Str(c.name.clone())),
                                ("start".into(), Json::Int(c.start as i64)),
                                ("count".into(), Json::Int(c.count as i64)),
                                ("fps".into(), Json::Float(c.fps)),
                                ("wrap".into(), Json::Int(c.wrap)),
                                ("loopStart".into(), Json::Int(c.loop_start)),
                            ])
                        })
                        .collect(),
                );
                if clips != get(&oracle, "clips").unwrap() {
                    bad += 1;
                    println!("{name}: clips differ");
                }
                let quantized = Json::List(
                    bank.atlas
                        .quantized
                        .iter()
                        .map(|q| {
                            Json::List(vec![
                                Json::Int(q.0 as i64),
                                Json::Int(q.1 as i64),
                                Json::Str(hexs(&q.2)),
                                sha(&q.3),
                            ])
                        })
                        .collect(),
                );
                let images = Json::List(
                    bank.atlas
                        .images
                        .iter()
                        .map(|i| i.as_ref().map_or(Json::Null, |im| sha(&im.data)))
                        .collect(),
                );
                if quantized != get(&oracle, "quantized").unwrap()
                    || images != get(&oracle, "images").unwrap()
                    || Json::List(
                        bank.atlas
                            .request_map
                            .iter()
                            .map(|&i| Json::Int(i as i64))
                            .collect(),
                    ) != get(&oracle, "request_map").unwrap()
                {
                    bad += 1;
                    println!("{name}: atlas textures differ");
                }
                let pack = bank.atlas.pack();
                let want_pack = get(&oracle, "pack").unwrap();
                match (pack, get(&want_pack, "error")) {
                    (Ok(()), None) => {
                        let entries = Json::List(
                            bank.atlas
                                .entries
                                .iter()
                                .map(|e| {
                                    Json::List(e.unwrap().iter().map(|&v| Json::Int(v)).collect())
                                })
                                .collect(),
                        );
                        let palettes = Json::List(
                            bank.atlas
                                .palettes
                                .iter()
                                .map(|p| Json::Str(hexs(p)))
                                .collect(),
                        );
                        let pages = Json::List(bank.atlas.pages.iter().map(|p| sha(p)).collect());
                        if entries != get(&want_pack, "entries").unwrap()
                            || palettes != get(&want_pack, "palettes").unwrap()
                            || pages != get(&want_pack, "pages").unwrap()
                            || sha(&bank.atlas.stream) != get(&want_pack, "stream").unwrap()
                        {
                            bad += 1;
                            println!("{name}: packed atlas differs");
                        }
                    }
                    (Err(e), Some(Json::Str(want))) if e == want => {}
                    (r, w) => {
                        bad += 1;
                        println!("{name}: pack rust {:?} python {w:?}", r.map(|_| "ok"));
                    }
                }
                checked += 1;
            }
            println!("checked {checked} scenes, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "fkbank" => {
            // hk-cook-parity fkbank <source dir> <oracle-fkbank.json> <regions.json>: the False Knight's
            // source_art, plan and append_bank against host/false_knight_art.py.
            use hk_cook::atlas::{Atlas, Quantized};
            use hk_cook::fk_bank::{append_bank, art_clips, plan, source_art, PRIORITY};
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            use sha2::{Digest, Sha256};
            let source = lazy_source();
            let oracle = parse(&std::fs::read_to_string(&args[3]).unwrap()).unwrap();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let int = |j: &Json| -> i64 {
                if let Json::Int(i) = j {
                    *i
                } else {
                    panic!("int")
                }
            };
            let Json::List(scenes) = field(&regions, "scenes") else {
                panic!("scenes")
            };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let (Json::Str(file), Json::Str(name)) =
                        (field(s, "file"), field(s, "scene_name"))
                    else {
                        panic!("scene")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    let Json::List(b) = field(s, "runtime_bounds") else {
                        panic!("bounds")
                    };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let sha = |d: &[u8]| Json::Str(hexs(&Sha256::digest(d)));
            let sc = Scene::new(&source, "level46").unwrap();
            let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
            let fk = rows
                .iter()
                .find(|r| r.control.as_ref().is_some_and(|c| c.0 == "FalseKnight"))
                .expect("False Knight");
            let art = source_art(&sc, &source, fk).unwrap();
            let mut bad = 0;
            let mine_sprites = Json::List(
                art.sprites
                    .iter()
                    .map(|(k, s)| {
                        Json::List(vec![
                            Json::Str(k.repr()),
                            Json::Int(s.w as i64),
                            Json::Int(s.h as i64),
                            Json::List(s.box_.iter().map(|&v| Json::Float(v)).collect()),
                            sha(&s.image.data),
                        ])
                    })
                    .collect(),
            );
            if mine_sprites != field(&oracle, "sprites") {
                bad += 1;
                println!("source sprites differ");
            }
            let mine_clips = Json::Obj(
                art.clips
                    .iter()
                    .map(|(n, c)| {
                        (
                            n.clone(),
                            Json::Obj(vec![
                                (
                                    "keys".into(),
                                    Json::List(
                                        c.keys.iter().map(|k| Json::Str(k.repr())).collect(),
                                    ),
                                ),
                                (
                                    "fps".into(),
                                    Json::Float(
                                        c.record.get("fps").and_then(|v| v.float()).unwrap(),
                                    ),
                                ),
                            ]),
                        )
                    })
                    .collect(),
            );
            if mine_clips != field(&oracle, "clips") {
                bad += 1;
                println!("art clips differ");
            }
            let mine_floor = Json::Obj(
                art.floor_frames
                    .iter()
                    .map(|(n, ks)| {
                        (
                            n.clone(),
                            Json::List(ks.iter().map(|k| Json::Str(k.repr())).collect()),
                        )
                    })
                    .collect(),
            );
            if mine_floor != field(&oracle, "floor_frames") {
                bad += 1;
                println!("floor frames differ");
            }
            if art.objects != field(&oracle, "objects") {
                bad += 1;
                println!("objects differ");
            }
            let quantize = |im: &hk_pil::Image, w: usize, h: usize| -> Result<Quantized, String> {
                let fallback =
                    |_: &hk_pil::Image, _: usize, _: usize| -> Result<Quantized, String> {
                        Err("octree fallback is not ported".into())
                    };
                hk_cook::quantize::quantize(im, w, h, Some(&fallback))
            };
            let names = art_clips();
            let mut ok_variants = 0;
            for v in list(field(&oracle, "variants")) {
                let Json::Str(label) = field(&v, "label") else {
                    panic!("label")
                };
                let scenery: Vec<(i64, i64)> = list(field(&v, "scenery"))
                    .iter()
                    .map(|r| {
                        let l = list(r.clone());
                        (int(&l[0]), int(&l[1]))
                    })
                    .collect();
                let limit = int(&field(&v, "stream_limit"));
                let mut atlas = Atlas::new(true, 40, 640, true, None);
                let (mut frames, mut clips) = (Vec::new(), Vec::new());
                let result = plan(
                    &art,
                    &scenery,
                    &quantize,
                    &PRIORITY,
                    18,
                    limit,
                    "False Knight",
                )
                .and_then(|p| {
                    append_bank(
                        &mut atlas,
                        &mut frames,
                        &mut clips,
                        &art,
                        &p,
                        &names,
                        "False Knight parts",
                    )
                    .map(|t| (p, t))
                })
                .and_then(|r| atlas.pack().map(|_| r));
                match (result, field(&v, "error")) {
                    (Ok((p, t)), Json::Null) => {
                        ok_variants += 1;
                        let cut = Json::List(
                            p.cut
                                .iter()
                                .map(|(k, c)| {
                                    Json::List(vec![
                                        Json::Str(k.repr()),
                                        Json::Bool(c.0),
                                        Json::List(
                                            c.1.iter()
                                                .map(|r| {
                                                    Json::List(
                                                        r.iter().map(|&x| Json::Int(x)).collect(),
                                                    )
                                                })
                                                .collect(),
                                        ),
                                    ])
                                })
                                .collect(),
                        );
                        let rows3 = Json::List(
                            t.sprite_rows
                                .iter()
                                .map(|r| {
                                    Json::List(vec![
                                        Json::Int(r.0),
                                        Json::Int(r.1),
                                        Json::Int(r.2 as i64),
                                    ])
                                })
                                .collect(),
                        );
                        let crow = Json::List(
                            t.clip_rows
                                .iter()
                                .map(|r| {
                                    Json::List(vec![
                                        Json::Int(r.0),
                                        Json::Int(r.1),
                                        Json::Int(r.2),
                                        Json::Int(r.3),
                                        Json::Int(r.4),
                                        Json::Str(r.5.clone()),
                                    ])
                                })
                                .collect(),
                        );
                        let seq = Json::List(t.sequence.iter().map(|&x| Json::Int(x)).collect());
                        let floor = Json::List(
                            t.floor_rows
                                .iter()
                                .map(|(a, b)| {
                                    Json::List(vec![
                                        Json::Str(a.clone()),
                                        Json::List(b.iter().map(|&x| Json::Int(x)).collect()),
                                    ])
                                })
                                .collect(),
                        );
                        let fr = Json::List(
                            frames
                                .iter()
                                .map(|f: &hk_cook::actor_art::Frame| {
                                    Json::Obj(vec![
                                        ("texture".into(), Json::Int(f.texture as i64)),
                                        (
                                            "box".into(),
                                            Json::List(
                                                f.box_.iter().map(|&x| Json::Float(x)).collect(),
                                            ),
                                        ),
                                        (
                                            "box_q16".into(),
                                            Json::List(
                                                f.box_q16
                                                    .unwrap()
                                                    .iter()
                                                    .map(|&x| Json::Int(x))
                                                    .collect(),
                                            ),
                                        ),
                                        ("sprite".into(), Json::Str(f.sprite.clone())),
                                    ])
                                })
                                .collect(),
                        );
                        let cl = Json::List(
                            clips
                                .iter()
                                .map(|c: &hk_cook::actor_art::Clip| {
                                    Json::Obj(vec![
                                        ("name".into(), Json::Str(c.name.clone())),
                                        ("start".into(), Json::Int(c.start as i64)),
                                        ("count".into(), Json::Int(c.count as i64)),
                                        ("fps".into(), Json::Float(c.fps)),
                                        ("wrap".into(), Json::Int(c.wrap)),
                                        ("loopStart".into(), Json::Int(c.loop_start)),
                                    ])
                                })
                                .collect(),
                        );
                        let pack = Json::Obj(vec![
                            (
                                "entries".into(),
                                Json::List(
                                    atlas
                                        .entries
                                        .iter()
                                        .map(|e| {
                                            Json::List(
                                                e.unwrap().iter().map(|&x| Json::Int(x)).collect(),
                                            )
                                        })
                                        .collect(),
                                ),
                            ),
                            (
                                "palettes".into(),
                                Json::List(
                                    atlas.palettes.iter().map(|x| Json::Str(hexs(x))).collect(),
                                ),
                            ),
                            (
                                "pages".into(),
                                Json::List(atlas.pages.iter().map(|x| sha(x)).collect()),
                            ),
                            ("stream".into(), sha(&atlas.stream)),
                        ]);
                        let good = p.decisions == list(field(&v, "decisions"))
                            && p.totals == field(&v, "totals")
                            && cut == field(&v, "cut")
                            && int(&field(&v, "anchor")) as usize == t.anchor
                            && rows3 == field(&v, "sprite_rows")
                            && crow == field(&v, "clip_rows")
                            && seq == field(&v, "sequence")
                            && floor == field(&v, "floor_rows")
                            && fr == field(&v, "frames")
                            && cl == field(&v, "clips")
                            && pack == field(&v, "pack");
                        if !good {
                            bad += 1;
                            println!("variant {label}: bank differs (decisions {} totals {} cut {} rows {} clips {} seq {} floor {} frames {} clips {} pack {})", p.decisions == list(field(&v, "decisions")), p.totals == field(&v, "totals"), cut == field(&v, "cut"), rows3 == field(&v, "sprite_rows"), crow == field(&v, "clip_rows"), seq == field(&v, "sequence"), floor == field(&v, "floor_rows"), fr == field(&v, "frames"), cl == field(&v, "clips"), pack == field(&v, "pack"));
                        }
                    }
                    (Err(e), Json::Str(want)) => {
                        if e != want {
                            bad += 1;
                            println!("variant {label}: refusal {e:?}, python {want:?}");
                        }
                    }
                    (r, want) => {
                        bad += 1;
                        println!(
                            "variant {label}: rust {:?}, python {want:?}",
                            r.map(|_| "ok")
                        );
                    }
                }
            }
            println!("checked source art and {ok_variants} cooked banks (plus refusals), {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "fkscene" => {
            // hk-cook-parity fkscene <source dir> <oracle-fkscene dir> <regions.json>: cook_scene_bank over
            // synthetic region files, including the generated Rust tables and art report.
            use hk_cook::atlas::{Atlas, Quantized};
            use hk_cook::fk_bank::{cook_scene_bank, BankRegion};
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            use sha2::{Digest, Sha256};
            let source = lazy_source();
            let dir = std::path::PathBuf::from(&args[3]);
            let oracle =
                parse(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let int = |j: &Json| -> i64 {
                if let Json::Int(i) = j {
                    *i
                } else {
                    panic!("int")
                }
            };
            let Json::List(scenes) = field(&regions, "scenes") else {
                panic!("scenes")
            };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let (Json::Str(file), Json::Str(name)) =
                        (field(s, "file"), field(s, "scene_name"))
                    else {
                        panic!("scene")
                    };
                    let f = |j: &Json| match j {
                        Json::Int(i) => *i as f64,
                        Json::Float(x) => *x,
                        _ => panic!("bound"),
                    };
                    let Json::List(b) = field(s, "runtime_bounds") else {
                        panic!("bounds")
                    };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let sha = |d: &[u8]| Json::Str(hexs(&Sha256::digest(d)));
            let sc = Scene::new(&source, "level46").unwrap();
            let scan = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
            let fk = scan
                .iter()
                .find(|r| r.control.as_ref().is_some_and(|c| c.0 == "FalseKnight"))
                .expect("False Knight");
            let rows: Vec<BankRegion> = list(field(&oracle, "rows"))
                .iter()
                .map(|r| BankRegion {
                    chunk_id: int(&field(r, "chunk_id")),
                    scene_id: int(&field(r, "scene_id")),
                    edge_sources: list(field(r, "edge_sources"))
                        .into_iter()
                        .map(|s| {
                            if let Json::Str(s) = s {
                                s
                            } else {
                                panic!("src")
                            }
                        })
                        .collect(),
                })
                .collect();
            let quantize = |im: &hk_pil::Image, w: usize, h: usize| -> Result<Quantized, String> {
                let fallback =
                    |_: &hk_pil::Image, _: usize, _: usize| -> Result<Quantized, String> {
                        Err("octree fallback is not ported".into())
                    };
                hk_cook::quantize::quantize(im, w, h, Some(&fallback))
            };
            let mut atlas = Atlas::standard();
            let (mut frames, mut clips) = (Vec::new(), Vec::new());
            let mut writer = cook_scene_bank(
                &sc,
                &source,
                fk,
                &rows,
                &dir,
                &mut atlas,
                &mut frames,
                &mut clips,
                &quantize,
            )
            .unwrap();
            writer.write(&dir, 11).unwrap();
            atlas.pack().unwrap();
            let mut bad = 0;
            let text = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap();
            let files = field(&oracle, "files");
            for n in ["false_knight_art.rs", "false_knight_floor.rs"] {
                if Json::Str(text(&format!("data/{n}"))) != field(&files, n) {
                    bad += 1;
                    println!("{n} differs");
                }
            }
            let Json::Obj(mut report) = parse(&text(".hkpsx/false-knight/art.json")).unwrap()
            else {
                panic!("report")
            };
            report.retain(|f| f.0 != "code_sha256");
            if Json::Obj(report) != field(&oracle, "report") {
                bad += 1;
                println!("art report differs");
            }
            let fr = Json::List(
                frames
                    .iter()
                    .map(|f: &hk_cook::actor_art::Frame| {
                        Json::Obj(vec![
                            ("texture".into(), Json::Int(f.texture as i64)),
                            (
                                "box".into(),
                                Json::List(f.box_.iter().map(|&x| Json::Float(x)).collect()),
                            ),
                            (
                                "box_q16".into(),
                                Json::List(
                                    f.box_q16.unwrap().iter().map(|&x| Json::Int(x)).collect(),
                                ),
                            ),
                            ("sprite".into(), Json::Str(f.sprite.clone())),
                        ])
                    })
                    .collect(),
            );
            if fr != field(&oracle, "frames") {
                bad += 1;
                println!("frames differ");
            }
            let cl = Json::List(
                clips
                    .iter()
                    .map(|c: &hk_cook::actor_art::Clip| {
                        Json::Obj(vec![
                            ("name".into(), Json::Str(c.name.clone())),
                            ("start".into(), Json::Int(c.start as i64)),
                            ("count".into(), Json::Int(c.count as i64)),
                            ("fps".into(), Json::Float(c.fps)),
                            ("wrap".into(), Json::Int(c.wrap)),
                            ("loopStart".into(), Json::Int(c.loop_start)),
                        ])
                    })
                    .collect(),
            );
            if cl != field(&oracle, "clips") {
                bad += 1;
                println!("clips differ");
            }
            let pack = Json::Obj(vec![
                (
                    "entries".into(),
                    Json::List(
                        atlas
                            .entries
                            .iter()
                            .map(|e| Json::List(e.unwrap().iter().map(|&x| Json::Int(x)).collect()))
                            .collect(),
                    ),
                ),
                (
                    "palettes".into(),
                    Json::List(atlas.palettes.iter().map(|x| Json::Str(hexs(x))).collect()),
                ),
                (
                    "pages".into(),
                    Json::List(atlas.pages.iter().map(|x| sha(x)).collect()),
                ),
                ("stream".into(), sha(&atlas.stream)),
            ]);
            if pack != field(&oracle, "pack") {
                bad += 1;
                println!("packed atlas differs");
            }
            let _ = std::fs::remove_dir_all(dir.join(".hkpsx"));
            for n in ["false_knight_art.rs", "false_knight_floor.rs"] {
                let _ = std::fs::remove_file(dir.join("data").join(n));
            }
            println!("checked the False Knight scene bank, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "mawlek" => {
            // hk-cook-parity mawlek <source dir> <oracle-mawlek dir>: host/mawlek_art.py over Crossroads_09.
            use hk_cook::atlas::{Atlas, Quantized};
            use hk_cook::mawlek_art as ma;
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            use sha2::{Digest, Sha256};
            let source = lazy_source();
            let dir = std::path::PathBuf::from(&args[3]);
            let oracle =
                parse(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let int = |j: &Json| -> i64 {
                if let Json::Int(i) = j {
                    *i
                } else {
                    panic!("int")
                }
            };
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let sha = |d: &[u8]| Json::Str(hexs(&Sha256::digest(d)));
            let sc = Scene::new(&source, "level45").unwrap();
            let mut bad = 0;
            let src = ma::sources(&sc, &source).unwrap();
            let digests = ma::check_contract(&sc, &source, &src).unwrap();
            if Json::Obj(
                digests
                    .iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                    .collect(),
            ) != field(&oracle, "digests")
            {
                bad += 1;
                println!("fsm digests differ");
            }
            ma::check_rust(&dir).unwrap();
            let geo = ma::geometry(&sc, &src).unwrap();
            if geo.to_json() != field(&oracle, "geometry") {
                bad += 1;
                println!(
                    "geometry differs: {}",
                    hk_cook::pyjson::dumps(&geo.to_json())
                );
            }
            let (art, turn) = ma::source_art(&sc, &source, &src).unwrap();
            if Json::Obj(vec![("spit_turn_degrees".into(), Json::Int(turn))])
                != field(&oracle, "extra")
            {
                bad += 1;
                println!("spit turn differs");
            }
            let mine_sprites = Json::List(
                art.sprites
                    .iter()
                    .map(|(k, s)| {
                        Json::List(vec![
                            Json::Str(k.repr()),
                            Json::Int(s.w as i64),
                            Json::Int(s.h as i64),
                            Json::List(s.box_.iter().map(|&v| Json::Float(v)).collect()),
                            sha(&s.image.data),
                        ])
                    })
                    .collect(),
            );
            if mine_sprites != field(&oracle, "sprites") {
                bad += 1;
                let (a, b) = (list(mine_sprites.clone()), list(field(&oracle, "sprites")));
                println!("source sprites differ ({} vs {})", a.len(), b.len());
                for (x, y) in a.iter().zip(&b) {
                    if x != y {
                        println!(
                            " mine   {}\n python {}",
                            hk_cook::pyjson::dumps_sorted_compact(x),
                            hk_cook::pyjson::dumps_sorted_compact(y)
                        );
                        break;
                    }
                }
            }
            let mine_clips = Json::Obj(
                art.clips
                    .iter()
                    .map(|(n, c)| {
                        (
                            n.clone(),
                            Json::Obj(vec![
                                (
                                    "keys".into(),
                                    Json::List(
                                        c.keys.iter().map(|k| Json::Str(k.repr())).collect(),
                                    ),
                                ),
                                (
                                    "fps".into(),
                                    Json::Float(
                                        c.record.get("fps").and_then(|v| v.float()).unwrap(),
                                    ),
                                ),
                            ]),
                        )
                    })
                    .collect(),
            );
            if mine_clips != field(&oracle, "clips") {
                bad += 1;
                println!("art clips differ");
            }
            let quantize = |im: &hk_pil::Image, w: usize, h: usize| -> Result<Quantized, String> {
                let fallback =
                    |_: &hk_pil::Image, _: usize, _: usize| -> Result<Quantized, String> {
                        Err("octree fallback is not ported".into())
                    };
                hk_cook::quantize::quantize(im, w, h, Some(&fallback))
            };
            let names = ma::art_clips();
            let bank_json = |frames: &Vec<hk_cook::actor_art::Frame>,
                             clips: &Vec<hk_cook::actor_art::Clip>,
                             atlas: &Atlas| {
                let fr = Json::List(
                    frames
                        .iter()
                        .map(|f| {
                            Json::Obj(vec![
                                ("texture".into(), Json::Int(f.texture as i64)),
                                (
                                    "box".into(),
                                    Json::List(f.box_.iter().map(|&x| Json::Float(x)).collect()),
                                ),
                                (
                                    "box_q16".into(),
                                    Json::List(
                                        f.box_q16.unwrap().iter().map(|&x| Json::Int(x)).collect(),
                                    ),
                                ),
                                ("sprite".into(), Json::Str(f.sprite.clone())),
                            ])
                        })
                        .collect(),
                );
                let cl = Json::List(
                    clips
                        .iter()
                        .map(|c| {
                            Json::Obj(vec![
                                ("name".into(), Json::Str(c.name.clone())),
                                ("start".into(), Json::Int(c.start as i64)),
                                ("count".into(), Json::Int(c.count as i64)),
                                ("fps".into(), Json::Float(c.fps)),
                                ("wrap".into(), Json::Int(c.wrap)),
                                ("loopStart".into(), Json::Int(c.loop_start)),
                            ])
                        })
                        .collect(),
                );
                let pack = Json::Obj(vec![
                    (
                        "entries".into(),
                        Json::List(
                            atlas
                                .entries
                                .iter()
                                .map(|e| {
                                    Json::List(e.unwrap().iter().map(|&x| Json::Int(x)).collect())
                                })
                                .collect(),
                        ),
                    ),
                    (
                        "palettes".into(),
                        Json::List(atlas.palettes.iter().map(|x| Json::Str(hexs(x))).collect()),
                    ),
                    (
                        "pages".into(),
                        Json::List(atlas.pages.iter().map(|x| sha(x)).collect()),
                    ),
                    ("stream".into(), sha(&atlas.stream)),
                ]);
                (fr, cl, pack)
            };
            let mut ok_variants = 0;
            for v in list(field(&oracle, "variants")) {
                let Json::Str(label) = field(&v, "label") else {
                    panic!("label")
                };
                let scenery: Vec<(i64, i64)> = list(field(&v, "scenery"))
                    .iter()
                    .map(|r| {
                        let l = list(r.clone());
                        (int(&l[0]), int(&l[1]))
                    })
                    .collect();
                let mut atlas = Atlas::new(true, 40, 640, true, None);
                let (mut frames, mut clips) = (Vec::new(), Vec::new());
                let result = hk_cook::fk_bank::plan(
                    &art,
                    &scenery,
                    &quantize,
                    &ma::PRIORITY,
                    ma::SCENE_PAGE_LIMIT,
                    ma::STREAM_BYTES_LIMIT,
                    "Mawlek",
                )
                .and_then(|p| {
                    hk_cook::fk_bank::append_bank(
                        &mut atlas,
                        &mut frames,
                        &mut clips,
                        &art,
                        &p,
                        &names,
                        "Mawlek parts",
                    )
                    .map(|t| (p, t))
                })
                .and_then(|r| atlas.pack().map(|_| r));
                match (result, field(&v, "error")) {
                    (Ok((p, t)), Json::Null) => {
                        ok_variants += 1;
                        let cut = Json::List(
                            p.cut
                                .iter()
                                .map(|(k, c)| {
                                    Json::List(vec![
                                        Json::Str(k.repr()),
                                        Json::Bool(c.0),
                                        Json::List(
                                            c.1.iter()
                                                .map(|r| {
                                                    Json::List(
                                                        r.iter().map(|&x| Json::Int(x)).collect(),
                                                    )
                                                })
                                                .collect(),
                                        ),
                                    ])
                                })
                                .collect(),
                        );
                        let rows3 = Json::List(
                            t.sprite_rows
                                .iter()
                                .map(|r| {
                                    Json::List(vec![
                                        Json::Int(r.0),
                                        Json::Int(r.1),
                                        Json::Int(r.2 as i64),
                                    ])
                                })
                                .collect(),
                        );
                        let crow = Json::List(
                            t.clip_rows
                                .iter()
                                .map(|r| {
                                    Json::List(vec![
                                        Json::Int(r.0),
                                        Json::Int(r.1),
                                        Json::Int(r.2),
                                        Json::Int(r.3),
                                        Json::Int(r.4),
                                        Json::Str(r.5.clone()),
                                    ])
                                })
                                .collect(),
                        );
                        let seq = Json::List(t.sequence.iter().map(|&x| Json::Int(x)).collect());
                        let (fr, cl, pack) = bank_json(&frames, &clips, &atlas);
                        let table = Json::Str(ma::rust_table(t.anchor as i64 + 5, &t, &geo, 9));
                        let good = p.decisions == list(field(&v, "decisions"))
                            && p.totals == field(&v, "totals")
                            && cut == field(&v, "cut")
                            && int(&field(&v, "anchor")) as usize == t.anchor
                            && rows3 == field(&v, "sprite_rows")
                            && crow == field(&v, "clip_rows")
                            && seq == field(&v, "sequence")
                            && fr == field(&v, "frames")
                            && cl == field(&v, "clips")
                            && pack == field(&v, "pack")
                            && table == field(&v, "table");
                        if !good {
                            bad += 1;
                            println!("variant {label}: bank differs");
                        }
                    }
                    (Err(e), Json::Str(want)) => {
                        if e != want {
                            bad += 1;
                            println!("variant {label}: refusal {e:?}, python {want:?}");
                        }
                    }
                    (r, want) => {
                        bad += 1;
                        println!(
                            "variant {label}: rust {:?}, python {want:?}",
                            r.map(|_| "ok")
                        );
                    }
                }
            }
            // The whole scene bank over synthetic region files.
            let scene = field(&oracle, "scene");
            let rows: Vec<ma::MawlekRegion> = list(field(&scene, "rows"))
                .iter()
                .map(|r| ma::MawlekRegion {
                    chunk_id: int(&field(r, "chunk_id")),
                    scene_id: int(&field(r, "scene_id")),
                })
                .collect();
            let mut atlas = Atlas::standard();
            let (mut frames, mut clips) = (Vec::new(), Vec::new());
            let mut writer = ma::cook_scene_bank(
                &sc,
                &source,
                &rows,
                &dir,
                &mut atlas,
                &mut frames,
                &mut clips,
                &quantize,
            )
            .unwrap();
            writer.write(&dir, 7).unwrap();
            atlas.pack().unwrap();
            let text = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap();
            if Json::Str(text("data/mawlek_art.rs")) != field(&scene, "file") {
                bad += 1;
                println!("data/mawlek_art.rs differs");
            }
            let Json::Obj(mut report) = parse(&text(".hkpsx/mawlek/art.json")).unwrap() else {
                panic!("report")
            };
            report.retain(|f| f.0 != "code_sha256");
            if Json::Obj(report) != field(&scene, "report") {
                bad += 1;
                println!("art report differs");
            }
            let (fr, cl, pack) = bank_json(&frames, &clips, &atlas);
            if fr != field(&scene, "frames")
                || cl != field(&scene, "clips")
                || pack != field(&scene, "pack")
            {
                bad += 1;
                println!("scene bank differs");
            }
            let _ = std::fs::remove_dir_all(dir.join(".hkpsx"));
            let _ = std::fs::remove_file(dir.join("data/mawlek_art.rs"));
            println!("checked the contract, geometry, source art, {ok_variants} cooked banks and the scene bank, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        "gruz" => {
            // hk-cook-parity gruz <source dir> <oracle-gruz dir>: the Gruz Mother bank of host/false_knight_art.py over Crossroads_04.
            use hk_cook::atlas::{Atlas, Quantized};
            use hk_cook::gruz_art as ma;
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            use sha2::{Digest, Sha256};
            let source = lazy_source();
            let dir = std::path::PathBuf::from(&args[3]);
            let oracle =
                parse(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();
            let field = |j: &Json, k: &str| -> Json {
                if let Json::Obj(f) = j {
                    f.iter()
                        .find(|x| x.0 == k)
                        .map(|x| x.1.clone())
                        .unwrap_or_else(|| panic!("no {k}"))
                } else {
                    panic!("not an object")
                }
            };
            let list = |j: Json| -> Vec<Json> {
                if let Json::List(l) = j {
                    l
                } else {
                    panic!("list")
                }
            };
            let int = |j: &Json| -> i64 {
                if let Json::Int(i) = j {
                    *i
                } else {
                    panic!("int")
                }
            };
            let hexs = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
            let sha = |d: &[u8]| Json::Str(hexs(&Sha256::digest(d)));
            let sc = Scene::new(&source, "level40").unwrap();
            let mut bad = 0;
            let src = ma::sources(&sc, &source).unwrap();
            let digests = ma::check_contract(&sc, &source, &src).unwrap();
            if Json::Obj(
                digests
                    .iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                    .collect(),
            ) != field(&oracle, "digests")
            {
                bad += 1;
                println!("fsm digests differ");
            }
            ma::check_rust(&dir).unwrap();
            let geo = ma::geometry(&sc, &src).unwrap();
            if geo.to_json() != field(&oracle, "geometry") {
                bad += 1;
                println!(
                    "geometry differs: {}",
                    hk_cook::pyjson::dumps(&geo.to_json())
                );
            }
            let (art, boxes) = ma::source_art(&sc, &source, &src).unwrap();
            let mut order: Vec<hk_cook::fk_bank::SpriteKey> = Vec::new();
            for name in ma::CLIPS {
                for k in &art.clip(name).unwrap().keys {
                    if !order.contains(k) {
                        order.push(k.clone());
                    }
                }
            }
            let mine_boxes = Json::List(
                order
                    .iter()
                    .map(|k| {
                        boxes
                            .iter()
                            .find(|b| &b.0 == k)
                            .and_then(|b| b.1)
                            .map_or(Json::Null, |b| {
                                Json::List(b.iter().map(|&v| Json::Float(v)).collect())
                            })
                    })
                    .collect(),
            );
            if mine_boxes != field(&oracle, "boxes") {
                bad += 1;
                println!("sprite boxes differ");
            }
            let mine_sprites = Json::List(
                art.sprites
                    .iter()
                    .map(|(k, s)| {
                        Json::List(vec![
                            Json::Str(k.repr()),
                            Json::Int(s.w as i64),
                            Json::Int(s.h as i64),
                            Json::List(s.box_.iter().map(|&v| Json::Float(v)).collect()),
                            sha(&s.image.data),
                        ])
                    })
                    .collect(),
            );
            if mine_sprites != field(&oracle, "sprites") {
                bad += 1;
                let (a, b) = (list(mine_sprites.clone()), list(field(&oracle, "sprites")));
                println!("source sprites differ ({} vs {})", a.len(), b.len());
                for (x, y) in a.iter().zip(&b) {
                    if x != y {
                        println!(
                            " mine   {}\n python {}",
                            hk_cook::pyjson::dumps_sorted_compact(x),
                            hk_cook::pyjson::dumps_sorted_compact(y)
                        );
                        break;
                    }
                }
            }
            let mine_clips = Json::Obj(
                art.clips
                    .iter()
                    .map(|(n, c)| {
                        (
                            n.clone(),
                            Json::Obj(vec![
                                (
                                    "keys".into(),
                                    Json::List(
                                        c.keys.iter().map(|k| Json::Str(k.repr())).collect(),
                                    ),
                                ),
                                (
                                    "fps".into(),
                                    Json::Float(
                                        c.record.get("fps").and_then(|v| v.float()).unwrap(),
                                    ),
                                ),
                                (
                                    "wrap".into(),
                                    Json::Int(
                                        c.record.get("wrapMode").and_then(|v| v.int()).unwrap(),
                                    ),
                                ),
                            ]),
                        )
                    })
                    .collect(),
            );
            if mine_clips != field(&oracle, "clips") {
                bad += 1;
                println!("art clips differ");
            }
            let quantize = |im: &hk_pil::Image, w: usize, h: usize| -> Result<Quantized, String> {
                let fallback =
                    |_: &hk_pil::Image, _: usize, _: usize| -> Result<Quantized, String> {
                        Err("octree fallback is not ported".into())
                    };
                hk_cook::quantize::quantize(im, w, h, Some(&fallback))
            };
            let names: Vec<String> = ma::CLIPS.iter().map(|s| s.to_string()).collect();
            let bank_json = |frames: &Vec<hk_cook::actor_art::Frame>,
                             clips: &Vec<hk_cook::actor_art::Clip>,
                             atlas: &Atlas| {
                let fr = Json::List(
                    frames
                        .iter()
                        .map(|f| {
                            Json::Obj(vec![
                                ("texture".into(), Json::Int(f.texture as i64)),
                                (
                                    "box".into(),
                                    Json::List(f.box_.iter().map(|&x| Json::Float(x)).collect()),
                                ),
                                (
                                    "box_q16".into(),
                                    Json::List(
                                        f.box_q16.unwrap().iter().map(|&x| Json::Int(x)).collect(),
                                    ),
                                ),
                                ("sprite".into(), Json::Str(f.sprite.clone())),
                            ])
                        })
                        .collect(),
                );
                let cl = Json::List(
                    clips
                        .iter()
                        .map(|c| {
                            Json::Obj(vec![
                                ("name".into(), Json::Str(c.name.clone())),
                                ("start".into(), Json::Int(c.start as i64)),
                                ("count".into(), Json::Int(c.count as i64)),
                                ("fps".into(), Json::Float(c.fps)),
                                ("wrap".into(), Json::Int(c.wrap)),
                                ("loopStart".into(), Json::Int(c.loop_start)),
                            ])
                        })
                        .collect(),
                );
                let pack = Json::Obj(vec![
                    (
                        "entries".into(),
                        Json::List(
                            atlas
                                .entries
                                .iter()
                                .map(|e| {
                                    Json::List(e.unwrap().iter().map(|&x| Json::Int(x)).collect())
                                })
                                .collect(),
                        ),
                    ),
                    (
                        "palettes".into(),
                        Json::List(atlas.palettes.iter().map(|x| Json::Str(hexs(x))).collect()),
                    ),
                    (
                        "pages".into(),
                        Json::List(atlas.pages.iter().map(|x| sha(x)).collect()),
                    ),
                    ("stream".into(), sha(&atlas.stream)),
                ]);
                (fr, cl, pack)
            };
            let mut ok_variants = 0;
            for v in list(field(&oracle, "variants")) {
                let Json::Str(label) = field(&v, "label") else {
                    panic!("label")
                };
                let scenery: Vec<(i64, i64)> = list(field(&v, "scenery"))
                    .iter()
                    .map(|r| {
                        let l = list(r.clone());
                        (int(&l[0]), int(&l[1]))
                    })
                    .collect();
                let mut atlas = Atlas::new(true, 40, 640, true, None);
                let (mut frames, mut clips) = (Vec::new(), Vec::new());
                let result = hk_cook::fk_bank::plan(
                    &art,
                    &scenery,
                    &quantize,
                    &ma::PRIORITY,
                    ma::SCENE_PAGE_LIMIT,
                    ma::STREAM_BYTES_LIMIT,
                    "Gruz Mother",
                )
                .and_then(|p| {
                    hk_cook::fk_bank::append_bank(
                        &mut atlas,
                        &mut frames,
                        &mut clips,
                        &art,
                        &p,
                        &names,
                        "Gruz Mother parts",
                    )
                    .map(|t| (p, t))
                })
                .and_then(|r| atlas.pack().map(|_| r));
                match (result, field(&v, "error")) {
                    (Ok((p, t)), Json::Null) => {
                        ok_variants += 1;
                        let cut = Json::List(
                            p.cut
                                .iter()
                                .map(|(k, c)| {
                                    Json::List(vec![
                                        Json::Str(k.repr()),
                                        Json::Bool(c.0),
                                        Json::List(
                                            c.1.iter()
                                                .map(|r| {
                                                    Json::List(
                                                        r.iter().map(|&x| Json::Int(x)).collect(),
                                                    )
                                                })
                                                .collect(),
                                        ),
                                    ])
                                })
                                .collect(),
                        );
                        let rows3 = Json::List(
                            t.sprite_rows
                                .iter()
                                .map(|r| {
                                    Json::List(vec![
                                        Json::Int(r.0),
                                        Json::Int(r.1),
                                        Json::Int(r.2 as i64),
                                    ])
                                })
                                .collect(),
                        );
                        let crow = Json::List(
                            t.clip_rows
                                .iter()
                                .map(|r| {
                                    Json::List(vec![
                                        Json::Int(r.0),
                                        Json::Int(r.1),
                                        Json::Int(r.2),
                                        Json::Int(r.3),
                                        Json::Int(r.4),
                                        Json::Str(r.5.clone()),
                                    ])
                                })
                                .collect(),
                        );
                        let seq = Json::List(t.sequence.iter().map(|&x| Json::Int(x)).collect());
                        let (fr, cl, pack) = bank_json(&frames, &clips, &atlas);
                        let sprite_boxes: Vec<Option<[f64; 4]>> = order
                            .iter()
                            .map(|k| boxes.iter().find(|b| &b.0 == k).and_then(|b| b.1))
                            .collect();
                        let table = Json::Str(ma::rust_table(
                            t.anchor as i64 + 5,
                            &t,
                            &geo,
                            9,
                            &sprite_boxes,
                        ));
                        let good = p.decisions == list(field(&v, "decisions"))
                            && p.totals == field(&v, "totals")
                            && cut == field(&v, "cut")
                            && int(&field(&v, "anchor")) as usize == t.anchor
                            && rows3 == field(&v, "sprite_rows")
                            && crow == field(&v, "clip_rows")
                            && seq == field(&v, "sequence")
                            && fr == field(&v, "frames")
                            && cl == field(&v, "clips")
                            && pack == field(&v, "pack")
                            && table == field(&v, "table");
                        if !good {
                            bad += 1;
                            println!("variant {label}: bank differs");
                        }
                    }
                    (Err(e), Json::Str(want)) => {
                        if e != want {
                            bad += 1;
                            println!("variant {label}: refusal {e:?}, python {want:?}");
                        }
                    }
                    (r, want) => {
                        bad += 1;
                        println!(
                            "variant {label}: rust {:?}, python {want:?}",
                            r.map(|_| "ok")
                        );
                    }
                }
            }
            // The whole scene bank over synthetic region files.
            let scene = field(&oracle, "scene");
            let rows: Vec<ma::GruzRegion> = list(field(&scene, "rows"))
                .iter()
                .map(|r| ma::GruzRegion {
                    chunk_id: int(&field(r, "chunk_id")),
                    scene_id: int(&field(r, "scene_id")),
                })
                .collect();
            let mut atlas = Atlas::standard();
            let (mut frames, mut clips) = (Vec::new(), Vec::new());
            let mut writer = ma::cook_scene_bank(
                &sc,
                &source,
                &rows,
                &dir,
                &mut atlas,
                &mut frames,
                &mut clips,
                &quantize,
            )
            .unwrap();
            writer.write(&dir, 7).unwrap();
            atlas.pack().unwrap();
            let text = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap();
            if Json::Str(text("data/gruz_art.rs")) != field(&scene, "file") {
                bad += 1;
                println!("data/gruz_art.rs differs");
            }
            let Json::Obj(mut report) = parse(&text(".hkpsx/gruz-mother/art.json")).unwrap() else {
                panic!("report")
            };
            report.retain(|f| f.0 != "code_sha256");
            if Json::Obj(report) != field(&scene, "report") {
                bad += 1;
                println!("art report differs");
            }
            let (fr, cl, pack) = bank_json(&frames, &clips, &atlas);
            if fr != field(&scene, "frames")
                || cl != field(&scene, "clips")
                || pack != field(&scene, "pack")
            {
                bad += 1;
                println!("scene bank differs");
            }
            let _ = std::fs::remove_dir_all(dir.join(".hkpsx"));
            let _ = std::fs::remove_file(dir.join("data/gruz_art.rs"));
            println!("checked the contract, geometry, source art, {ok_variants} cooked banks and the scene bank, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        other => panic!("unknown mode {other}"),
    }
}
