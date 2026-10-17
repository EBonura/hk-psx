//! The tables: TSV files of names, counts and seconds, and AUDIO_CENSUS.md.

use crate::model::*;
use crate::scan::World;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// Usable SPU RAM: 512 KiB less the capture buffers and silent loop at the
/// bottom (the SDK's first sample address, 0x1010) and the 16 bytes the reverb
/// work area parks on at the top (docs/BUDGET.md).
pub const SPU_USABLE: u64 = 524_288 - 4_112 - 16;
/// The music ring that stays resident while area music streams (docs/BUDGET.md).
pub const RING: u64 = 16_384;
/// A clip reached by this share of scenes is part of the Knight, the UI and the pickups.
const GLOBAL_SHARE: f64 = 0.80;
/// Reached by this share of scenes: a regional bank (grass, walls, splats), not scene-local.
const WIDE_SHARE: f64 = 0.15;
const LONG: f64 = 15.0;
/// (label, one-shot rate, ambience rate): the cap, Manny's halving, what the port plays at today.
pub const PROFILES: [(&str, i64, i64); 3] = [
    ("full", 22050, 22050),
    ("half", 11025, 11025),
    ("port", 5512, 4000),
];

/// Layers audible under each mixer snapshot, from the cooked mixer values
/// (area-music.json `snapshot_layer_db`, audible above -40 dB).
fn snapshot_layers(name: &str) -> Option<Vec<usize>> {
    match name {
        "Silent" => Some(vec![]),
        "Normal" | "Normal Soft" => Some(vec![0, 3]),
        "Sub Area" => Some(vec![3]),
        "Tension Only" => Some(vec![4]),
        "Action and Sub" => Some(vec![2, 3]),
        _ => None,
    }
}

fn put(out: &Path, name: &str, text: String) -> Result<(), String> {
    std::fs::write(out.join(name), text).map_err(|e| e.to_string())
}

fn kb(b: u64) -> String {
    format!("{:.1}", b as f64 / 1024.0)
}
fn mb(b: u64) -> String {
    format!("{:.1}", b as f64 / 1_048_576.0)
}
fn mins(s: f64) -> String {
    format!("{:.1}", s.max(0.0) / 60.0)
}
fn pct(v: &mut [u64], p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort();
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    Music,
    Atmos,
    Loop,
    Persistent,
    Wide,
    Local,
    Unreached,
}

struct Piece {
    names: Vec<String>,
    layers: Vec<usize>,
    cue_ix: Vec<usize>,
    /// Distinct audible layer sets (premix variants).
    variants: BTreeSet<Vec<usize>>,
    guess: bool,
    scenes: BTreeSet<usize>,
    base: bool,
    dlc: bool,
    locs: BTreeSet<Loc>,
    xa_ok: bool,
    traversal_theme: bool,
}

pub fn write(w: &World, out: &Path) -> Result<(), String> {
    let n_scenes = w.scenes.len();
    let clips = &w.clips;
    // Canonical clip per sound: the same name and length is one sound whichever file holds it.
    let mut first: BTreeMap<String, usize> = BTreeMap::new();
    let canon: Vec<usize> = clips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            *first
                .entry(format!(
                    "{}|{}",
                    c.name.to_lowercase(),
                    (c.seconds * 1000.0).round()
                ))
                .or_insert(i)
        })
        .collect();
    let dup_names = clips.len() - first.len();

    let mut reach = vec![0usize; clips.len()];
    for s in &w.scenes {
        for &c in s.clips.keys() {
            reach[c] += 1;
        }
    }
    let music_set: BTreeSet<usize> = w
        .cues
        .iter()
        .flat_map(|c| c.layers.iter().flatten().map(|&i| canon[i]))
        .collect();
    let atmos_set: BTreeSet<usize> = w
        .atmos_channels
        .iter()
        .flatten()
        .map(|&i| canon[i])
        .collect();
    let loop_set: BTreeSet<usize> = w
        .scenes
        .iter()
        .flat_map(|s| s.loop_clips.iter().map(|&i| canon[i]))
        .collect();
    let g_min = (GLOBAL_SHARE * n_scenes as f64).ceil() as usize;
    let w_min = (WIDE_SHARE * n_scenes as f64).ceil() as usize;
    // reach per canonical sound
    let mut sreach: BTreeMap<usize, usize> = BTreeMap::new();
    for s in &w.scenes {
        let set: BTreeSet<usize> = s.clips.keys().map(|&c| canon[c]).collect();
        for c in set {
            *sreach.entry(c).or_default() += 1;
        }
    }
    // The scenes that carry the always-resident prefabs: the Knight, the menus and the UI.
    let mut persistent_set: BTreeSet<usize> = BTreeSet::new();
    for s in w.scenes.iter() {
        if s.name == "Knight_Pickup" {
            persistent_set.extend(s.clips.keys().map(|&c| canon[c]));
        }
        if s.name == "Menu_Title" {
            // the title scene also carries its own music and ambience; only the UI sounds are always loaded
            persistent_set.extend(
                s.clips
                    .keys()
                    .filter(|&&c| {
                        let n = clips[c].name.to_lowercase();
                        n.starts_with("ui_") || n.starts_with("menu_")
                    })
                    .map(|&c| canon[c]),
            );
        }
    }
    persistent_set.extend(sreach.iter().filter(|(_, &n)| n >= g_min).map(|(&c, _)| c));
    let cat = |i: usize| -> Cat {
        let c = canon[i];
        if music_set.contains(&c) {
            Cat::Music
        } else if atmos_set.contains(&c) {
            Cat::Atmos
        } else if persistent_set.contains(&c) {
            Cat::Persistent
        } else if loop_set.contains(&c) {
            Cat::Loop
        } else {
            match sreach.get(&c).copied().unwrap_or(0) {
                0 => Cat::Unreached,
                n if n >= g_min => Cat::Persistent,
                n if n >= w_min => Cat::Wide,
                _ => Cat::Local,
            }
        }
    };
    let bytes = |i: usize, rate: i64| adpcm_bytes(clips[i].seconds, rate, clips[i].rate);

    let mut t = String::from("# scene\tclip key\tclip name\tseconds\tdirect\n");
    for sc in &w.scenes {
        for (&c, &d) in &sc.clips {
            let _ = writeln!(
                t,
                "{}\t{}\t{}\t{:.3}\t{}",
                sc.name, clips[c].key, clips[c].name, clips[c].seconds, d as u8
            );
        }
    }
    put(out, "scene_clips.tsv", t)?;
    // ---------------------------------------------------------------- clips.tsv
    let mut t =
        String::from("# key\tname\tchannels\trate\tseconds\tstored_bytes\tscenes\tcategory\n");
    for (i, c) in clips.iter().enumerate() {
        let _ = writeln!(
            t,
            "{}\t{}\t{}\t{}\t{:.3}\t{}\t{}\t{:?}",
            c.key,
            c.name,
            c.channels,
            c.rate,
            c.seconds,
            c.stored,
            reach[i],
            cat(i)
        );
    }
    put(out, "clips.tsv", t)?;

    // ---------------------------------------------------------------- scenes.tsv
    let mut t =
        String::from("# index\tscene\tarea\tdlc\tlocation\tsnapshot\tatmos\tmusic (reach:cue)\n");
    for sc in &w.scenes {
        let cues: Vec<String> = sc
            .cues
            .iter()
            .map(|(&c, r)| format!("{}:{}", r.label(), w.cues[c].name))
            .collect();
        let _ = writeln!(
            t,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            sc.index,
            sc.name,
            sc.area,
            dlc_of(sc.area),
            sc.loc.map(Loc::label).unwrap_or(""),
            sc.snapshot,
            sc.atmos
                .map(|a| w.atmos_cues[a].name.clone())
                .unwrap_or_default(),
            cues.join(" ")
        );
    }
    put(out, "scenes.tsv", t)?;

    // ---------------------------------------------------------------- per scene SPU
    struct SceneNeed {
        global: Vec<u64>,
        wide: Vec<u64>,
        local: Vec<u64>,
        amb: Vec<u64>,
        local_n: usize,
        local_s: f64,
        direct_s: f64,
    }
    let globals: BTreeSet<usize> = (0..clips.len())
        .filter(|&i| cat(i) == Cat::Persistent)
        .map(|i| canon[i])
        .collect();
    let g_bytes: Vec<u64> = PROFILES
        .iter()
        .map(|p| globals.iter().map(|&i| bytes(i, p.1)).sum())
        .collect();
    let g_secs: f64 = globals.iter().map(|&i| clips[i].seconds).sum();
    let mut needs: Vec<SceneNeed> = Vec::new();
    for sc in &w.scenes {
        let mut wide = BTreeSet::new();
        let mut local = BTreeSet::new();
        let mut direct_local = BTreeSet::new();
        for (&c, &d) in sc.clips.iter() {
            if d && cat(c) == Cat::Local {
                direct_local.insert(canon[c]);
            }
            match cat(c) {
                Cat::Wide => {
                    wide.insert(canon[c]);
                }
                Cat::Local => {
                    local.insert(canon[c]);
                }
                _ => {}
            }
        }
        let mut amb: BTreeSet<usize> = sc.loop_clips.iter().map(|&i| canon[i]).collect();
        if let Some(a) = sc.atmos {
            for &ch in &w.atmos_cues[a].channels {
                if let Some(Some(c)) = w.atmos_channels.get(ch) {
                    amb.insert(canon[*c]);
                }
            }
        }
        let f = |set: &BTreeSet<usize>, p: usize, ambience: bool| -> u64 {
            set.iter()
                .map(|&i| {
                    bytes(
                        i,
                        if ambience {
                            PROFILES[p].2
                        } else {
                            PROFILES[p].1
                        },
                    )
                })
                .sum()
        };
        needs.push(SceneNeed {
            global: g_bytes.clone(),
            wide: (0..3).map(|p| f(&wide, p, false)).collect(),
            local: (0..3).map(|p| f(&local, p, false)).collect(),
            amb: (0..3).map(|p| f(&amb, p, true)).collect(),
            local_n: local.len(),
            local_s: local.iter().map(|&i| clips[i].seconds).sum(),
            direct_s: direct_local.iter().map(|&i| clips[i].seconds).sum(),
        });
    }
    let mut t = String::from("# scene\tarea\tlocation\tlocal_clips\tlocal_seconds\tdirect_seconds\tlocal_B@full\tlocal_B@half\tlocal_B@port\tamb_B@full\tamb_B@half\tamb_B@port\n");
    for (sc, n) in w.scenes.iter().zip(&needs) {
        let _ = writeln!(
            t,
            "{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}",
            sc.name,
            sc.area,
            sc.loc.map(Loc::label).unwrap_or(""),
            n.local_n,
            n.local_s,
            n.direct_s,
            n.local[0],
            n.local[1],
            n.local[2],
            n.amb[0],
            n.amb[1],
            n.amb[2]
        );
    }
    put(out, "scene_spu.tsv", t)?;

    // ---------------------------------------------------------------- cues / pieces
    let base_scene = |s: &crate::scan::SceneRec| dlc_of(s.area).is_empty();
    let mut pieces: BTreeMap<Vec<usize>, Piece> = BTreeMap::new();
    for (ci, c) in w.cues.iter().enumerate() {
        let layers: Vec<usize> = c.layers.iter().flatten().map(|&i| canon[i]).collect();
        if layers.is_empty() {
            continue;
        }
        let slots: Vec<usize> = c
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.is_some())
            .map(|(i, _)| i)
            .collect();
        let key = layers.clone();
        let p = pieces.entry(key).or_insert_with(|| Piece {
            names: Vec::new(),
            layers: layers.clone(),
            cue_ix: Vec::new(),
            variants: BTreeSet::new(),
            guess: false,
            scenes: BTreeSet::new(),
            base: false,
            dlc: false,
            locs: BTreeSet::new(),
            xa_ok: true,
            traversal_theme: false,
        });
        p.names.push(c.name.clone());
        p.cue_ix.push(ci);
        let add_variant = |snapshot: &str, p: &mut Piece| {
            let audible = snapshot_layers(snapshot);
            let set: Vec<usize> = match &audible {
                Some(a) => a.iter().copied().filter(|s| slots.contains(s)).collect(),
                None => slots.clone(),
            };
            let set = if set.is_empty() && snapshot != "Silent" {
                slots.clone()
            } else {
                set
            };
            if audible.is_none() && !snapshot.is_empty() {
                p.guess = true;
            }
            if !set.is_empty() {
                p.variants.insert(set);
            }
        };
        let mut script_only = true;
        for (si, sc) in w.scenes.iter().enumerate() {
            let reach_kind = sc.cues.get(&ci).copied();
            let alt = w
                .cues
                .iter()
                .enumerate()
                .any(|(o, oc)| sc.cues.contains_key(&o) && oc.alternatives.contains(&c.key));
            let Some(r) = reach_kind.or(if alt { Some(Reach::Indirect) } else { None }) else {
                continue;
            };
            p.scenes.insert(si);
            if base_scene(sc) {
                p.base = true;
            } else {
                p.dlc = true;
            }
            let loc = sc.loc.unwrap_or(Loc::Traversal);
            p.locs.insert(loc);
            let theme = matches!(r, Reach::SceneCue | Reach::Region);
            if theme {
                script_only = false;
                if loc == Loc::Traversal {
                    p.xa_ok = false;
                    p.traversal_theme = true;
                }
                for (sci, snap) in &sc.states {
                    if *sci == ci {
                        add_variant(snap, p);
                    }
                }
            }
            if r == Reach::Indirect {
                // reached through an alternative or a prefab: treated as the same theme
                if loc == Loc::Traversal && !theme {
                    p.xa_ok = false;
                    p.traversal_theme = true;
                }
            }
        }
        if script_only || p.variants.is_empty() {
            add_variant("Normal", p);
        }
    }
    let mut cue_t = String::from(
        "# cue\tevent\tlayers (clip:seconds)\tscenes\tbase\tdlc\tlocations\tvariants\txa_ok\n",
    );
    for p in pieces.values() {
        let layers: Vec<String> = p
            .layers
            .iter()
            .map(|&i| format!("{}:{:.1}", clips[i].name, clips[i].seconds))
            .collect();
        let locs: Vec<&str> = p.locs.iter().map(|l| l.label()).collect();
        let _ = writeln!(
            cue_t,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            p.names.join("+"),
            w.cues[p.cue_ix[0]].event,
            layers.join(" | "),
            p.scenes.len(),
            p.base as u8,
            p.dlc as u8,
            locs.join(","),
            p.variants.len(),
            p.xa_ok as u8
        );
    }
    put(out, "music.tsv", cue_t)?;

    // atmos
    let mut t = String::from("# atmos cue\tscenes\tchannel=clip:seconds\n");
    for (i, a) in w.atmos_cues.iter().enumerate() {
        let n = w.scenes.iter().filter(|s| s.atmos == Some(i)).count();
        let ch: Vec<String> = a
            .channels
            .iter()
            .filter_map(|&c| {
                w.atmos_channels
                    .get(c)
                    .copied()
                    .flatten()
                    .map(|x| format!("{c}={}:{:.1}", clips[x].name, clips[x].seconds))
            })
            .collect();
        let _ = writeln!(t, "{}\t{}\t{}", a.name, n, ch.join(" "));
    }
    put(out, "atmos.tsv", t)?;

    // owners: enemies, NPCs and other objects with sounds
    #[derive(Default)]
    struct Owner {
        scenes: BTreeSet<usize>,
        areas: BTreeSet<&'static str>,
        clips: BTreeSet<usize>,
        arena: bool,
        base: bool,
    }
    let mut owners: BTreeMap<(String, String), Owner> = BTreeMap::new();
    for (si, sc) in w.scenes.iter().enumerate() {
        for ((k, n), cl) in &sc.owners {
            let o = owners.entry((k.clone(), n.clone())).or_default();
            o.scenes.insert(si);
            o.areas.insert(sc.area);
            o.arena |= sc.loc == Some(Loc::Arena);
            if base_scene(sc) {
                o.base = true;
            }
            for &c in cl {
                if matches!(cat(c), Cat::Local | Cat::Wide | Cat::Loop) {
                    o.clips.insert(canon[c]);
                }
            }
        }
    }
    let mut t = String::from(
        "# kind\tname\tscenes\tareas\tclips\tseconds\tB@full\tB@half\tB@port\tin_arena\tbase\n",
    );
    let mut owner_rows: Vec<(&(String, String), &Owner)> = owners
        .iter()
        .filter(|((k, _), o)| k != "object" && !o.clips.is_empty())
        .collect();
    owner_rows.sort_by(|a, b| a.0.cmp(b.0));
    for ((k, n), o) in &owner_rows {
        let secs: f64 = o.clips.iter().map(|&i| clips[i].seconds).sum();
        let b: Vec<u64> = PROFILES
            .iter()
            .map(|p| o.clips.iter().map(|&i| bytes(i, p.1)).sum())
            .collect();
        let _ = writeln!(
            t,
            "{k}\t{n}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{}",
            o.scenes.len(),
            o.areas.len(),
            o.clips.len(),
            secs,
            b[0],
            b[1],
            b[2],
            o.arena as u8,
            o.base as u8
        );
    }
    put(out, "owners.tsv", t)?;

    // ---------------------------------------------------------------- markdown
    let mut m = String::new();
    let all_secs: f64 = clips.iter().map(|c| c.seconds).sum();
    let stored: u64 = clips.iter().map(|c| c.stored).sum();
    let mut count_by = |c: Cat| {
        (0..clips.len())
            .filter(|&i| cat(i) == c && canon[i] == i)
            .count()
    };
    let _ = (&mut count_by,);
    let n_of = |c: Cat| {
        (0..clips.len())
            .filter(|&i| canon[i] == i && cat(i) == c)
            .count()
    };
    let s_of = |c: Cat| -> f64 {
        (0..clips.len())
            .filter(|&i| canon[i] == i && cat(i) == c)
            .map(|i| clips[i].seconds)
            .sum()
    };
    let b_of = |c: Cat, rate: i64| -> u64 {
        (0..clips.len())
            .filter(|&i| canon[i] == i && cat(i) == c)
            .map(|i| bytes(i, rate))
            .sum()
    };
    let _ = writeln!(m, "# HK audio census (generated)\n");
    let _ = writeln!(m, "Generated by `hk-audio-census extract` from the Windows install; do not edit by hand. Names, counts, durations and sizes only; no audio. Regenerate: `cargo run --release --manifest-path host/Cargo.toml -p hk-audio-census -- extract <hollow_knight_Data> docs/audio-census`. Companion tables in this directory: `clips.tsv` (every AudioClip), `scene_clips.tsv` (every clip each of the 501 scenes can play), `scenes.tsv`, `music.tsv`, `atmos.tsv`, `owners.tsv` (enemy and NPC sound sets), `scene_spu.tsv` (per-scene SPU bytes).\n");
    let _ = writeln!(m, "## 1. Totals\n");
    let _ = writeln!(m, "- AudioClips in the install: {} ({} distinct sounds after dropping same-name same-length copies, {} copies), {} min total, {} MB of Vorbis (FSB) stored.", clips.len(), first.len(), dup_names, mins(all_secs), mb(stored));
    let _ = writeln!(m, "- Scenes scanned: {} (every BuildSettings scene; {} base game, {} Grimm Troupe, {} Godhome).", n_scenes, w.scenes.iter().filter(|s| dlc_of(s.area).is_empty()).count(), w.scenes.iter().filter(|s| dlc_of(s.area) == "grimm").count(), w.scenes.iter().filter(|s| dlc_of(s.area) == "godhome").count());
    let _ = writeln!(m, "- MusicCues: {} ({} with audio, {} distinct layer sets); AtmosCues: {}; atmos channels with a clip: {}.\n", w.cues.len(), w.cues.iter().filter(|c| c.layers.iter().any(Option::is_some)).count(), pieces.len(), w.atmos_cues.len(), w.atmos_channels.iter().flatten().count());
    let _ = writeln!(m, "| Category | Sounds | Minutes | SPU KB @22,050 | @11,025 | @port (5,512 one-shot, 4,000 ambience) |");
    let _ = writeln!(m, "| --- | ---: | ---: | ---: | ---: | ---: |");
    for (label, c) in [
        ("Music layers (in MusicCues)", Cat::Music),
        ("Atmos channel loops (AudioManager)", Cat::Atmos),
        ("Looping scene AudioSources", Cat::Loop),
        (
            "Persistent (the Knight prefab, UI sounds, pickups)",
            Cat::Persistent,
        ),
        ("Wide SFX (15% to 80% of scenes)", Cat::Wide),
        ("Local SFX (under 15% of scenes)", Cat::Local),
        (
            "Not reached by any scene (played by name or unused)",
            Cat::Unreached,
        ),
    ] {
        let port = if c == Cat::Atmos || c == Cat::Loop {
            b_of(c, 4000)
        } else {
            b_of(c, 5512)
        };
        let _ = writeln!(
            m,
            "| {} | {} | {} | {} | {} | {} |",
            label,
            n_of(c),
            mins(s_of(c)),
            kb(b_of(c, 22050)),
            kb(b_of(c, 11025)),
            kb(port)
        );
    }
    let _ = writeln!(m, "\nSPU bytes are PS1 ADPCM of the clip folded to mono, rate capped at the clip's own (16 bytes per 28 samples). Music layers are stereo 44.1 or 48 kHz in the source and are not SPU residents (they stream).\n");
    let _ = writeln!(m, "Persistent set ({} sounds, {:.0} s): {} KB at 22,050, {} KB at 11,025, {} KB at port rates.\n", globals.len(), g_secs, kb(g_bytes[0]), kb(g_bytes[1]), kb(g_bytes[2]));

    // ---- music pieces
    let _ = writeln!(m, "## 2. Music\n");
    let mut plist: Vec<&Piece> = pieces.values().collect();
    plist.sort_by(|a, b| a.names[0].cmp(&b.names[0]));
    let piece_secs = |p: &Piece| {
        p.layers
            .iter()
            .map(|&i| clips[i].seconds)
            .fold(0.0, f64::max)
    };
    let total_piece_secs: f64 = plist.iter().map(|p| piece_secs(p)).sum();
    let stem_secs: f64 = music_set.iter().map(|&i| clips[i].seconds).sum();
    let _ = writeln!(m, "{} pieces (cues with distinct layer sets); {} min if each is heard once ({} min counting every stem); {} distinct stem clips.", plist.len(), mins(total_piece_secs), mins(stem_secs), music_set.len());
    let base_p = plist.iter().filter(|p| p.base).count();
    let dlc_p = plist.iter().filter(|p| !p.base && p.dlc).count();
    let _ = writeln!(
        m,
        "Base game {} pieces ({} min), DLC-only {} pieces ({} min).\n",
        base_p,
        mins(plist.iter().filter(|p| p.base).map(|p| piece_secs(p)).sum()),
        dlc_p,
        mins(
            plist
                .iter()
                .filter(|p| !p.base && p.dlc)
                .map(|p| piece_secs(p))
                .sum()
        )
    );
    let _ = writeln!(
        m,
        "| Piece (cue names) | Stems | Seconds | Variants | Where | Scenes | DLC | CD free |"
    );
    let _ = writeln!(m, "| --- | ---: | ---: | ---: | --- | ---: | --- | --- |");
    for p in &plist {
        let locs: Vec<&str> = p.locs.iter().map(|l| l.label()).collect();
        let dl = if p.base {
            if p.dlc {
                "both"
            } else {
                ""
            }
        } else if p.dlc {
            "yes"
        } else {
            ""
        };
        let where_ = if locs.is_empty() {
            "script only, no scene".to_string()
        } else {
            locs.join(", ")
        };
        let _ = writeln!(
            m,
            "| {} | {} | {:.0} | {}{} | {} | {} | {} | {} |",
            p.names.join(", "),
            p.layers.len(),
            piece_secs(p),
            p.variants.len(),
            if p.guess { "*" } else { "" },
            where_,
            p.scenes.len(),
            dl,
            if p.xa_ok { "yes" } else { "no" }
        );
    }
    let _ = writeln!(m, "\nVariants: premixed audible-layer sets the scenes and regions can select through mixer snapshots (`*` = a snapshot not in the cooked table, counted as all layers). \"CD free\": every use is a script-applied fight or event cue, or in a boss arena, cutscene or menu scene; \"no\" means a traversal room plays it as its theme, so scene streaming shares the drive.\n");

    // other long audio
    let _ = writeln!(
        m,
        "### Long clips outside the cues (cutscene audio, set pieces, arena loops)\n"
    );
    let _ = writeln!(
        m,
        "| Clip | Seconds | Ch | Rate | Category | Scenes | First scene | Where |"
    );
    let _ = writeln!(m, "| --- | ---: | ---: | ---: | --- | ---: | --- | --- |");
    let mut longs: Vec<usize> = (0..clips.len())
        .filter(|&i| {
            canon[i] == i && clips[i].seconds >= LONG && !matches!(cat(i), Cat::Music | Cat::Atmos)
        })
        .collect();
    longs.sort_by(|&a, &b| clips[b].seconds.partial_cmp(&clips[a].seconds).unwrap());
    let mut long_total = 0.0;
    let mut long_xa = 0.0;
    for &i in &longs {
        let holders: Vec<&crate::scan::SceneRec> = w
            .scenes
            .iter()
            .filter(|s| s.clips.keys().any(|&c| canon[c] == i))
            .collect();
        let locs: BTreeSet<&str> = holders
            .iter()
            .filter_map(|s| s.loc)
            .map(Loc::label)
            .collect();
        let free = holders.iter().all(|s| s.loc != Some(Loc::Traversal));
        if clips[i].channels == 2 {
            long_total += clips[i].seconds;
            if free {
                long_xa += clips[i].seconds;
            }
        }
        let _ = writeln!(
            m,
            "| {} | {:.1} | {} | {} | {:?} | {} | {} | {} |",
            clips[i].name,
            clips[i].seconds,
            clips[i].channels,
            clips[i].rate,
            cat(i),
            holders.len(),
            holders.first().map(|s| s.name.as_str()).unwrap_or("-"),
            locs.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    let _ = writeln!(m, "\n{} clips of {} s or more outside the cues; stereo ones total {} min, of which {} min are heard only outside traversal rooms.\n", longs.len(), LONG, mins(long_total), mins(long_xa));

    // ---- ambience
    let _ = writeln!(m, "## 3. Ambience\n");
    let _ = writeln!(m, "| Atmos cue | Scenes | Channels | Longest s | Sum s | KB @22,050 | KB @11,025 | KB @4,000 |");
    let _ = writeln!(
        m,
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
    );
    for (i, a) in w.atmos_cues.iter().enumerate() {
        let n = w.scenes.iter().filter(|s| s.atmos == Some(i)).count();
        let ids: BTreeSet<usize> = a
            .channels
            .iter()
            .filter_map(|&c| w.atmos_channels.get(c).copied().flatten())
            .map(|x| canon[x])
            .collect();
        let longest = ids.iter().map(|&x| clips[x].seconds).fold(0.0, f64::max);
        let sum: f64 = ids.iter().map(|&x| clips[x].seconds).sum();
        let f = |r: i64| -> u64 { ids.iter().map(|&x| bytes(x, r)).sum() };
        let _ = writeln!(
            m,
            "| {} | {} | {} | {:.1} | {:.1} | {} | {} | {} |",
            a.name,
            n,
            ids.len(),
            longest,
            sum,
            kb(f(22050)),
            kb(f(11025)),
            kb(f(4000))
        );
    }
    let amb_all: BTreeSet<usize> = atmos_set.iter().chain(loop_set.iter()).copied().collect();
    let _ = writeln!(m, "\nAll {} atmos channel loops ({} s): {} KB @22,050, {} KB @11,025, {} KB @4,000. Looping scene AudioSources add {} more loops ({} KB @11,025).\n", atmos_set.len(), atmos_set.iter().map(|&i| clips[i].seconds).sum::<f64>() as u64, kb(atmos_set.iter().map(|&i| bytes(i, 22050)).sum()), kb(atmos_set.iter().map(|&i| bytes(i, 11025)).sum()), kb(atmos_set.iter().map(|&i| bytes(i, 4000)).sum()), loop_set.difference(&atmos_set).count(), kb(loop_set.difference(&atmos_set).map(|&i| bytes(i, 11025)).sum()));
    let _ = amb_all;

    // ---- SFX by prefix
    let _ = writeln!(m, "## 4. Sound effects\n");
    let mut by_prefix: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for (i, c) in clips.iter().enumerate() {
        if canon[i] != i || matches!(cat(i), Cat::Music | Cat::Atmos) {
            continue;
        }
        let n = c.name.to_lowercase();
        let p: String = n
            .split(|ch: char| ch == '_' || ch == ' ' || ch == '-' || ch.is_ascii_digit())
            .next()
            .unwrap_or("")
            .to_string();
        let e = by_prefix.entry(p).or_default();
        e.0 += 1;
        e.1 += c.seconds;
    }
    let mut pv: Vec<_> = by_prefix.into_iter().collect();
    pv.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
    let _ = writeln!(
        m,
        "Name prefixes (first word of the clip name), top 40 of {}:\n",
        pv.len()
    );
    let _ = writeln!(m, "| Prefix | Sounds | Seconds |\n| --- | ---: | ---: |");
    for (p, (n, s)) in pv.iter().take(40) {
        let _ = writeln!(m, "| {} | {} | {:.0} |", p, n, s);
    }

    // ---- per area
    let _ = writeln!(m, "\n## 5. Per area: what each area's rooms need\n");
    let _ = writeln!(m, "Scene-local sounds are those reached by under 15% of scenes; the area pool is their union over the area's scenes. Bytes are at port one-shot rates (5,512) and at 22,050 (full); the worst scene is the largest single room.\n");
    let _ = writeln!(m, "| Area | Scenes | Pool sounds | Pool s | Pool KB @22k | @11k | @port | Worst room KB @22k | @11k | @port | Amb KB @4k | @11k |");
    let _ = writeln!(
        m,
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
    );
    for area in AREAS {
        let idx: Vec<usize> = (0..n_scenes)
            .filter(|&i| w.scenes[i].area == area)
            .collect();
        if idx.is_empty() {
            continue;
        }
        let mut pool = BTreeSet::new();
        let mut amb = BTreeSet::new();
        for &si in &idx {
            for &c in w.scenes[si].clips.keys() {
                if cat(c) == Cat::Local {
                    pool.insert(canon[c]);
                }
            }
            amb.extend(w.scenes[si].loop_clips.iter().map(|&i| canon[i]));
            if let Some(a) = w.scenes[si].atmos {
                for &ch in &w.atmos_cues[a].channels {
                    if let Some(Some(c)) = w.atmos_channels.get(ch) {
                        amb.insert(canon[*c]);
                    }
                }
            }
        }
        let pb = |r: i64| -> u64 { pool.iter().map(|&i| bytes(i, r)).sum() };
        let worst = |p: usize| idx.iter().map(|&i| needs[i].local[p]).max().unwrap_or(0);
        let ab = |r: i64| -> u64 { amb.iter().map(|&i| bytes(i, r)).sum() };
        let _ = writeln!(
            m,
            "| {} | {} | {} | {:.0} | {} | {} | {} | {} | {} | {} | {} | {} |",
            area,
            idx.len(),
            pool.len(),
            pool.iter().map(|&i| clips[i].seconds).sum::<f64>(),
            kb(pb(22050)),
            kb(pb(11025)),
            kb(pb(5512)),
            kb(worst(0)),
            kb(worst(1)),
            kb(worst(2)),
            kb(ab(4000)),
            kb(ab(11025))
        );
    }

    // ---- per location class
    let _ = writeln!(m, "\n## 6. Per room type: whole-room SPU need\n");
    let _ = writeln!(m, "A room needs the persistent set + the wide sounds it reaches + its local sounds + its ambience, plus the {} KB music ring in rooms that stream area music. Usable SPU is {} KB ({} bytes). Columns give the median, 90th percentile and maximum over the room type, and how many rooms exceed the SPU.\n", kb(RING), kb(SPU_USABLE), SPU_USABLE);
    let _ = writeln!(
        m,
        "| Room type | Profile | Rooms | Median KB | p90 KB | Max KB | Rooms over SPU | Max room |"
    );
    let _ = writeln!(m, "| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |");
    for loc in [Loc::Traversal, Loc::Arena, Loc::Cutscene, Loc::Menu] {
        for (pi, prof) in PROFILES.iter().enumerate() {
            let rows: Vec<(u64, &str)> = (0..n_scenes)
                .filter(|&i| w.scenes[i].loc == Some(loc))
                .map(|i| {
                    let n = &needs[i];
                    let ring = if loc == Loc::Traversal { RING } else { 0 };
                    (
                        n.global[pi] + n.wide[pi] + n.local[pi] + n.amb[pi] + ring,
                        w.scenes[i].name.as_str(),
                    )
                })
                .collect();
            if rows.is_empty() {
                continue;
            }
            let mut v: Vec<u64> = rows.iter().map(|r| r.0).collect();
            let max = rows.iter().max_by_key(|r| r.0).unwrap();
            let over = v.iter().filter(|&&x| x > SPU_USABLE).count();
            let _ = writeln!(
                m,
                "| {} | {} | {} | {} | {} | {} | {} | {} |",
                loc.label(),
                prof.0,
                rows.len(),
                kb(pct(&mut v, 0.5)),
                kb(pct(&mut v, 0.9)),
                kb(max.0),
                over,
                max.1
            );
        }
    }
    let _ = writeln!(m, "\nComponents of the persistent set and the widest rooms, in KB at 22,050 / 11,025 / port: persistent {} / {} / {}; wide sounds reached by one room at most {} / {} / {}.\n", kb(g_bytes[0]), kb(g_bytes[1]), kb(g_bytes[2]), kb(needs.iter().map(|n| n.wide[0]).max().unwrap_or(0)), kb(needs.iter().map(|n| n.wide[1]).max().unwrap_or(0)), kb(needs.iter().map(|n| n.wide[2]).max().unwrap_or(0)));
    // worst rooms by local
    let mut order: Vec<usize> = (0..n_scenes).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(needs[i].local[0]));
    let _ = writeln!(
        m,
        "Largest scene-local sets (KB at 22,050 / 11,025 / port; sounds; seconds):\n"
    );
    let _ = writeln!(m, "| Room | Area | Type | Sounds | Seconds | KB @22k | @11k | @port |\n| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |");
    for &i in order.iter().take(15) {
        let n = &needs[i];
        let _ = writeln!(
            m,
            "| {} | {} | {} | {} | {:.0} | {} | {} | {} |",
            w.scenes[i].name,
            w.scenes[i].area,
            w.scenes[i].loc.map(Loc::label).unwrap_or(""),
            n.local_n,
            n.local_s,
            kb(n.local[0]),
            kb(n.local[1]),
            kb(n.local[2])
        );
    }
    // how many rooms fit the current free bytes
    let free = [16_656u64, 27_552, 56_432];
    let _ = writeln!(m, "\nRooms whose scene-local sounds fit the port's measured free SPU (16,656 / 27,552 / 56,432 bytes), of {}:\n", n_scenes);
    let _ = writeln!(
        m,
        "| Profile | <= 16,656 | <= 27,552 | <= 56,432 |\n| --- | ---: | ---: | ---: |"
    );
    for (pi, prof) in PROFILES.iter().enumerate() {
        let c = |f: u64| {
            needs
                .iter()
                .filter(|n| n.local[pi] + n.wide[pi] <= f)
                .count()
        };
        let _ = writeln!(
            m,
            "| {} | {} | {} | {} |",
            prof.0,
            c(free[0]),
            c(free[1]),
            c(free[2])
        );
    }

    // ---- enemies / bosses / NPCs
    let _ = writeln!(m, "\n## 7. Per enemy, boss and NPC\n");
    let enemies: Vec<_> = owner_rows
        .iter()
        .filter(|((k, _), _)| k == "enemy")
        .collect();
    let npcs: Vec<_> = owner_rows.iter().filter(|((k, _), _)| k == "npc").collect();
    let e_clips: BTreeSet<usize> = enemies
        .iter()
        .flat_map(|(_, o)| o.clips.iter().copied())
        .collect();
    let _ = writeln!(m, "{} enemy and boss objects own at least one scene-specific sound (HealthManager objects, names normalised), {} NPC objects; together {} distinct enemy sounds ({} s, {} KB @11,025). The full list is `owners.tsv`.\n", enemies.len(), npcs.len(), e_clips.len(), e_clips.iter().map(|&i| clips[i].seconds).sum::<f64>() as u64, kb(e_clips.iter().map(|&i| bytes(i, 11025)).sum()));
    let _ = writeln!(m, "Largest enemy and boss sets:\n");
    let _ = writeln!(m, "| Name | Scenes | Sounds | Seconds | KB @22k | @11k | @port | Arena |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
    let mut ev: Vec<_> = enemies.iter().collect();
    ev.sort_by_key(|(_, o)| {
        std::cmp::Reverse(o.clips.iter().map(|&i| bytes(i, 22050)).sum::<u64>())
    });
    for (k, o) in ev.iter().take(30) {
        let b: Vec<u64> = PROFILES
            .iter()
            .map(|p| o.clips.iter().map(|&i| bytes(i, p.1)).sum())
            .collect();
        let _ = writeln!(
            m,
            "| {} | {} | {} | {:.0} | {} | {} | {} | {} |",
            k.1,
            o.scenes.len(),
            o.clips.len(),
            o.clips.iter().map(|&i| clips[i].seconds).sum::<f64>(),
            kb(b[0]),
            kb(b[1]),
            kb(b[2]),
            if o.arena { "yes" } else { "" }
        );
    }

    // ---- room-type seconds: reach versus direct
    let _ = writeln!(m, "\nScene-local seconds per room (sounds reached by under 15% of scenes). `Reach` includes everything the scene's prefabs and spawners can bring in (Grimm Troupe spawners, effects); `direct` is only clips placed on the scene's own objects.\n");
    let _ = writeln!(m, "| Room type | Rooms | Reach median s | p90 s | max s | Direct median s | p90 s | max s |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    for loc in [Loc::Traversal, Loc::Arena, Loc::Cutscene, Loc::Menu] {
        let idx: Vec<usize> = (0..n_scenes)
            .filter(|&i| w.scenes[i].loc == Some(loc))
            .collect();
        if idx.is_empty() {
            continue;
        }
        let mut r: Vec<u64> = idx
            .iter()
            .map(|&i| (needs[i].local_s * 10.0) as u64)
            .collect();
        let mut d: Vec<u64> = idx
            .iter()
            .map(|&i| (needs[i].direct_s * 10.0) as u64)
            .collect();
        let f = |x: u64| format!("{:.0}", x as f64 / 10.0);
        let _ = writeln!(
            m,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            loc.label(),
            idx.len(),
            f(pct(&mut r, 0.5)),
            f(pct(&mut r, 0.9)),
            f(pct(&mut r, 1.0)),
            f(pct(&mut d, 0.5)),
            f(pct(&mut d, 0.9)),
            f(pct(&mut d, 1.0))
        );
    }

    // ---- DLC
    let _ = writeln!(m, "\n## 8. Base game, Grimm Troupe and Godhome\n");
    let dlc_of_sound = |c: usize| -> (bool, bool, bool) {
        // (reached by a base scene, by a Grimm scene, by a Godhome scene)
        let mut r = (false, false, false);
        for s in &w.scenes {
            if s.clips.keys().any(|&x| canon[x] == c) {
                match dlc_of(s.area) {
                    "" => r.0 = true,
                    "grimm" => r.1 = true,
                    _ => r.2 = true,
                }
            }
        }
        r
    };
    let mut ex: [(usize, f64, u64, u64); 3] = [(0, 0.0, 0, 0); 3];
    let reached: BTreeSet<usize> = sreach
        .keys()
        .copied()
        .filter(|&c| !music_set.contains(&c))
        .collect();
    for &c in &reached {
        let (b, g, h) = dlc_of_sound(c);
        let slot = if b {
            None
        } else if g && !h {
            Some(1)
        } else if h && !g {
            Some(2)
        } else if g || h {
            Some(1)
        } else {
            None
        };
        if let Some(sl) = slot {
            ex[sl].0 += 1;
            ex[sl].1 += clips[c].seconds;
            ex[sl].2 += bytes(c, 11025);
            ex[sl].3 += bytes(c, 22050);
        }
    }
    let base_total: (usize, f64) = reached
        .iter()
        .filter(|&&c| dlc_of_sound(c).0)
        .fold((0, 0.0), |a, &c| (a.0 + 1, a.1 + clips[c].seconds));
    let _ = writeln!(m, "| Part | Scenes | Sounds only this part reaches | Minutes | MB ADPCM @22,050 | @11,025 |\n| --- | ---: | ---: | ---: | ---: | ---: |");
    let _ = writeln!(
        m,
        "| Base game | {} | {} (all sounds a base scene reaches) | {} | | |",
        w.scenes
            .iter()
            .filter(|s| dlc_of(s.area).is_empty())
            .count(),
        base_total.0,
        mins(base_total.1)
    );
    let _ = writeln!(
        m,
        "| Grimm Troupe | {} | {} | {} | {} | {} |",
        w.scenes
            .iter()
            .filter(|s| dlc_of(s.area) == "grimm")
            .count(),
        ex[1].0,
        mins(ex[1].1),
        mb(ex[1].3),
        mb(ex[1].2)
    );
    let _ = writeln!(
        m,
        "| Godhome | {} | {} | {} | {} | {} |",
        w.scenes
            .iter()
            .filter(|s| dlc_of(s.area) == "godhome")
            .count(),
        ex[2].0,
        mins(ex[2].1),
        mb(ex[2].3),
        mb(ex[2].2)
    );
    let dlc_music: BTreeSet<usize> = plist
        .iter()
        .filter(|p| !p.base && p.dlc)
        .flat_map(|p| p.layers.iter().copied())
        .collect();
    let _ = writeln!(
        m,
        "\nMusic reached only by DLC scenes: {} pieces, {} distinct stems, {} min of stems.\n",
        plist.iter().filter(|p| !p.base && p.dlc).count(),
        dlc_music.len(),
        mins(dlc_music.iter().map(|&i| clips[i].seconds).sum())
    );

    // ---- music options
    let _ = writeln!(m, "## 9. Music: disc cost of each mechanism\n");
    // variants: (seconds, xa_ok, base)
    struct V {
        secs: f64,
        xa: bool,
        base: bool,
    }
    let mut vs: Vec<V> = Vec::new();
    for p in &plist {
        let secs = piece_secs(p);
        for _ in 0..p.variants.len().max(1) {
            vs.push(V {
                secs,
                xa: p.xa_ok,
                base: p.base,
            });
        }
    }
    let sector = 2048.0;
    let ring_bytes =
        |secs: f64, rate: f64| -> f64 { ((secs * rate / 28.0 * 16.0) / sector).ceil() * sector };
    let xa_packed = |sel: &dyn Fn(&V) -> bool| -> (f64, f64) {
        let mut secs: Vec<f64> = vs.iter().filter(|v| sel(v)).map(|v| v.secs).collect();
        secs.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let lower: f64 = secs.iter().sum::<f64>() * 38_400.0;
        let mut real = 0.0;
        for chunk in secs.chunks(4) {
            real += chunk.iter().cloned().fold(0.0, f64::max) * 75.0 * 2048.0;
        }
        (lower, real)
    };
    let sum_secs =
        |sel: &dyn Fn(&V) -> bool| -> f64 { vs.iter().filter(|v| sel(v)).map(|v| v.secs).sum() };
    let ring = |sel: &dyn Fn(&V) -> bool, rate: f64| -> f64 {
        vs.iter()
            .filter(|v| sel(v))
            .map(|v| ring_bytes(v.secs, rate))
            .sum()
    };
    let _ = writeln!(m, "{} premix variants over the {} pieces ({} min of variant audio). Sizes are what the data takes from the disc, counted as sectors x 2,048 bytes (a Mode 2 XA sector costs the same disc space as a Mode 1 data sector; 333,000 sectors fill a 74 minute disc, 360,000 an 80 minute one). XA lower bound = seconds x 38,400 B/s (one sector in four per channel at single speed); packed = variants grouped four to a channel set, each set as long as its longest member (the existing MUSIC.XA layout, whose 12,844 sectors are 25.1 MB on this basis). Ring columns are 22,050 and 11,025 Hz mono ADPCM, rounded up to whole sectors.\n", vs.len(), plist.len(), mins(sum_secs(&|_| true)));
    let _ = writeln!(m, "| Selection | Variants | Minutes | XA lower MB | XA packed MB | XA packed sectors | Ring 22,050 mono MB | Ring 11,025 mono MB |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    let rows: Vec<(&str, Box<dyn Fn(&V) -> bool>)> = vec![
        ("All pieces, base + DLC", Box::new(|_| true)),
        ("All pieces, base game only", Box::new(|v: &V| v.base)),
        (
            "CD-free pieces only (fights, events, arenas, cutscenes, title)",
            Box::new(|v: &V| v.xa),
        ),
        (
            "CD-free pieces, base game",
            Box::new(|v: &V| v.xa && v.base),
        ),
        (
            "Traversal themes only (CD busy: need the ring)",
            Box::new(|v: &V| !v.xa),
        ),
        (
            "Traversal themes, base game",
            Box::new(|v: &V| !v.xa && v.base),
        ),
    ];
    for (label, f) in &rows {
        let n = vs.iter().filter(|v| f(v)).count();
        let (lo, real) = xa_packed(f.as_ref());
        let _ = writeln!(
            m,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            label,
            n,
            mins(sum_secs(f.as_ref())),
            mb(lo as u64),
            mb(real as u64),
            (real / 2048.0) as u64,
            mb(ring(f.as_ref(), 22050.0) as u64),
            mb(ring(f.as_ref(), 11025.0) as u64)
        );
    }
    let _ = writeln!(m, "\nLong stereo clips outside the cues that are heard only outside traversal rooms (cutscene audio, arena loops): {} min, XA lower bound {} MB.", mins(long_xa), mb((long_xa * 38_400.0) as u64));
    let _ = writeln!(m, "Drive share while playing: a 22,050 Hz mono ring stream reads 12,600 B/s, 8.2% of a 1x drive (153,600 B/s of Mode 1 data) and 4.1% of 2x; XA or CD-DA takes the whole drive.");

    put(out, "AUDIO_CENSUS.md", m)?;
    Ok(())
}
