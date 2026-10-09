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
        "actors" => {
            // hk-cook-parity actors <source dir> <oracle dir> <regions.json>: the controls the
            // ported recognizers give each actor against the Python actor_sources dump.
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions { f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone()) } else { None }) else { panic!("no scenes") };
            let get = |j: &Json, k: &str| -> Option<Json> { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()) } else { None } };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let Some(Json::Str(file)) = get(s, "file") else { panic!("scene file") };
                    let Some(Json::Str(name)) = get(s, "scene_name") else { panic!("scene name") };
                    let Some(Json::List(b)) = get(s, "runtime_bounds") else { panic!("runtime_bounds") };
                    let f = |j: &Json| match j { Json::Int(i) => *i as f64, Json::Float(x) => *x, _ => panic!("bound") };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let ported = ["ZombieSwipeWalker", "Baldur", "Aspid", "FalseKnight", "Climber", "MossWalker", "Vengefly", "AcidFlyer", "Mosquito", "Gruzzer", "GruzzerReserve", "GruzMother", "WalkLeftRight", "EggSac", "Mawlek", "ZombieShield", "Pigeon", "Blocker", "HuskGuard", "Hatcher", "HatcherBaby"];
            let (mut checked, mut bad) = (std::collections::BTreeMap::<String, usize>::new(), 0usize);
            for s in &scenes {
                let (Some(Json::Str(name)), Some(Json::Str(file))) = (get(s, "scene_name"), get(s, "file")) else { panic!("scene row") };
                let oracle = parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap()).unwrap();
                let Json::List(oracle) = oracle else { panic!("oracle shape") };
                let sc = Scene::new(&source, &file).unwrap();
                let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
                for row in &oracle {
                    let Some(Json::Str(src)) = get(row, "source") else { continue };
                    let control = get(row, "movement_control");
                    let kind = control.as_ref().and_then(|c| get(c, "kind"));
                    let mine = rows.iter().find(|r| r.source == src).and_then(|r| r.control.as_ref());
                    match (&kind, mine) {
                        (Some(Json::Str(k)), m) if ported.contains(&k.as_str()) => {
                            *checked.entry(k.clone()).or_default() += 1;
                            if m.map(|m| &m.1) != control.as_ref() {
                                bad += 1;
                                println!("{name} {src}: {k} control differs{}", if m.is_none() { " (not recognized)" } else { "" });
                            }
                            // `movement_supported` carries the additive-scene rule.
                            let python = matches!(get(row, "movement_supported"), Some(Json::Bool(true)));
                            if rows.iter().find(|r| r.source == src).is_some_and(|r| r.supported != python) {
                                bad += 1;
                                println!("{name} {src}: {k} supported flag differs (oracle {python})");
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
                        let strs = |v: &[String]| Json::List(v.iter().map(|s| Json::Str(s.clone())).collect());
                        let mine = [
                            ("position", Json::List(r.position.iter().map(|&f| Json::Float(f)).collect())),
                            ("health", hk_cook::music::value_json(r.health_manager.get("hp").unwrap())),
                            ("components", Json::Obj(r.components.iter().map(|c| (c.0.to_string(), Json::Str(c.1.clone()))).collect())),
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
            use hk_cook::actor_specs::{generated_actor_records, scene_actor_bank, Region, SpecActor};
            use hk_cook::pyjson::{parse, Json};
            use hk_unity::scene::Scene;
            let source = lazy_source();
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions { f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone()) } else { None }) else { panic!("no scenes") };
            let get = |j: &Json, k: &str| -> Option<Json> { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()) } else { None } };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let Some(Json::Str(file)) = get(s, "file") else { panic!("scene file") };
                    let Some(Json::Str(name)) = get(s, "scene_name") else { panic!("scene name") };
                    let Some(Json::List(b)) = get(s, "runtime_bounds") else { panic!("runtime_bounds") };
                    let f = |j: &Json| match j { Json::Int(i) => *i as f64, Json::Float(x) => *x, _ => panic!("bound") };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let Json::List(keys) = parse(&std::fs::read_to_string(format!("{}/_keys.json", args[3])).unwrap()).unwrap() else { panic!("keys") };
            let keys: Vec<String> = keys.into_iter().map(|k| if let Json::Str(s) = k { s } else { panic!("key") }).collect();
            // The oracle's synthetic bindings (oracle_specs.py): deterministic in the actor index.
            let v = |idx: i64, key: &str| (idx * 131 + key.bytes().map(i64::from).sum::<i64>() * 7 + key.len() as i64) % 60000;
            let corpse = |idx: i64| {
                let mut c = vec![
                    ("air_clip", Json::Int(v(idx, "air"))),
                    ("land_clip", Json::Int(v(idx, "land"))),
                    ("bounds", Json::List(["b0", "b1", "b2", "b3"].iter().map(|k| Json::Int(v(idx, k))).collect())),
                    ("spawn_offset", Json::List(["s0", "s1"].iter().map(|k| Json::Int(v(idx, k))).collect())),
                    ("bounce_factor", Json::Int((idx * 977) % 65537)),
                ];
                if idx % 3 == 1 {
                    c.extend([("breaker", Json::Bool(true)), ("smash_bounces", Json::Int(2)), ("hold_ticks", Json::Int(7))]);
                }
                if idx % 3 == 2 {
                    c.extend([("fling_speed", Json::Int(983040)), ("gravity", Json::Int(3145728)), ("remove_after_land", Json::Int(1))]);
                }
                Json::Obj(c.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
            };
            let nocorpse = ["WalkLeftRight", "GruzMother", "Hatcher", "HatcherBaby", "ZombieShield", "Blocker", "Pigeon", "Mawlek", "FalseKnight"];
            let (mut checked, mut bad) = (std::collections::BTreeMap::<String, usize>::new(), 0usize);
            for s in &scenes {
                let Some(Json::Str(name)) = get(s, "scene_name") else { panic!("scene row") };
                let Some(Json::Str(file)) = get(s, "file") else { panic!("scene row") };
                let Some(oracle) = parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap()).ok() else { panic!("oracle") };
                let sc = Scene::new(&source, &file).unwrap();
                let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
                let clips: Vec<Vec<(String, i64)>> = (0..rows.len()).map(|i| keys.iter().map(|k| (k.clone(), v(i as i64, k))).collect()).collect();
                let corpses: Vec<Option<Json>> = rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let kind = r.control.as_ref().map(|c| c.0.as_str());
                        if i % 2 == 0 && kind.is_some_and(|k| nocorpse.contains(&k)) { None } else { Some(corpse(i as i64)) }
                    })
                    .collect();
                let actor = |i: usize| SpecActor { row: &rows[i], clips: &clips[i], corpse: corpses[i].as_ref() };
                let mut chunks: Vec<(i64, Vec<usize>)> = Vec::new();
                for i in 0..rows.len() {
                    let c = (i as i64 * 5) % 4;
                    match chunks.iter_mut().find(|k| k.0 == c) {
                        Some(k) => k.1.push(i),
                        None => chunks.push((c, vec![i])),
                    }
                }
                let Some(Json::List(want_regions)) = get(&oracle, "regions") else { panic!("regions") };
                for (chunk, members) in &chunks {
                    let want = want_regions.iter().find(|r| get(r, "chunk_id") == Some(Json::Int(*chunk))).expect("oracle region");
                    let actors: Vec<SpecActor> = members.iter().map(|&i| actor(i)).collect();
                    let got = generated_actor_records(&actors);
                    let python_error = !matches!(get(want, "error"), Some(Json::Null));
                    match (&got, python_error) {
                        (Ok(g), false) => {
                            let mine = Json::List(g.iter().map(|(t, p)| Json::List(vec![Json::Str(t.clone()), p.to_json()])).collect());
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
                let regions: Vec<Region> = chunks.iter().map(|(c, m)| Region { chunk_id: *c, actors: m.iter().map(|&i| actor(i)).collect() }).collect();
                let want = get(&oracle, "bank").unwrap();
                match (scene_actor_bank(&regions), get(&want, "error")) {
                    (Ok((specs, placed)), None) => {
                        let mine = Json::Obj(vec![
                            ("specs".into(), Json::List(specs.into_iter().map(Json::Str).collect())),
                            ("placed".into(), Json::Obj(placed.into_iter().map(|(k, (i, p))| (k, Json::List(vec![Json::Int(i as i64), p.to_json()]))).collect())),
                        ]);
                        *checked.entry("banks".into()).or_default() += 1;
                        if want != mine {
                            bad += 1;
                            println!("{name}: bank differs");
                        }
                    }
                    (Err(_), Some(_)) => *checked.entry("agreed bank errors".into()).or_default() += 1,
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
            let field = |j: &Json, k: &str| -> Json { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_else(|| panic!("no {k}")) } else { panic!("not an object") } };
            let Json::Obj(consts) = field(&oracle, "constants") else { panic!("constants") };
            let constants: Vec<(String, f64)> = consts.into_iter().map(|(k, v)| (k, match v { Json::Int(i) => i as f64, Json::Float(f) => f, _ => panic!("constant") })).collect();
            let mut bad = 0;
            let values = hk_cook::vitals::source_vital_values(&source, &constants).unwrap();
            if values != field(&oracle, "values") {
                bad += 1;
                println!("vital values differ");
            }
            if Json::Str(hk_cook::vitals::generated_vital_params(&values).unwrap()) != field(&oracle, "vital") {
                bad += 1;
                println!("vital params differ");
            }
            let Json::Obj(nails) = field(&oracle, "nail") else { panic!("nail") };
            for (dt, text) in nails {
                if Json::Str(hk_cook::vitals::generated_nail_response_params(&constants, dt.parse().unwrap()).unwrap()) != text {
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
            let Json::List(cases) = parse(&std::fs::read_to_string(&args[2]).unwrap()).unwrap() else { panic!("cases") };
            let field = |j: &Json, k: &str| -> Json { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap() } else { panic!("not an object") } };
            let f = |j: &Json| match j { Json::Int(i) => *i as f64, Json::Float(x) => *x, _ => panic!("coordinate") };
            let (mut ok, mut agreed, mut bad) = (0, 0, 0);
            for (n, case) in cases.iter().enumerate() {
                let Json::List(points) = field(case, "points") else { panic!("points") };
                let points: Vec<(f64, f64)> = points.iter().map(|p| { let Json::List(p) = p else { panic!("pt") }; (f(&p[0]), f(&p[1])) }).collect();
                let Json::Int(limit) = field(case, "limit") else { panic!("limit") };
                let got = hk_cook::polygons::bounded_polygons(&points, limit as usize);
                match (got, field(case, "error")) {
                    (Ok(pieces), Json::Null) => {
                        let mine = Json::List(pieces.iter().map(|p| Json::List(p.iter().map(|q| Json::List(vec![Json::Float(q.0), Json::Float(q.1)])).collect())).collect());
                        if mine == field(case, "pieces") { ok += 1 } else { bad += 1; println!("case {n}: pieces differ") }
                    }
                    (Err(e), Json::Str(want)) => {
                        if e == want || (want == "KeyError" && e.starts_with("KeyError")) { agreed += 1 } else { bad += 1; println!("case {n}: error {e:?}, python {want:?}") }
                    }
                    (r, want) => { bad += 1; println!("case {n}: rust ok {}, python error {want:?}", r.is_ok()) }
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
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions { f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone()) } else { None }) else { panic!("no scenes") };
            let get = |j: &Json, k: &str| -> Option<Json> { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()) } else { None } };
            let (mut checked, mut bad) = (std::collections::BTreeMap::<String, usize>::new(), 0usize);
            for s in &scenes {
                let (Some(Json::Str(name)), Some(Json::Str(file))) = (get(s, "scene_name"), get(s, "file")) else { panic!("scene row") };
                let oracle = parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap()).unwrap();
                let sc = Scene::new(&source, &file).unwrap();
                let results = [
                    ("hazards", hk_cook::static_sources::hazard_sources(&sc, None).map(Json::List)),
                    ("shrooms", hk_cook::static_sources::shroom_sources(&sc, None).map(Json::List)),
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
                        (Err(_), Some(_)) => *checked.entry(format!("{key} agreed errors")).or_default() += 1,
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
            let field = |j: &Json, k: &str| -> Json { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_else(|| panic!("no {k}")) } else { panic!("not an object") } };
            let ints = |j: &Json| -> Vec<i64> { let Json::List(l) = j else { panic!("list") }; l.iter().map(|v| if let Json::Int(i) = v { *i } else { panic!("int") }).collect() };
            let list = |j: Json| -> Vec<Json> { if let Json::List(l) = j { l } else { panic!("list") } };
            let mut bad = 0;
            let (mut cut, mut refused) = (0, 0);
            for case in list(field(&oracle, "decompose")) {
                let (Json::Int(w), Json::Int(h), Json::Bool(streamed), Json::Str(plane)) = (field(&case, "w"), field(&case, "h"), field(&case, "streamed"), field(&case, "plane")) else { panic!("case") };
                let plane: Vec<u8> = plane.bytes().map(|b| b - b'0').collect();
                match (fka::decompose(&plane, w as usize, h as usize, streamed), field(&case, "error")) {
                    (Ok(parts), Json::Null) => {
                        let mine = Json::List(parts.iter().map(|p| Json::List(p.iter().map(|&v| Json::Int(v)).collect())).collect());
                        if mine == field(&case, "parts") { cut += 1 } else { bad += 1; println!("decompose {w}x{h} streamed={streamed}: parts differ") }
                    }
                    (Err(e), Json::Str(want)) if e == want => refused += 1,
                    (r, want) => { bad += 1; println!("decompose {w}x{h}: rust ok {}, python error {want:?}", r.is_ok()) }
                }
            }
            for case in list(field(&oracle, "part_box")) {
                let b = ints(&field(&case, "box"));
                let (Json::Int(w), Json::Int(h)) = (field(&case, "w"), field(&case, "h")) else { panic!("w h") };
                let r = ints(&field(&case, "rect"));
                let mine = fka::part_box([b[0], b[1], b[2], b[3]], w, h, [r[0], r[1], r[2], r[3]]);
                if mine.to_vec() != ints(&field(&case, "out")) { bad += 1; println!("part_box differs") }
            }
            // The False Knight's scene.
            let regions = parse(&std::fs::read_to_string(&args[4]).unwrap()).unwrap();
            let Json::List(scenes) = field(&regions, "scenes") else { panic!("scenes") };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes.iter().map(|s| {
                let (Json::Str(file), Json::Str(name)) = (field(s, "file"), field(s, "scene_name")) else { panic!("scene") };
                let f = |j: &Json| match j { Json::Int(i) => *i as f64, Json::Float(x) => *x, _ => panic!("bound") };
                let Json::List(b) = field(s, "runtime_bounds") else { panic!("bounds") };
                (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
            }).collect();
            let sc = Scene::new(&source, "level46").unwrap();
            let rows = hk_cook::actors::scan(&sc, &source, &catalogue).unwrap();
            let fk = rows.iter().find(|r| r.control.as_ref().is_some_and(|c| c.0 == "FalseKnight")).expect("False Knight");
            let wave = fka::shockwave_source(&sc, &source, fk.game_object).unwrap();
            let want_wave = field(&oracle, "wave");
            if wave.params != field(&want_wave, "params") || Json::Str(wave.clip_name.clone()) != field(&want_wave, "clip") || Json::Str(wave.library.sid()) != field(&want_wave, "library") {
                bad += 1;
                println!("shockwave differs");
            }
            let objects = fka::source_objects(&sc, &source, fk).unwrap();
            if objects != field(&oracle, "objects") { bad += 1; println!("objects differ: {}", hk_cook::pyjson::dumps(&objects)); }
            let ti = field(&oracle, "table_inputs");
            let sprite_rows: Vec<(i64, i64, bool)> = list(field(&ti, "sprite_rows")).iter().map(|r| { let v = ints(r); (v[0], v[1], v[2] != 0) }).collect();
            let clip_rows: Vec<fka::ClipRow> = list(field(&ti, "clip_rows")).iter().map(|r| { let Json::List(l) = r else { panic!("row") }; let v = ints(&Json::List(l[..5].to_vec())); let Json::Str(n) = &l[5] else { panic!("name") }; (v[0], v[1], v[2], v[3], v[4], n.clone()) }).collect();
            let floor_rows: Vec<(String, Vec<i64>)> = list(field(&ti, "floor_rows")).iter().map(|r| { let Json::List(l) = r else { panic!("row") }; let Json::Str(n) = &l[0] else { panic!("state") }; (n.clone(), ints(&l[1])) }).collect();
            let table = fka::rust_table(123, &sprite_rows, &clip_rows, &ints(&field(&ti, "sequence")), &floor_rows, &objects).unwrap();
            if Json::Str(table) != field(&oracle, "rust_table") { bad += 1; println!("rust_table differs"); }
            let bindings = vec![
                fka::Binding { name: "FK_A".into(), doc: "first doc".into(), rows: vec![(1, 2), (3, 4)], boxes: None },
                fka::Binding { name: "FK_B".into(), doc: "boxes doc".into(), rows: vec![], boxes: Some(vec![[1, 2, 3, 4], [5, 6, 7, 8]]) },
            ];
            if Json::Str(fka::rust_bindings(&bindings, 7)) != field(&oracle, "rust_bindings") { bad += 1; println!("rust_bindings differs"); }
            let rb = field(&oracle, "region_bindings");
            let rows_in: Vec<(i64, Vec<String>)> = list(field(&rb, "rows")).iter().map(|r| { let Json::Int(c) = field(r, "chunk_id") else { panic!("chunk") }; (c, list(field(r, "edge_sources")).into_iter().map(|s| if let Json::Str(s) = s { s } else { panic!("src") }).collect()) }).collect();
            let draws = field(&rb, "draws");
            let result = fka::region_bindings(&rows_in, &objects, &|c| Ok(list(field(&draws, &c.to_string())).iter().map(|d| if let Json::Str(s) = field(d, "source") { s } else { panic!("src") }).collect())).unwrap();
            let mine = Json::Obj(result.iter().map(|b| (b.name.clone(), match &b.boxes {
                Some(boxes) => Json::Obj(vec![("doc".into(), Json::Str(b.doc.clone())), ("boxes".into(), Json::List(boxes.iter().map(|x| Json::List(x.iter().map(|&v| Json::Int(v)).collect())).collect()))]),
                None => Json::Obj(vec![("doc".into(), Json::Str(b.doc.clone())), ("rows".into(), Json::List(b.rows.iter().map(|&(a, c)| Json::List(vec![Json::Int(a), Json::Int(c)])).collect()))]),
            })).collect());
            if mine != field(&rb, "result") { bad += 1; println!("region_bindings differ"); }
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
                let Json::Obj(top) = report else { panic!("report") };
                for (key, list) in top.iter_mut() {
                    if key == "scenes" || key == "regions" {
                        let Json::List(items) = list else { panic!("list") };
                        for item in items {
                            let Json::Obj(f) = item else { panic!("item") };
                            f.retain(|x| x.0 != "static_pogo" && x.0 != "pogo_targets");
                        }
                    }
                }
            };
            strip(&mut report);
            let scene_count = { let Json::Obj(top) = &report else { panic!("report") }; let Some((_, Json::List(s))) = top.iter().find(|x| x.0 == "scenes") else { panic!("scenes") }; s.len() };
            hk_cook::static_sources::postpack_pogo(&mut report, &source, scene_count).unwrap();
            let pick = |report: &Json, list: &str, key: &str| -> Vec<Option<Json>> {
                let Json::Obj(top) = report else { panic!("report") };
                let Some((_, Json::List(items))) = top.iter().find(|x| x.0 == list) else { panic!("list") };
                items.iter().map(|i| if let Json::Obj(f) = i { f.iter().find(|x| x.0 == key).map(|x| x.1.clone()) } else { None }).collect()
            };
            let mut bad = 0;
            for (list, key) in [("scenes", "static_pogo"), ("regions", "pogo_targets")] {
                let (mine, want) = (pick(&report, list, key), pick(&original, list, key));
                bad += mine.iter().zip(&want).filter(|(a, b)| a != b).count();
                if let Some(i) = mine.iter().zip(&want).position(|(a, b)| a != b) {
                    println!("first difference at {list}[{i}]:\n rust:   {}\n python: {}", mine[i].as_ref().map(hk_cook::pyjson::dumps_sorted_compact).unwrap_or_default(), want[i].as_ref().map(hk_cook::pyjson::dumps_sorted_compact).unwrap_or_default());
                }
                println!("{list}.{key}: {} entries, {} differ", want.len(), mine.iter().zip(&want).filter(|(a, b)| a != b).count());
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
            let Some(Json::List(scenes)) = (if let Json::Obj(f) = &regions { f.iter().find(|k| k.0 == "scenes").map(|k| k.1.clone()) } else { None }) else { panic!("no scenes") };
            let get = |j: &Json, k: &str| -> Option<Json> { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()) } else { None } };
            let catalogue: Vec<(String, [f64; 4], String)> = scenes
                .iter()
                .map(|s| {
                    let (Some(Json::Str(file)), Some(Json::Str(name)), Some(Json::List(b))) = (get(s, "file"), get(s, "scene_name"), get(s, "runtime_bounds")) else { panic!("scene row") };
                    let f = |j: &Json| match j { Json::Int(i) => *i as f64, Json::Float(x) => *x, _ => panic!("bound") };
                    (file, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])], name)
                })
                .collect();
            let (mut regions_checked, mut actors_checked, mut bad) = (0, 0, 0);
            for s in &scenes {
                let (Some(Json::Str(name)), Some(Json::Str(file))) = (get(s, "scene_name"), get(s, "file")) else { panic!("scene row") };
                let Json::List(oracle) = parse(&std::fs::read_to_string(format!("{}/{name}.json", args[3])).unwrap()).unwrap() else { panic!("oracle") };
                let sc = Scene::new(&source, &file).unwrap();
                for region in &oracle {
                    let Some(Json::List(b)) = get(region, "bounds") else { panic!("bounds") };
                    let f = |j: &Json| match j { Json::Int(i) => *i as f64, Json::Float(x) => *x, _ => panic!("bound") };
                    let rows = hk_cook::actors::scan_in(&sc, &source, &catalogue, [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])]).unwrap();
                    let mine = Json::List(rows.iter().map(|r| Json::List(vec![Json::Str(r.source.clone()), Json::Bool(r.supported), r.control.as_ref().map_or(Json::Null, |c| Json::Str(c.0.clone()))])).collect());
                    regions_checked += 1;
                    actors_checked += rows.len();
                    if get(region, "actors") != Some(mine) {
                        bad += 1;
                        println!("{name} chunk {:?}: actors differ", get(region, "chunk_id"));
                    }
                }
            }
            println!("checked {regions_checked} regions, {actors_checked} actors, {bad} mismatches");
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
            let field = |j: &Json, k: &str| -> Json { if let Json::Obj(f) = j { f.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_else(|| panic!("no {k}")) } else { panic!("not an object") } };
            let list = |j: Json| -> Vec<Json> { if let Json::List(l) = j { l } else { panic!("list") } };
            let int = |j: &Json| -> i64 { if let Json::Int(i) = j { *i } else { panic!("int") } };
            let hex = |j: &Json| -> Vec<u8> { let Json::Str(s) = j else { panic!("hex") }; (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect() };
            let tohex = |b: &[u8]| Json::Str(b.iter().map(|x| format!("{x:02x}")).collect());
            let mut bad = 0;
            let mut counts = std::collections::BTreeMap::<&str, usize>::new();
            for case in list(field(&oracle, "dense_pack")) {
                let rects: Vec<(i64, i64, usize)> = list(field(&case, "rects")).iter().map(|r| { let v = list(r.clone()); (int(&v[0]), int(&v[1]), int(&v[2]) as usize) }).collect();
                match (packer::dense_pack(&rects), field(&case, "error")) {
                    (Ok((pages, placements)), Json::Null) => {
                        let mine = Json::List(placements.iter().map(|p| Json::List(vec![Json::Int(p.0 as i64), Json::Int(p.1 as i64), Json::Int(p.2), Json::Int(p.3), Json::Int(p.4), Json::Int(p.5)])).collect());
                        *counts.entry("dense_pack").or_default() += 1;
                        if int(&field(&case, "pages")) != pages as i64 || mine != field(&case, "placements") { bad += 1; println!("dense_pack differs"); }
                    }
                    (Err(_), Json::Str(_)) => *counts.entry("dense_pack refusals").or_default() += 1,
                    (r, _) => { bad += 1; println!("dense_pack error disagreement (rust ok {})", r.is_ok()); }
                }
            }
            for case in list(field(&oracle, "covers")) {
                let (w, h) = (int(&field(&case, "w")) as usize, int(&field(&case, "h")) as usize);
                match (alpha_cover::compute_record(w, h, &hex(&field(&case, "palette")), &hex(&field(&case, "pixels"))), field(&case, "error")) {
                    (Ok(rec), Json::Null) => { *counts.entry("covers").or_default() += 1; if tohex(&rec) != field(&case, "record") { bad += 1; println!("cover {w}x{h} differs"); } }
                    (Err(_), Json::Str(_)) => *counts.entry("cover refusals").or_default() += 1,
                    (r, _) => { bad += 1; println!("cover error disagreement (rust ok {})", r.is_ok()); }
                }
            }
            for case in list(field(&oracle, "canonical_black")) {
                let (p, x) = canonical_black(&hex(&field(&case, "palette")), &hex(&field(&case, "pixels")));
                *counts.entry("canonical_black").or_default() += 1;
                if tohex(&p) != field(&case, "out_palette") || tohex(&x) != field(&case, "out_pixels") { bad += 1; println!("canonical_black differs"); }
            }
            for (n, case) in list(field(&oracle, "atlas")).iter().enumerate() {
                let params = field(case, "params");
                let max_cluts = match field(&params, "max_cluts") { Json::Int(i) => Some(i as usize), _ => None };
                let b = |k: &str| matches!(field(&params, k), Json::Bool(true));
                let mut atlas = Atlas::new(b("deduplicate"), int(&field(&params, "max_pages")) as usize, int(&field(&params, "max_textures")) as usize, b("alpha_covers"), max_cluts);
                for op in list(field(case, "ops")) {
                    let r = atlas.add_quantized(int(&field(&op, "w")) as usize, int(&field(&op, "h")) as usize, &hex(&field(&op, "palette")), &hex(&field(&op, "pixels")), matches!(field(&op, "streamed"), Json::Bool(true)), matches!(field(&op, "unique"), Json::Bool(true)));
                    match (r, field(&op, "index")) {
                        (Ok(i), Json::Int(want)) if i as i64 == want => {}
                        (Err(_), Json::Null) => {}
                        _ => { bad += 1; println!("atlas {n}: add_quantized differs"); }
                    }
                }
                if let Json::Obj(grids) = field(case, "grids") {
                    for (k, v) in grids { let v = list(v); atlas.grids.insert(k.parse().unwrap(), (int(&v[0]) as usize, int(&v[1]) as usize)); }
                }
                let rm = Json::List(atlas.request_map.iter().map(|&i| Json::Int(i as i64)).collect());
                let qz = Json::List(atlas.quantized.iter().map(|q| Json::List(vec![Json::Int(q.0 as i64), Json::Int(q.1 as i64), tohex(&q.2), tohex(&q.3)])).collect());
                if rm != field(case, "request_map") || qz != field(case, "quantized") { bad += 1; println!("atlas {n}: canonical textures differ"); }
                match (atlas.pack(), field(case, "error")) {
                    (Ok(()), Json::Null) => {
                        *counts.entry("atlas packs").or_default() += 1;
                        let entries = Json::List(atlas.entries.iter().map(|e| Json::List(e.unwrap().iter().map(|&v| Json::Int(v)).collect())).collect());
                        let palettes = Json::List(atlas.palettes.iter().map(|p| tohex(p)).collect());
                        let sha = |d: &[u8]| { use sha2::{Digest, Sha256}; Json::Str(Sha256::digest(d).iter().map(|x| format!("{x:02x}")).collect()) };
                        let pages = Json::List(atlas.pages.iter().map(|p| sha(p)).collect());
                        let ok = entries == field(case, "entries") && palettes == field(case, "palettes") && pages == field(case, "pages") && sha(&atlas.stream) == field(case, "stream")
                            && int(&field(case, "stream_len")) as usize == atlas.stream.len() && int(&field(case, "cluts")) as usize == atlas.cluts && int(&field(case, "animation_bytes")) as usize == atlas.animation_bytes && int(&field(case, "alpha_cover_bytes")) as usize == atlas.alpha_cover_bytes;
                        if !ok { bad += 1; println!("atlas {n}: pack differs"); }
                    }
                    (Err(e), Json::Str(want)) => { *counts.entry("atlas refusals").or_default() += 1; if e != want { bad += 1; println!("atlas {n}: refusal {e:?}, python {want:?}"); } }
                    (r, want) => { bad += 1; println!("atlas {n}: rust ok {}, python error {want:?}", r.is_ok()); }
                }
            }
            println!("checked {counts:?}, {bad} mismatches");
            if bad != 0 {
                std::process::exit(1);
            }
        }
        other => panic!("unknown mode {other}"),
    }
}
