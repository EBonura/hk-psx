//! hk-psx's Rust cookers. Run from the repository root:
//!   hk-cook props|break-effects|scene-sfx|xa-music|cook-audio|geo-audio|focus-audio|cook-music|ambience|area-music|runner-audio [--source <hollow_knight_Data>]
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
        Some("cook-music") => hk_cook::music_report::main(&root, source.as_deref()),
        Some("ambience") => hk_cook::ambience::main(&root, source.as_deref()),
        Some("area-music") => hk_cook::area_music::main(&root, source.as_deref()),
        Some("runner-audio") => hk_cook::runner_audio::main(&root, &args[2..], source.as_deref()),
        Some("props") => hk_cook::props::main(&root, source.as_deref()),
        Some("break-effects") => hk_cook::break_effects::main(&root, source.as_deref()),
        Some("scene-sfx") => hk_cook::scene_sfx::main(&root, source.as_deref()),
        Some("xa-music") => hk_cook::xa_music::main(&root, source.as_deref()),
        Some("opaque-tiles") => hk_cook::opaque_tiles::main(&root, &args[2..]),
        Some("opaque-groups") => hk_cook::opaque_groups::main(&root),
        Some("gpu-census") => hk_cook::gpu_census::main(&root, &args[2..]),
        Some("scene-certificates") => hk_cook::scene_certificates::main(&root, &args[2..]),
        _ => Err("usage: hk-cook props|break-effects|scene-sfx|xa-music|cook-audio|geo-audio|focus-audio|cook-music|ambience|area-music|runner-audio [--source DIR] | opaque-tiles|opaque-groups|scene-certificates|gpu-census".into()),
    };
    if let Err(e) = result {
        eprintln!("hk-cook: {e}");
        std::process::exit(1);
    }
}
