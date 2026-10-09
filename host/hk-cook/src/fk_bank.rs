//! The False Knight's whole clip set as scene art: source sprites, the static/streamed plan,
//! and the part records appended to the actor atlas. Ported from the pixel half of
//! host/false_knight_art.py (`source_art`, `plan`, `append_bank`). Mawlek and the Gruz Mother
//! take the same plan and bank with their own clip lists and budgets.

use crate::actor_art::{guest_wrap, Clip, Frame};
use crate::actors::Row;
use crate::atlas::{Atlas, Quantized, Quantizer, MAX_TEXTURE_AXIS};
use crate::common::{err, get, py_round, Result};
use crate::cook::{focal, native_sprite, tk_sprite, CAM_Z};
use crate::cook_audio::u;
use crate::false_knight_art::{
    aligned, child, children, component, decompose, local, part_box, rect_bytes, shockwave_source,
    source_objects,
};
use crate::packer::dense_pack;
use crate::prefab::num;
use crate::pyfloat;
use crate::pyjson::Json;
use crate::recog::variables;
use hk_pil::Image;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// The guest's `hk_sim::false_knight::Clip` order (`false_knight.CLIP_ORDER`).
pub const BODY_CLIPS: [&str; 25] = [
    "Idle",
    "Turn",
    "Jump Antic",
    "Jump",
    "Land",
    "Run",
    "Run Antic",
    "Jump Attack Up",
    "Jump Attack Hit 1",
    "Jump Attack Hit 2",
    "Jump Attack Hit 3",
    "Attack Antic",
    "Attack",
    "Attack Recover",
    "Rage",
    "Stun Roll",
    "Stun Roll End",
    "Stun Open",
    "Stun Opened",
    "Stun Hit",
    "Stun Recover",
    "Death Fall",
    "Death Land",
    "Death Spaz",
    "Blank",
];
pub const EXTRA_CLIPS: [&str; 6] = [
    "Body",
    "Head Idle",
    "Head Hit",
    "Head Spaz",
    "Death Head 1",
    "Death Head 2",
];
pub const SPURT_CLIP: &str = "Shockwave Spurt";
/// The order clips are offered to the static pages.
pub const PRIORITY: [&str; 33] = [
    "Floor",
    "Idle",
    "Turn",
    "Jump Antic",
    "Land",
    "Jump",
    "Attack Antic",
    "Attack",
    "Attack Recover",
    SPURT_CLIP,
    "Rage",
    "Jump Attack Up",
    "Jump Attack Hit 1",
    "Jump Attack Hit 2",
    "Jump Attack Hit 3",
    "Run Antic",
    "Run",
    "Stun Opened",
    "Head Idle",
    "Head Hit",
    "Head Spaz",
    "Body",
    "Stun Open",
    "Stun Hit",
    "Stun Roll",
    "Stun Roll End",
    "Stun Recover",
    "Death Fall",
    "Death Land",
    "Death Spaz",
    "Death Head 1",
    "Death Head 2",
    "Blank",
];
pub const SCENE_PAGE_LIMIT: usize = 18;
pub const STREAM_BYTES_LIMIT: i64 = 180 * 1024;

/// `ART_CLIPS`: the guest's `ArtClip` names, in its order.
pub fn art_clips() -> Vec<String> {
    BODY_CLIPS
        .iter()
        .chain(EXTRA_CLIPS.iter())
        .chain([SPURT_CLIP].iter())
        .map(|s| s.to_string())
        .collect()
}

/// A sprite's identity: a tk2d sprite at a scale and offset, or a floor sprite by name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SpriteKey {
    Tk(String, i64, u64, u64, u64),
    Floor(String),
    /// Mawlek's keys: sprite, scales, quarter turn and the repr of the tint.
    Part(String, i64, u64, u64, i64, String),
    /// The Gruz Mother's keys: sprite and the one scale all three objects draw at.
    Gruz(String, i64, u64),
}

/// Python's `repr` of a string.
pub fn py_str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `round(v, 6)`: the double nearest the six-decimal rendering of the exact value.
pub fn round6(v: f64) -> f64 {
    format!("{v:.6}").parse().unwrap_or(v)
}

impl SpriteKey {
    pub fn tk(sid: &str, id: i64, factor: f64, ox: f64, oy: f64) -> SpriteKey {
        SpriteKey::Tk(
            sid.to_string(),
            id,
            round6(factor).to_bits(),
            round6(ox).to_bits(),
            round6(oy).to_bits(),
        )
    }

    /// `str(key)`: the Python tuple's repr, which the frame records carry as their `sprite` field.
    pub fn repr(&self) -> String {
        match self {
            SpriteKey::Tk(sid, id, a, b, c) => format!(
                "('{sid}', {id}, {}, {}, {})",
                pyfloat::repr(f64::from_bits(*a)),
                pyfloat::repr(f64::from_bits(*b)),
                pyfloat::repr(f64::from_bits(*c))
            ),
            SpriteKey::Floor(name) => format!("('floor', '{name}')"),
            SpriteKey::Gruz(sid, id, s) => {
                format!("('{sid}', {id}, {})", pyfloat::repr(f64::from_bits(*s)))
            }
            SpriteKey::Part(sid, id, a, b, turn, tint) => format!(
                "('{sid}', {id}, {}, {}, {turn}, {})",
                pyfloat::repr(f64::from_bits(*a)),
                pyfloat::repr(f64::from_bits(*b)),
                py_str_repr(tint)
            ),
        }
    }
}

pub struct SpriteRec {
    pub image: Image,
    pub box_: [f64; 4],
    pub w: usize,
    pub h: usize,
}

pub struct ArtClipRec {
    pub record: Value,
    pub keys: Vec<SpriteKey>,
}

pub struct SourceArt {
    pub sprites: Vec<(SpriteKey, SpriteRec)>,
    pub clips: Vec<(String, ArtClipRec)>,
    pub floor_frames: Vec<(String, Vec<SpriteKey>)>,
    pub objects: Json,
}

impl SourceArt {
    pub fn sprite(&self, k: &SpriteKey) -> &SpriteRec {
        &self.sprites.iter().find(|s| &s.0 == k).unwrap().1
    }
    pub fn clip(&self, name: &str) -> Result<&ArtClipRec> {
        self.clips
            .iter()
            .find(|c| c.0 == name)
            .map(|c| &c.1)
            .ok_or_else(|| format!("no art clip {name}"))
    }
}

fn texel_size(box_: &[f64; 4], project: f64) -> (i64, i64) {
    (
        ((box_[2] - box_[0]) * project).ceil() as i64,
        ((box_[3] - box_[1]) * project).ceil() as i64,
    )
}

/// `source_art(s, sc, actor)`: every art sprite the bank needs, with its size, box and owning clip.
pub fn source_art(sc: &Scene, source: &Source, fk: &Row) -> Result<SourceArt> {
    let fk_gid = fk.game_object;
    let animator = fk
        .part("tk2dSpriteAnimator")
        .ok_or("no tk2dSpriteAnimator")?;
    let library_o = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_o))?;
    let m = u(sc.world(*sc.go_transform.get(&fk_gid).ok_or("no transform")?))?;
    let sprite = fk.part("tk2dSprite").ok_or("no tk2dSprite")?;
    let ss = crate::recog::xy(sprite, "_scale")?;
    let scale = [(m[0][0] * ss[0]).abs(), (m[1][1] * ss[1]).abs()];
    if (scale[0] - scale[1]).abs() > 1e-6 {
        return err("False Knight art assumes a uniform transform scale");
    }
    let kids = children(sc, fk_gid)?;
    let (head, death_head) = (child(&kids, "Head")?, child(&kids, "Death Head")?);
    let ((head_pos, _), (_, dh_scale)) = (local(sc, head)?, local(sc, death_head)?);
    // The Head rides the body's transform, so its local offset is folded into its boxes; the Death Head leaves on its own body.
    let head_offset = (head_pos[0] * scale[0], head_pos[1] * scale[1]);
    let death_head_scale = scale[0] * dh_scale[0];
    let wave = shockwave_source(sc, source, fk_gid)?;
    let project = focal() / -CAM_Z;
    let by_name: Vec<(String, &Value)> = {
        let mut out: Vec<(String, &Value)> = Vec::new();
        for c in get(&library, "clips")?.list().unwrap_or(&[]) {
            let n = c.get("name").and_then(Value::str).unwrap_or_default();
            if n.is_empty() {
                continue;
            }
            match out.iter_mut().find(|x| x.0 == n) {
                Some(slot) => slot.1 = c,
                None => out.push((n, c)),
            }
        }
        out
    };
    let mut textures: HashMap<String, Arc<Image>> = HashMap::new();
    let mut collections: HashMap<String, Value> = HashMap::new();
    let mut sprites: Vec<(SpriteKey, SpriteRec)> = Vec::new();
    let mut clips: Vec<(String, ArtClipRec)> = Vec::new();
    for name in art_clips() {
        let spurt = name == SPURT_CLIP;
        let lib_o = if spurt { &wave.library } else { &library_o };
        let clip: Value = if spurt {
            wave.clip.clone()
        } else {
            (*by_name
                .iter()
                .find(|c| c.0 == name)
                .ok_or_else(|| format!("no clip {name}"))?
                .1)
                .clone()
        };
        let (factor, ox, oy) = if spurt {
            (1.0, 0.0, 0.0)
        } else if ["Head Idle", "Head Hit", "Head Spaz"].contains(&name.as_str()) {
            (scale[0], head_offset.0, head_offset.1)
        } else if name == "Death Head 1" || name == "Death Head 2" {
            (death_head_scale, 0.0, 0.0)
        } else {
            (scale[0], 0.0, 0.0)
        };
        let mut keys = Vec::new();
        for frame in get(&clip, "frames")?.list().unwrap_or(&[]) {
            let co = u(source.deref(&lib_o.file, get(frame, "spriteCollection")?))?;
            let sid = co.sid();
            if !collections.contains_key(&sid) {
                collections.insert(sid.clone(), u(source.read(&co))?);
            }
            let index = get(frame, "spriteId")?.int().unwrap_or(0);
            let key = SpriteKey::tk(&sid, index, factor, ox, oy);
            if !sprites.iter().any(|s| s.0 == key) {
                let (image, b) = tk_sprite(
                    source,
                    &co.file,
                    &collections[&sid],
                    index as usize,
                    &mut textures,
                )?;
                let b = [
                    b[0] * factor + ox,
                    b[1] * factor + oy,
                    b[2] * factor + ox,
                    b[3] * factor + oy,
                ];
                let (w, h) = texel_size(&b, project);
                if w > MAX_TEXTURE_AXIS as i64 || h > MAX_TEXTURE_AXIS as i64 {
                    return err(format!(
                        "False Knight frame {w}x{h} exceeds the texture axis"
                    ));
                }
                sprites.push((
                    key.clone(),
                    SpriteRec {
                        image,
                        box_: b,
                        w: w.max(1) as usize,
                        h: h.max(1) as usize,
                    },
                ));
            }
            keys.push(key);
        }
        clips.push((name, ArtClipRec { record: clip, keys }));
    }
    // Floor sprites: SpriteRenderers on `FK Floor`, placed relative to it.
    let floor_gid = crate::false_knight_art::additive_named_pub(sc, "FK Floor")?;
    let floor_children = children(sc, floor_gid)?;
    let fm = u(sc.world(*sc.go_transform.get(&floor_gid).ok_or("no transform")?))?;
    let anchor = [fm[0][3], fm[1][3]];
    let mut floor_frames: Vec<(String, Vec<SpriteKey>)> = Vec::new();
    for (state, names) in [
        ("Cracked", vec!["Cracked 1", "Cracked 2"]),
        ("Broken", vec!["Broken"]),
    ] {
        let mut keys = Vec::new();
        for name in names {
            let gid = child(&floor_children, name)?;
            let renderer = component(sc, gid, "SpriteRenderer")?;
            if get(get(renderer, "m_Color")?, "a")?.float() != Some(1.0)
                || get(renderer, "m_FlipX")?.truthy()
                || get(renderer, "m_FlipY")?.truthy()
            {
                return err(format!("floor sprite {name} is tinted or flipped"));
            }
            let wm = u(sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?))?;
            if wm[0][1].abs() > 1e-6 || wm[1][0].abs() > 1e-6 {
                return err(format!("floor sprite {name} is rotated"));
            }
            let sprite_obj = u(sc.deref(get(renderer, "m_Sprite")?))?;
            let (image, b) = native_sprite(source, &sprite_obj)?;
            let (sx, sy) = (wm[0][0], wm[1][1]);
            let b = [
                wm[0][3] - anchor[0] + b[0] * sx,
                wm[1][3] - anchor[1] + b[1] * sy,
                wm[0][3] - anchor[0] + b[2] * sx,
                wm[1][3] - anchor[1] + b[3] * sy,
            ];
            let (w, h) = texel_size(&b, project);
            let key = SpriteKey::Floor(name.to_string());
            if h > MAX_TEXTURE_AXIS as i64 {
                return err(format!("floor sprite {name} is {h} texels tall"));
            }
            sprites.push((
                key.clone(),
                SpriteRec {
                    image,
                    box_: b,
                    w: w.max(1) as usize,
                    h: h.max(1) as usize,
                },
            ));
            keys.push(key);
        }
        floor_frames.push((state.to_string(), keys));
    }
    let objects = source_objects(sc, source, fk)?;
    // `Death Head Speed` is read inside source_objects; this keeps the FSM variable lookup honest.
    let _ = (
        variables as fn(&Value) -> Vec<(String, &Value)>,
        num as fn(&Value) -> Option<f64>,
    );
    Ok(SourceArt {
        sprites,
        clips,
        floor_frames,
        objects,
    })
}

/// One sprite's residency: streamed or static, and its parts.
pub type Cut = (bool, Vec<[i64; 4]>);

/// What `plan` decides.
pub struct Plan {
    pub quantized: Vec<(SpriteKey, Quantized)>,
    pub cut: Vec<(SpriteKey, Cut)>,
    pub decisions: Vec<Json>,
    pub totals: Json,
}

impl Plan {
    fn cut_of(&self, k: &SpriteKey) -> Option<&Cut> {
        self.cut.iter().find(|c| &c.0 == k).map(|c| &c.1)
    }
}

fn jstr(s: &str) -> Json {
    Json::Str(s.to_string())
}

/// `plan(sprites, clips, floor_frames, scenery, quantize, priority, page_limit, stream_limit, label)`:
/// decide each sprite's residency and parts. Refuses rather than resizes.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    art: &SourceArt,
    scenery: &[(i64, i64)],
    quantize: Quantizer,
    priority: &[&str],
    page_limit: usize,
    stream_limit: i64,
    label: &str,
) -> Result<Plan> {
    let mut order: Vec<(String, Vec<SpriteKey>)> = Vec::new();
    for name in priority {
        let keys: Vec<SpriteKey> = if *name == "Floor" {
            art.floor_frames
                .iter()
                .flat_map(|f| f.1.iter().cloned())
                .collect()
        } else {
            art.clip(name)?.keys.clone()
        };
        let mut unique: Vec<SpriteKey> = Vec::new();
        for k in keys {
            if !unique.contains(&k) {
                unique.push(k);
            }
        }
        order.push((name.to_string(), unique));
    }
    let mut quantized: Vec<(SpriteKey, Quantized)> = Vec::new();
    let mut cut: Vec<(SpriteKey, Cut)> = Vec::new();
    let (mut static_keys, mut streamed_keys): (Vec<SpriteKey>, Vec<SpriteKey>) =
        (Vec::new(), Vec::new());
    let mut decisions = Vec::new();
    let mut stream_bytes = 0i64;
    let find_cut = |cut: &Vec<(SpriteKey, Cut)>, k: &SpriteKey| {
        cut.iter().find(|c| &c.0 == k).map(|c| c.1.clone())
    };
    for (name, keys) in order {
        let fresh: Vec<SpriteKey> = keys
            .into_iter()
            .filter(|k| find_cut(&cut, k).is_none())
            .collect();
        for k in &fresh {
            if !quantized.iter().any(|q| &q.0 == k) {
                let s = art.sprite(k);
                quantized.push((k.clone(), quantize(&s.image, s.w, s.h)?));
            }
        }
        let plane = |quantized: &Vec<(SpriteKey, Quantized)>, k: &SpriteKey| {
            quantized
                .iter()
                .find(|q| &q.0 == k)
                .unwrap()
                .1
                .plane
                .clone()
        };
        let mut trial: Vec<(SpriteKey, Vec<[i64; 4]>)> = Vec::new();
        for k in &fresh {
            let s = art.sprite(k);
            trial.push((
                k.clone(),
                decompose(&plane(&quantized, k), s.w, s.h, false)?,
            ));
        }
        let mut rects: Vec<(i64, i64)> = scenery.to_vec();
        for k in &static_keys {
            rects.extend(
                find_cut(&cut, k)
                    .unwrap()
                    .1
                    .iter()
                    .map(|r| (aligned(r[2]), r[3])),
            );
        }
        for t in &trial {
            rects.extend(t.1.iter().map(|r| (aligned(r[2]), r[3])));
        }
        let pages = if rects.is_empty() {
            0
        } else {
            dense_pack(
                &rects
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (r.0, r.1, i))
                    .collect::<Vec<_>>(),
            )?
            .0
        };
        if pages <= page_limit {
            let texel_bytes: i64 = trial.iter().flat_map(|t| t.1.iter()).map(rect_bytes).sum();
            for (k, parts) in trial {
                cut.push((k.clone(), (false, parts)));
                static_keys.push(k);
            }
            decisions.push(Json::Obj(vec![
                ("clip".into(), Json::Str(name.clone())),
                ("residency".into(), jstr("static")),
                ("sprites".into(), Json::Int(fresh.len() as i64)),
                ("scene_pages".into(), Json::Int(pages as i64)),
                ("texel_bytes".into(), Json::Int(texel_bytes)),
            ]));
            continue;
        }
        let mut cells: Vec<(SpriteKey, Vec<[i64; 4]>)> = Vec::new();
        for k in &fresh {
            let s = art.sprite(k);
            cells.push((k.clone(), decompose(&plane(&quantized, k), s.w, s.h, true)?));
        }
        let extra: i64 = cells.iter().flat_map(|c| c.1.iter()).map(rect_bytes).sum();
        if stream_bytes + extra > stream_limit {
            return err(format!("{label} clip {name} fits neither the {page_limit} scene pages nor the {stream_limit}-byte stream budget ({stream_bytes}+{extra})"));
        }
        stream_bytes += extra;
        for (k, parts) in cells {
            cut.push((k.clone(), (true, parts)));
            streamed_keys.push(k);
        }
        decisions.push(Json::Obj(vec![
            ("clip".into(), Json::Str(name.clone())),
            ("residency".into(), jstr("streamed")),
            ("sprites".into(), Json::Int(fresh.len() as i64)),
            ("scene_pages".into(), Json::Int(pages as i64)),
            ("texel_bytes".into(), Json::Int(extra)),
        ]));
    }
    let mut rects: Vec<(i64, i64)> = scenery.to_vec();
    for k in &static_keys {
        rects.extend(
            find_cut(&cut, k)
                .unwrap()
                .1
                .iter()
                .map(|r| (aligned(r[2]), r[3])),
        );
    }
    let (final_pages, _) = dense_pack(
        &rects
            .iter()
            .enumerate()
            .map(|(i, r)| (r.0, r.1, i))
            .collect::<Vec<_>>(),
    )?;
    let sum_bytes = |keys: &Vec<SpriteKey>| -> i64 {
        keys.iter()
            .map(|k| {
                find_cut(&cut, k)
                    .unwrap()
                    .1
                    .iter()
                    .map(rect_bytes)
                    .sum::<i64>()
            })
            .sum()
    };
    let parts = |keys: &Vec<SpriteKey>| -> i64 {
        keys.iter()
            .map(|k| find_cut(&cut, k).unwrap().1.len() as i64)
            .sum()
    };
    let totals = Json::Obj(vec![
        ("scene_pages".into(), Json::Int(final_pages as i64)),
        ("stream_bytes".into(), Json::Int(stream_bytes)),
        ("static_bytes".into(), Json::Int(sum_bytes(&static_keys))),
        ("static_parts".into(), Json::Int(parts(&static_keys))),
        ("streamed_parts".into(), Json::Int(parts(&streamed_keys))),
        ("sprites".into(), Json::Int(cut.len() as i64)),
    ]);
    Ok(Plan {
        quantized,
        cut,
        decisions,
        totals,
    })
}

/// What `append_bank` returns: the guest-linked tables.
pub struct BankTables {
    pub anchor: usize,
    pub sprite_rows: Vec<(i64, i64, bool)>,
    pub clip_rows: Vec<crate::false_knight_art::ClipRow>,
    pub sequence: Vec<i64>,
    pub floor_rows: Vec<(String, Vec<i64>)>,
}

/// `append_bank(atlas, frames, clips, sprites, art_clips, floor_frames, quantized, cut, names, anchor_name)`.
#[allow(clippy::too_many_arguments)]
pub fn append_bank(
    atlas: &mut Atlas,
    frames: &mut Vec<Frame>,
    clips: &mut Vec<Clip>,
    art: &SourceArt,
    plan: &Plan,
    names: &[String],
    anchor_name: &str,
) -> Result<BankTables> {
    let first = frames.len();
    let mut sprite_index: Vec<(SpriteKey, usize)> = Vec::new();
    let mut sprite_rows: Vec<(i64, i64, bool)> = Vec::new();
    let mut ordered: Vec<SpriteKey> = Vec::new();
    for name in names {
        ordered.extend(art.clip(name)?.keys.iter().cloned());
    }
    for (_, keys) in &art.floor_frames {
        ordered.extend(keys.iter().cloned());
    }
    let mut seen: Vec<SpriteKey> = Vec::new();
    for key in ordered {
        if seen.contains(&key) {
            continue;
        }
        seen.push(key.clone());
        let (streamed, parts) = plan.cut_of(&key).ok_or("sprite without a plan")?.clone();
        let rec = art.sprite(&key);
        let q = &plan
            .quantized
            .iter()
            .find(|q| q.0 == key)
            .ok_or("sprite without quantized texels")?
            .1;
        let box_q16 = [
            py_round(rec.box_[0] * 65536.0),
            py_round(rec.box_[1] * 65536.0),
            py_round(rec.box_[2] * 65536.0),
            py_round(rec.box_[3] * 65536.0),
        ];
        let start = (frames.len() - first) as i64;
        for rect in &parts {
            let (x, y, pw, ph) = (
                rect[0] as usize,
                rect[1] as usize,
                rect[2] as usize,
                rect[3] as usize,
            );
            let texture = atlas.add_quantized(
                pw,
                ph,
                &q.palette,
                &Atlas::pack_plane(&q.plane, rec.w, x, y, pw, ph),
                streamed,
                false,
            )?;
            let b = part_box(box_q16, rec.w as i64, rec.h as i64, *rect);
            frames.push(Frame {
                texture,
                box_: [
                    b[0] as f64 / 65536.0,
                    b[1] as f64 / 65536.0,
                    b[2] as f64 / 65536.0,
                    b[3] as f64 / 65536.0,
                ],
                sprite: key.repr(),
                box_q16: Some(b),
                event: Value::Map(Vec::new()),
            });
        }
        sprite_index.push((key, sprite_rows.len()));
        sprite_rows.push((start, parts.len() as i64, streamed));
    }
    let anchor = clips.len();
    clips.push(Clip {
        name: anchor_name.to_string(),
        start: first,
        count: (frames.len() - first).max(1),
        fps: 1.0,
        wrap: 2,
        loop_start: 0,
    });
    let (mut clip_rows, mut sequence) = (Vec::new(), Vec::new());
    for name in names {
        let rec = art.clip(name)?;
        let fps = get(&rec.record, "fps")?.float().ok_or("fps")?;
        let loop_start = rec
            .record
            .get("loopStart")
            .and_then(Value::int)
            .unwrap_or(0);
        clip_rows.push((
            sequence.len() as i64,
            rec.keys.len() as i64,
            py_round(fps * 65536.0),
            guest_wrap(&rec.record)?,
            loop_start,
            name.clone(),
        ));
        for k in &rec.keys {
            sequence.push(sprite_index.iter().find(|s| &s.0 == k).unwrap().1 as i64);
        }
    }
    let floor_rows: Vec<(String, Vec<i64>)> = art
        .floor_frames
        .iter()
        .map(|(state, keys)| {
            (
                state.clone(),
                keys.iter()
                    .map(|k| sprite_index.iter().find(|s| &s.0 == k).unwrap().1 as i64)
                    .collect(),
            )
        })
        .collect();
    Ok(BankTables {
        anchor,
        sprite_rows,
        clip_rows,
        sequence,
        floor_rows,
    })
}

/// `scenery_rects(base_packs)`: the distinct static textures of a scene's views, as the scene bank packs them.
pub fn scenery_rects(base_packs: &[Vec<u8>]) -> Result<Vec<(i64, i64)>> {
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut rects = Vec::new();
    for raw in base_packs {
        for (i, blob) in crate::region_delta::textures(raw)?.into_iter().enumerate() {
            let page = u16::from_le_bytes([raw[40 + i * 16], raw[41 + i * 16]]);
            if page == 65535 || seen.contains(&blob) {
                continue;
            }
            let (w, h) = (
                u16::from_le_bytes([blob[0], blob[1]]) as i64,
                u16::from_le_bytes([blob[2], blob[3]]) as i64,
            );
            seen.push(blob);
            rects.push((aligned(w), h));
        }
    }
    Ok(rects)
}

/// A region of the False Knight's scene, as the cook's report rows describe it.
pub struct BankRegion {
    pub chunk_id: i64,
    pub scene_id: i64,
    pub edge_sources: Vec<String>,
}

/// What `cook_scene_bank` hands back for the caller to finish once the bank's clip base is known.
pub struct BankWriter {
    anchor: usize,
    tables: BankTables,
    objects: Json,
    bindings: Vec<crate::false_knight_art::Binding>,
    scene_id: i64,
    report: Vec<(String, Json)>,
}

impl BankWriter {
    pub fn report(&self) -> Json {
        Json::Obj(self.report.clone())
    }

    /// `write(clip_base)`: data/false_knight_art.rs, data/false_knight_floor.rs and the art report.
    pub fn write(&mut self, root: &std::path::Path, clip_base: usize) -> Result<()> {
        let table = crate::false_knight_art::rust_table(
            self.anchor as i64 + clip_base as i64,
            &self.tables.sprite_rows,
            &self.tables.clip_rows,
            &self.tables.sequence,
            &self.tables.floor_rows,
            &self.objects,
        )?;
        let floor = crate::false_knight_art::rust_bindings(&self.bindings, self.scene_id);
        for (name, text) in [
            ("false_knight_art.rs", table),
            ("false_knight_floor.rs", floor),
        ] {
            let path = root.join("data").join(name);
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
                std::fs::create_dir_all(root.join("data")).map_err(|e| e.to_string())?;
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        match self.report.iter_mut().find(|f| f.0 == "anchor_clip") {
            Some(slot) => slot.1 = Json::Int((self.anchor + clip_base) as i64),
            None => self.report.push((
                "anchor_clip".into(),
                Json::Int((self.anchor + clip_base) as i64),
            )),
        }
        let dir = root.join(".hkpsx/false-knight");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(
            dir.join("art.json"),
            crate::pyjson::dumps(&Json::Obj(self.report.clone())) + "\n",
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// `cook_scene_bank(s, sc, actor, rows, atlas, frames, clips)`: append the whole bank to the scene's shared actor atlas.
#[allow(clippy::too_many_arguments)]
pub fn cook_scene_bank(
    sc: &Scene,
    source: &Source,
    fk: &Row,
    rows: &[BankRegion],
    root: &std::path::Path,
    atlas: &mut Atlas,
    frames: &mut Vec<Frame>,
    clips: &mut Vec<Clip>,
    quantize: Quantizer,
) -> Result<BankWriter> {
    let art = source_art(sc, source, fk)?;
    let mut base = Vec::new();
    for r in rows {
        let path = root.join(format!("data/regions/region-{:03}/room.hk", r.chunk_id));
        base.push(std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?);
    }
    let scenery = scenery_rects(&base)?;
    let planned = plan(
        &art,
        &scenery,
        quantize,
        &PRIORITY,
        SCENE_PAGE_LIMIT,
        STREAM_BYTES_LIMIT,
        "False Knight",
    )?;
    let names = art_clips();
    let tables = append_bank(
        atlas,
        frames,
        clips,
        &art,
        &planned,
        &names,
        "False Knight parts",
    )?;
    let region_rows: Vec<(i64, Vec<String>)> = rows
        .iter()
        .map(|r| (r.chunk_id, r.edge_sources.clone()))
        .collect();
    let bindings =
        crate::false_knight_art::region_bindings(&region_rows, &art.objects, &|chunk| {
            let path = root.join(format!("data/regions/region-{chunk:03}/scene.json"));
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let scene = crate::pyjson::parse(&text)?;
            let Json::Obj(fields) = scene else {
                return err("scene.json is not an object");
            };
            let Some((_, Json::List(draws))) = fields.into_iter().find(|f| f.0 == "draws") else {
                return err("scene.json lacks draws");
            };
            Ok(draws
                .into_iter()
                .map(|d| {
                    if let Json::Obj(f) = d {
                        f.into_iter()
                            .find(|x| x.0 == "source")
                            .map(|x| {
                                if let Json::Str(s) = x.1 {
                                    s
                                } else {
                                    String::new()
                                }
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                })
                .collect())
        })?;
    let mut report: Vec<(String, Json)> = vec![("decisions".into(), Json::List(planned.decisions))];
    if let Json::Obj(totals) = planned.totals {
        report.extend(totals);
    }
    report.push((
        "art_clips".into(),
        Json::List(names.iter().map(|n| Json::Str(n.clone())).collect()),
    ));
    report.push((
        "anchor_clip_in_bank".into(),
        Json::Int(tables.anchor as i64),
    ));
    report.push((
        "bindings".into(),
        Json::Obj(
            bindings
                .iter()
                .map(|b| {
                    (
                        b.name.clone(),
                        Json::Int(b.boxes.as_ref().map_or(b.rows.len(), Vec::len) as i64),
                    )
                })
                .collect(),
        ),
    ));
    // Python recorded the hash of host/false_knight_art.py; the identity of this cooker is its own source.
    report.push((
        "code_sha256".into(),
        Json::Str(crate::cook_audio::sha(
            &[
                include_bytes!("false_knight_art.rs").as_slice(),
                include_bytes!("fk_bank.rs").as_slice(),
            ]
            .concat(),
        )),
    ));
    let scene_id = rows
        .first()
        .map(|r| r.scene_id)
        .ok_or("a False Knight bank needs at least one region")?;
    Ok(BankWriter {
        anchor: tables.anchor,
        tables,
        objects: art.objects,
        bindings,
        scene_id,
        report,
    })
}
