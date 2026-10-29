//! hk-psx's Rust cookers. Run from the repository root:
//!   hk-cook actor-persistence|dialogue-art|props|break-effects|scene-sfx|xa-music|cook-audio|geo-audio|focus-audio|cook-music|ambience|area-music|runner-audio [--source <hollow_knight_Data>]
//!   hk-cook opaque-tiles [--grid-shift 2|3] [--variant]|opaque-groups
//!   hk-cook scene-certificates [--output DIR] [--report PATH] [--manifest PATH]
//!   hk-cook gpu-census [--profile DIR | --camera X Y] [--region N] [--top N] [--every N]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = std::env::current_dir().expect("cwd");
    let source = args
        .iter()
        .position(|a| a == "--source")
        .map(|i| std::path::PathBuf::from(&args[i + 1]));
    let result = match args.get(1).map(String::as_str) {
        Some("cook-audio") => hk_cook::cook_audio::main(&root, source.as_deref()),
        Some("geo-audio") => hk_cook::geo_audio::main(&root, source.as_deref()),
        Some("focus-audio") => hk_cook::focus_audio::main(&root, source.as_deref()),
        Some("ability-params") => hk_cook::ability_params::main(&root, None),
        Some("cook-music") => hk_cook::music_report::main(&root, source.as_deref()),
        Some("ambience") => hk_cook::ambience::main(&root, source.as_deref()),
        Some("area-music") => hk_cook::area_music::main(&root, source.as_deref()),
        Some("runner-audio") => hk_cook::runner_audio::main(&root, &args[2..], source.as_deref()),
        Some("actor-persistence") => hk_cook::actor_persistence::main(&root, source.as_deref()),
        Some("battle-waves") => hk_cook::battle_waves::main(&root, source.as_deref()),
        Some("crossroads22-wave-prepare") => hk_cook::crossroads22_wave::prepare(&root).map_err(|e| e.to_string()),
        Some("crossroads22-wave-finish") => hk_cook::crossroads22_wave::finish(&root).map_err(|e| e.to_string()),
        Some("crossroads22-wave-arrivals") => hk_cook::crossroads22_wave::bind_arrivals(&root).map_err(|e| e.to_string()),
        Some("crossroads22-bridge") => hk_cook::crossroads22_bridge::main(&root, source.as_deref(), &args[2..]).map_err(|e|e.to_string()),
        Some("crawler-recoil") => hk_cook::crawler_recoil::bind_generated_specs(&root),
        Some("mossman-bind") => hk_cook::mossman_scene::bind_generated_specs(&root).map_err(|e| e.to_string()),
        Some("shaman-runner-scene") => hk_cook::shaman_runner_scene::cook(&root).map_err(|e| e.to_string()),
        Some("shaman-runner-bind") => hk_cook::shaman_runner_scene::bind_generated_specs(&root).map_err(|e| e.to_string()),
        Some("hornet-measure") => hk_cook::hornet_scene::measure(&root).map_err(|e| e.to_string()),
        Some("dialogue-art") => hk_cook::dialogue_art::main(&root, source.as_deref()),
        Some("title-style") => hk_cook::title_style::main(&root, source.as_deref(), &args[2..].iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && args[2 + i - 1] == "--source")).map(|(_, a)| a.clone()).collect::<Vec<_>>()),
        Some("hornet-bank") => hk_cook::hornet_scene::bank_report(&root, args.iter().position(|a| a == "--sheet").map(|i| std::path::Path::new(&args[i + 1]))).map_err(|e| e.to_string()),
        Some("boss-survey") => hk_cook::boss_measure::survey(&root, &args[2..]).map_err(|e| e.to_string()),
        Some("boss-measure") => hk_cook::boss_measure::main(&root, &args[2..]).map_err(|e| e.to_string()),
        Some("props") => hk_cook::props::main(&root, source.as_deref()),
        Some("break-effects") => hk_cook::break_effects::main(&root, source.as_deref()),
        Some("scene-sfx") => hk_cook::scene_sfx::main(&root, source.as_deref()),
        Some("xa-music") => hk_cook::xa_music::main(&root, source.as_deref()),
        Some("opaque-tiles") => hk_cook::opaque_tiles::main(&root, &args[2..]),
        Some("opaque-groups") => hk_cook::opaque_groups::main(&root),
        Some("gpu-census") => hk_cook::gpu_census::main(&root, &args[2..]),
        Some("scene-certificates") => hk_cook::scene_certificates::main(&root, &args[2..]),
        _ => Err("usage: hk-cook actor-persistence|props|break-effects|scene-sfx|xa-music|cook-audio|geo-audio|focus-audio|cook-music|ambience|area-music|runner-audio [--source DIR] | opaque-tiles|opaque-groups|scene-certificates|gpu-census".into()),
    };
    if let Err(e) = result {
        eprintln!("hk-cook: {e}");
        std::process::exit(1);
    }
}
