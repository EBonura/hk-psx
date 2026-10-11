//! Bounded actual Rust recognizer audit, without cooking or admitting new scenes.
use hk_cook::{
    actor_art::{ArtActor, ArtBank},
    actor_specs::{generated_actor_records, SpecActor},
    actors,
    atlas::Atlas,
    effects_art,
    pyjson::dumps,
    quantize, runner,
};
use hk_unity::{scene::Scene, Source};
use serde_json::json;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: mossman-runner-recognition ORIGINAL_DATA OUTPUT_JSON".into());
    }
    let source = Source::new(Path::new(&args[1]))?;
    let mut rows = Vec::new();
    let mut art_rows = Vec::new();
    for (file, gid) in [
        ("level128", 736),
        ("level134", 110),
        ("level135", 808),
        ("level136", 510),
        ("level136", 1082),
    ] {
        let scene = Scene::new(&source, file)?;
        let position = scene.point(gid, 0.0, 0.0, 0.0)?;
        let found = runner::recognize(&scene, &source, gid, position)
            .map_err(|e| format!("{file}:{gid}: {e}"))?;
        let control: serde_json::Value = serde_json::from_str(&dumps(&found.control))?;
        rows.push(json!({"game_object_source":format!("{file}:{gid}"),"control":control}));
    }
    for (file, selected) in [
        ("level128", vec![736]),
        ("level134", vec![110]),
        ("level135", vec![808]),
        ("level136", vec![510, 1082]),
    ] {
        let scene = Scene::new(&source, file)?;
        let all_rows = actors::scan(&scene, &source, &[])?;
        let room_population: Vec<_> = all_rows.iter().map(|a| json!({"game_object":a.game_object,"supported":a.supported,"control":a.control.as_ref().map(|(_,c)|serde_json::from_str::<serde_json::Value>(&dumps(c)).unwrap())})).collect();
        let actor_rows: Vec<_> = all_rows
            .into_iter()
            .filter(|a| selected.contains(&a.game_object))
            .collect();
        if actor_rows.len() != selected.len() || actor_rows.iter().any(|a| !a.supported) {
            return Err(
                format!("{file}: selected actors not admitted by actual actor scan").into(),
            );
        }
        let mut art: Vec<_> = actor_rows.iter().map(ArtActor::new).collect();
        let mut bank = ArtBank::new(Atlas::standard());
        hk_cook::actor_art::append_actor_art_bodies(
            &scene,
            &source,
            &mut art,
            &mut bank,
            &quantize::atlas_quantizer,
        )?;
        effects_art::append_corpse_art(
            &source,
            &scene,
            &mut art,
            &mut bank,
            &quantize::atlas_quantizer,
        )?;
        if art.iter().any(|a| !a.supported || a.corpse.is_none()) {
            return Err(format!("{file}: selected actor body/corpse art refused").into());
        }
        bank.atlas.pack()?;
        let specs: Vec<_> = art
            .iter()
            .map(|a| SpecActor {
                row: a.row,
                clips: &a.clips,
                corpse: a.corpse.as_ref(),
            })
            .collect();
        let generated = generated_actor_records(&specs)?;
        art_rows.push(json!({"scene_file":file,"selected_game_objects":selected,
            "complete_health_manager_population":room_population,
            "scope":"Selected actors only, isolated body/corpse art; excludes other room contents and source audio. Not an incremental whole-room bank growth or fit measurement.",
            "frames":bank.frames.len(),"clips":bank.clips.len(),"textures":bank.atlas.quantized.len(),"cluts":bank.atlas.cluts,
            "texture_page_bytes":bank.atlas.pages.iter().map(Vec::len).sum::<usize>(),"palette_bytes":bank.atlas.palettes.iter().map(Vec::len).sum::<usize>(),"stream_bytes":bank.atlas.stream.len(),
            "generated_specs":generated.iter().map(|(s,p)|json!({"spec":s,"placement":serde_json::from_str::<serde_json::Value>(&dumps(&p.to_json())).unwrap()})).collect::<Vec<_>>()
        }));
    }
    let target = Path::new(&args[2]);
    let next = target.with_extension("json.next");
    std::fs::write(
        &next,
        serde_json::to_vec_pretty(
            &json!({"instances":rows,"isolated_art_spec_preflight":art_rows,"terrain_cook_performed":false,"guest_build_performed":false,"runtime_parity":"unknown"}),
        )?,
    )?;
    std::fs::rename(next, target)?;
    Ok(())
}
