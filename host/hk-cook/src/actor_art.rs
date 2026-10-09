//! Appending actors' original clips to the room atlas. Ported from `append_actor_art`,
//! `append_barrel_art` and `guest_wrap` in host/cook.py.

use crate::actors::Row;
use crate::atlas::{Atlas, MAX_FRAME_TILES, MAX_TEXTURE_AXIS, SLOT_PIXELS};
use crate::common::{err, get, py_round, Result};
use crate::cook::{focal, native_sprite, tk_sprite, CAM_Z};
use crate::cook_audio::u;
use crate::pyjson::Json;
use hk_pil::Image;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// `BARREL_CLIP`.
const BARREL_CLIP: &str = "Falling Barrel";

/// One cooked frame record: the atlas texture, its world box and the source frame it came from.
#[derive(Clone, Debug)]
pub struct Frame {
    pub texture: usize,
    pub box_: [f64; 4],
    pub sprite: String,
    /// The tk2d frame record (`{}` for a frame with none).
    pub event: Value,
}

/// One cooked clip record.
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub start: usize,
    pub count: usize,
    pub fps: f64,
    pub wrap: i64,
    pub loop_start: i64,
}

/// An actor while its art is cooked: what `append_actor_art` writes onto the actor dict.
pub struct ArtActor<'a> {
    pub row: &'a Row,
    /// `movement_supported`; a refused actor is switched off here.
    pub supported: bool,
    pub limitations: Vec<String>,
    /// `<slot>_clip` bindings in the order they were written.
    pub clips: Vec<(String, i64)>,
    pub visual_scale: Option<[f64; 2]>,
    /// `actor['corpse']`, once the corpse art is cooked.
    pub corpse: Option<Json>,
}

impl<'a> ArtActor<'a> {
    pub fn new(row: &'a Row) -> ArtActor<'a> {
        ArtActor { row, supported: row.supported, limitations: row.limitations.clone(), clips: Vec::new(), visual_scale: None, corpse: None }
    }

    pub fn set_clip(&mut self, key: &str, value: i64) {
        match self.clips.iter_mut().find(|c| c.0 == key) {
            Some(slot) => slot.1 = value,
            None => self.clips.push((key.to_string(), value)),
        }
    }

    pub fn control(&self) -> Option<&Json> {
        self.row.control.as_ref().map(|c| &c.1)
    }
}

/// The atlas and the lists the actor, corpse and barrel art append to.
pub struct ArtBank {
    pub atlas: Atlas,
    pub frames: Vec<Frame>,
    pub clips: Vec<Clip>,
}

impl ArtBank {
    pub fn new(atlas: Atlas) -> ArtBank {
        ArtBank { atlas, frames: Vec::new(), clips: Vec::new() }
    }
}

/// `guest_wrap(clip)`: tk2d wrapMode to the guest's loop/loop-section/once set.
pub fn guest_wrap(clip: &Value) -> Result<i64> {
    guest_wrap_of(clip, get(clip, "frames")?.list().map_or(0, <[Value]>::len))
}

/// `guest_wrap` for a clip whose frame list has been thinned to `count` frames.
pub fn guest_wrap_of(clip: &Value, count: usize) -> Result<i64> {
    let mode = get(clip, "wrapMode")?.int().unwrap_or(-1);
    match mode {
        0..=2 => Ok(mode),
        6 if count == 1 => Ok(2),
        _ => err(format!("unsupported tk2d wrap mode {mode} for {count}-frame clip {}", get(clip, "name").ok().and_then(Value::str).unwrap_or_default())),
    }
}

fn cj<'a>(c: &'a Json, key: &str) -> Option<&'a Json> {
    match c {
        Json::Obj(f) => f.iter().find(|k| k.0 == key).map(|k| &k.1),
        _ => None,
    }
}

fn jstr(c: &Json, key: &str) -> Result<String> {
    match cj(c, key) {
        Some(Json::Str(s)) => Ok(s.clone()),
        _ => err(format!("control lacks {key}")),
    }
}

/// `image.putalpha(image.getchannel('A').point(lambda a: round(a * alpha)))`.
pub fn scale_alpha(image: &mut Image, alpha: f64) {
    let table: Vec<u8> = (0..256).map(|a| py_round(a as f64 * alpha).clamp(0, 255) as u8).collect();
    for px in image.data.chunks_exact_mut(4) {
        px[3] = table[px[3] as usize];
    }
}

fn frame_tiles(w: i64, h: i64) -> i64 {
    -(-w).div_euclid(SLOT_PIXELS as i64) * -(-h).div_euclid(SLOT_PIXELS as i64)
}

/// The slot-to-clip bindings an actor's controller cooks (the chain in `append_actor_art`).
fn bindings_for(control: &Json, library: &Value) -> Result<Vec<(String, String)>> {
    let pairs = |p: &[(&str, &str)]| -> Vec<(String, String)> { p.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    if let Some(Json::Obj(b)) = cj(control, "art_bindings") {
        return b.iter().map(|(k, v)| if let Json::Str(s) = v { Ok((k.clone(), s.clone())) } else { err("art binding is not a clip name") }).collect();
    }
    let kind = jstr(control, "kind")?;
    Ok(match kind.as_str() {
        "ZombieSwipeWalker" => {
            let leap = cj(control, "parameters").and_then(|p| cj(p, "attack")).and_then(|a| cj(a, "kind")) == Some(&Json::Str("Leap".into()));
            let mut b = if leap {
                // Leaper: the Attack clip anticipates and keeps playing through the jump (lunge slot), Land is the cooldown clip.
                pairs(&[("walk", "Walk"), ("turn", "Turn"), ("idle", "Idle"), ("anticipate", "Attack"), ("lunge", "Attack"), ("cooldown", "Land")])
            } else {
                pairs(&[("walk", "Walk"), ("turn", "Turn"), ("idle", "Idle"), ("anticipate", "Attack Anticipate"), ("lunge", "Attack Lunge"), ("cooldown", "Attack Cooldown")])
            };
            // Barger and Hornhead libraries have no Fall clip; the controller never plays it.
            if get(library, "clips")?.list().unwrap_or(&[]).iter().any(|c| c.get("name").and_then(Value::str).as_deref() == Some("Fall")) {
                b.push(("fall".into(), "Fall".into()));
            }
            b
        }
        "WalkLeftRight" => vec![("walk".into(), jstr(control, "walk_clip_name")?), ("turn".into(), jstr(control, "turn_clip_name")?)],
        // Walk continues through source turns; the walk clip doubles as `turn_clip`.
        "Climber" => pairs(&[("walk", "Walk"), ("turn", "Walk"), ("stun", "Stun")]),
        // Idle doubles as `walk_clip`; TurnToIdle is the idle-facing turn.
        "Vengefly" => pairs(&[("walk", "Idle"), ("turn", "TurnToIdle"), ("startle", "Startle"), ("chase", "Chase"), ("turn_fly", "TurnToFly")]),
        "Gruzzer" => pairs(&[("walk", "Fly"), ("turn", "Fly")]),
        // Idle doubles as `walk_clip`; the turn slot is unused and holds Idle too.
        "Baldur" => pairs(&[("walk", "Idle"), ("turn", "Idle"), ("start", "Start"), ("roll", "Roll"), ("stop", "Stop")]),
        "Aspid" => pairs(&[("walk", "Fly"), ("turn", "TurnToFly"), ("fire", "Fire Long")]),
        // One looping Idle and no movement: the walk and turn slots hold it too.
        "EggSac" => pairs(&[("walk", "Idle"), ("turn", "Idle"), ("idle", "Idle")]),
        other => return err(format!("unsupported actor art controller: {other}")),
    })
}

fn named_clip<'a>(library: &'a Value, name: &str) -> Result<&'a Value> {
    get(library, "clips")?.list().unwrap_or(&[]).iter().find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name)).ok_or_else(|| format!("no clip {name}"))
}

fn num(v: &Value) -> Result<f64> {
    v.float().ok_or_else(|| "not a number".to_string())
}

/// `append_actor_art(s, sc, actors, atlas, frames, clips)` without the corpse and barrel passes
/// (`append_actor_art_bodies`), which the caller sequences.
pub fn append_actor_art_bodies(sc: &Scene, source: &Source, actors: &mut [ArtActor], bank: &mut ArtBank, quantize: crate::atlas::Quantizer) -> Result<()> {
    let mut textures: HashMap<String, Arc<Image>> = HashMap::new();
    let mut collections: HashMap<String, Value> = HashMap::new();
    let mut cache: HashMap<(String, i64, u64, u64, i64), (usize, [f64; 4])> = HashMap::new();
    let mut clip_cache: HashMap<(String, String, u64, u64, i64), usize> = HashMap::new();
    let scale = focal() / -CAM_Z;
    for actor in actors.iter_mut() {
        if !actor.supported {
            continue;
        }
        let row = actor.row;
        let control = actor.control().ok_or("supported actor without a control")?.clone();
        let animator = row.part("tk2dSpriteAnimator").ok_or("no tk2dSpriteAnimator")?;
        let library_o = u(sc.deref(get(animator, "library")?))?;
        let library = u(source.read(&library_o))?;
        let m = u(sc.world(*sc.go_transform.get(&row.game_object).ok_or("actor has no transform")?))?;
        let sprite = row.part("tk2dSprite").ok_or("no tk2dSprite")?;
        let sprite_scale = get(sprite, "_scale")?;
        let sx = (m[0][0] * num(get(sprite_scale, "x")?)?).abs();
        let sy = (m[1][1] * num(get(sprite_scale, "y")?)?).abs();
        let color_a = num(get(get(sprite, "_color")?, "a")?)?;
        let alpha = py_round(color_a * 255.0);
        let bindings = bindings_for(&control, &library)?;
        let library_sid = library_o.sid();
        let collection_for = |collections: &mut HashMap<String, Value>, frame: &Value| -> Result<(hk_unity::Obj, String)> {
            let co = u(source.deref(&library_o.file, get(frame, "spriteCollection")?))?;
            let sid = co.sid();
            if !collections.contains_key(&sid) {
                collections.insert(sid.clone(), u(source.read(&co))?);
            }
            Ok((co, sid))
        };
        // The whole-actor pre-pass: a frame the animation cache cannot hold makes the actor an explicit unsupported record.
        let mut refused: Vec<(i64, i64)> = Vec::new();
        for (_, name) in &bindings {
            let clip = named_clip(&library, name)?;
            for frame in get(clip, "frames")?.list().unwrap_or(&[]) {
                let (co, sid) = collection_for(&mut collections, frame)?;
                let (_, b) = tk_sprite(source, &co.file, &collections[&sid], get(frame, "spriteId")?.int().unwrap_or(0) as usize, &mut textures)?;
                let (w, h) = (((b[2] - b[0]) * sx * scale).ceil() as i64, ((b[3] - b[1]) * sy * scale).ceil() as i64);
                if frame_tiles(w, h) > MAX_FRAME_TILES as i64 || w > MAX_TEXTURE_AXIS as i64 || h > MAX_TEXTURE_AXIS as i64 {
                    refused.push((w, h));
                }
            }
        }
        refused.sort();
        if let Some(&(w, h)) = refused.last() {
            let reason = if w > MAX_TEXTURE_AXIS as i64 || h > MAX_TEXTURE_AXIS as i64 {
                format!("exceeds the {MAX_TEXTURE_AXIS}-pixel texture axis and would be resampled")
            } else {
                format!("binds {} of the {MAX_FRAME_TILES} animation slots a frame may hold", frame_tiles(w, h))
            };
            actor.supported = false;
            actor.limitations.push(format!("actor frame {w}x{h} {reason}; art not cooked"));
            continue;
        }
        // A recognizer may ask for one palette per clip rather than one per frame (`shared_palette`).
        let shared = cj(&control, "shared_palette").is_some_and(|j| matches!(j, Json::Bool(true)));
        let (sx_bits, sy_bits) = (sx.to_bits(), sy.to_bits());
        for (kind, name) in &bindings {
            let clip_key = (library_sid.clone(), name.clone(), sx_bits, sy_bits, alpha);
            if !clip_cache.contains_key(&clip_key) && shared {
                let clip = named_clip(&library, name)?;
                let start = bank.frames.len();
                let mut pending: Vec<(Image, f64, f64, [f64; 4], String, Value)> = Vec::new();
                for frame in get(clip, "frames")?.list().unwrap_or(&[]) {
                    let (co, sid) = collection_for(&mut collections, frame)?;
                    let index = get(frame, "spriteId")?.int().unwrap_or(0);
                    let (mut image, b) = tk_sprite(source, &co.file, &collections[&sid], index as usize, &mut textures)?;
                    scale_alpha(&mut image, color_a);
                    let b = [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
                    pending.push((image, (b[2] - b[0]) * scale, (b[3] - b[1]) * scale, b, format!("{sid}:{index}"), frame.clone()));
                }
                let items: Vec<(Image, f64, f64)> = pending.iter().map(|p| (p.0.clone(), p.1, p.2)).collect();
                let textures_out = bank.atlas.add_frames_shared(&items, quantize)?;
                for (texture, p) in textures_out.iter().zip(&pending) {
                    bank.frames.push(Frame { texture: *texture, box_: p.3, sprite: p.4.clone(), event: p.5.clone() });
                }
                clip_cache.insert(clip_key.clone(), bank.clips.len());
                bank.clips.push(Clip { name: format!("{library_sid}/{name}"), start, count: get(clip, "frames")?.list().map_or(0, <[Value]>::len), fps: num(get(clip, "fps")?)?, wrap: guest_wrap(clip)?, loop_start: clip.get("loopStart").and_then(Value::int).unwrap_or(0) });
            }
            if !clip_cache.contains_key(&clip_key) {
                let source_clip = named_clip(&library, name)?;
                let start = bank.frames.len();
                // A recognizer may keep every `stride`th frame of a clip at the matching fraction of its rate.
                let stride = cj(&control, "frame_stride").and_then(|s| cj(s, name)).map_or(1, |j| if let Json::Int(i) = j { *i } else { 1 });
                let mut frame_list: Vec<Value> = get(source_clip, "frames")?.list().unwrap_or(&[]).to_vec();
                let (mut fps, mut loop_start) = (num(get(source_clip, "fps")?)?, source_clip.get("loopStart").and_then(Value::int).unwrap_or(0));
                if stride != 1 {
                    frame_list = frame_list.into_iter().step_by(stride as usize).collect();
                    fps /= stride as f64;
                    loop_start = loop_start.div_euclid(stride);
                }
                for frame in &frame_list {
                    let (co, sid) = collection_for(&mut collections, frame)?;
                    let index = get(frame, "spriteId")?.int().unwrap_or(0);
                    let key = (sid.clone(), index, sx_bits, sy_bits, alpha);
                    if !cache.contains_key(&key) {
                        let (mut image, b) = tk_sprite(source, &co.file, &collections[&sid], index as usize, &mut textures)?;
                        scale_alpha(&mut image, color_a);
                        let b = [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
                        // add_tiled returns the frame's first texture; a frame inside one slot is still a single streamed texture.
                        let texture = bank.atlas.add_tiled(&image, (b[2] - b[0]) * scale, (b[3] - b[1]) * scale, quantize)?;
                        cache.insert(key.clone(), (texture, b));
                    }
                    let (texture, b) = cache[&key];
                    bank.frames.push(Frame { texture, box_: b, sprite: format!("{sid}:{index}"), event: frame.clone() });
                }
                clip_cache.insert(clip_key.clone(), bank.clips.len());
                let wrap = guest_wrap_of(source_clip, frame_list.len())?;
                bank.clips.push(Clip { name: format!("{library_sid}/{name}"), start, count: frame_list.len(), fps, wrap, loop_start });
            }
            actor.set_clip(&format!("{kind}_clip"), clip_cache[&clip_key] as i64);
        }
        actor.visual_scale = Some([sx, sy]);
    }
    Ok(())
}

/// `append_barrel_art(s, actors, atlas, frames, clips)`: `FK Barrel Summon`'s pooled `Falling Barrel`.
pub fn append_barrel_art(source: &Source, actors: &mut [ArtActor], bank: &mut ArtBank, quantize: crate::atlas::Quantizer) -> Result<()> {
    let scale = focal() / -CAM_Z;
    for actor in actors.iter_mut() {
        let Some(control) = actor.control().cloned() else { continue };
        if !actor.supported || cj(&control, "kind") != Some(&Json::Str("FalseKnight".into())) {
            continue;
        }
        let barrel = cj(&control, "barrel").ok_or("False Knight without a barrel")?;
        let sprite = jstr(barrel, "sprite")?;
        let (name, path_id) = sprite.rsplit_once(':').ok_or("barrel sprite id")?;
        let file = u(source.file(name))?;
        let obj = u(source.object(&file, path_id.parse().map_err(|_| "barrel path id")?))?;
        let (image, b) = native_sprite(source, &obj)?;
        actor.set_clip("barrel_clip", bank.clips.len() as i64);
        bank.clips.push(Clip { name: format!("{}/{BARREL_CLIP}", jstr(barrel, "source")?), start: bank.frames.len(), count: 1, fps: 1.0, wrap: 2, loop_start: 0 });
        let texture = bank.atlas.add(&image, (b[2] - b[0]) * scale, (b[3] - b[1]) * scale, true, quantize)?;
        bank.frames.push(Frame { texture, box_: b, sprite, event: Value::Map(Vec::new()) });
    }
    Ok(())
}
