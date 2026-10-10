//! Behaviour parity: the guest enemy module against the original, scene by scene.
//!
//! The guest's `game/src/enemies.rs` is compiled natively here, with the hardware-free stand-ins the
//! enemy tests use (`shared/hk-sim/tests/common/enemy_stubs.rs`), and run over the real cooked
//! rooms, scene banks and actor catalogue. Its per-tick state is compared with a per-frame trace of
//! the original recorded by `tools/hkref` (see README.md).
//!
//! Inputs (all read-only, ignored build outputs, selected by environment):
//!   HKBP_DATA    cooked data directory holding regions.json and regions/chunk_N.hk
//!   HKBP_BANKS   directory of scene_N.hkwm world-metadata banks
//!   HKBP_REGIONS_RS (build time) the cooked regions.rs the actor catalogue is lifted from
#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unexpected_cfgs
)]
use hk_sim::*;
include!("../../../shared/hk-sim/tests/common/enemy_stubs.rs");
#[path = "../../../game/src/enemies.rs"]
mod enemies;
mod catalogue {
    include!(concat!(env!("OUT_DIR"), "/scene_actors.rs"));
}

mod approach;
mod compare;
mod harass;
mod index_check;
mod og_trace;
mod plot;
mod poke;
mod replay;
mod scan;
mod static_check;
mod tour;
mod world_data;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("probe") => replay::probe(&args[1..]),
        Some("clips") => replay::clips(&args[1..]),
        Some("edges") => replay::edges(&args[1..]),
        Some("static") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            static_check::run(
                &raw.into_iter()
                    .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                    .collect(),
            );
        }
        Some("index-check") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            index_check::run(
                &raw.into_iter()
                    .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                    .collect(),
            );
        }
        Some("approach") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            let names = raw
                .into_iter()
                .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                .collect();
            approach::report(
                std::path::Path::new(&args[1]),
                &names,
                args.get(2).map(String::as_str),
            );
        }
        Some("plot") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            let names = raw
                .into_iter()
                .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                .collect();
            plot::run(std::path::Path::new(&args[1]), &names, &args[2..]);
        }
        Some("harass") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            harass::run(
                &raw.into_iter()
                    .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                    .collect(),
                args.get(1).map_or(3000, |t| t.parse().unwrap()),
            );
        }
        Some("tour") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            let names = raw
                .into_iter()
                .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                .collect();
            tour::report(
                std::path::Path::new(&args[1]),
                &names,
                args.get(2).map(String::as_str),
            );
        }
        Some("scan") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            let names = raw
                .into_iter()
                .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                .collect();
            scan::run(&names, args.get(1).map_or(2400, |t| t.parse().unwrap()));
        }
        Some("poke") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            let names = raw
                .into_iter()
                .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                .collect();
            poke::report(
                std::path::Path::new(&args[1]),
                &names,
                args.get(2).map(String::as_str),
            );
        }
        Some("dump") => {
            let names_path = std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            let names = raw
                .into_iter()
                .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                .collect();
            compare::dump(std::path::Path::new(&args[1]), &names, &args[2..]);
        }
        Some("compare") => {
            // compare RUN_DIR [SCENE_NAME]
            let names_path =
                std::env::var("HKBP_SCENE_NAMES").expect("HKBP_SCENE_NAMES: scene id -> name json");
            let raw: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&std::fs::read_to_string(names_path).unwrap()).unwrap();
            let names = raw
                .into_iter()
                .map(|(k, v)| (k.parse::<usize>().unwrap(), v))
                .collect();
            compare::report(
                std::path::Path::new(&args[1]),
                args.get(2).map(String::as_str),
                &names,
            );
        }
        _ => {
            eprintln!("usage: hk-behaviour-parity probe SCENE_ID [TICKS]");
            std::process::exit(2);
        }
    }
}
