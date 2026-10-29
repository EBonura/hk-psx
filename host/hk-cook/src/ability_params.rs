//! Desolate Dive, Howling Wraiths and the nail arts, from the Hero's FSMs and the `Spells` and
//! `Attacks` children of the Knight prefab (docs/SPELLS.md): the numbers of `hk_sim::QuakeParams`,
//! `ScreamParams` and `NailArtParams`, written to data/ability_params.rs and
//! .hkpsx/ability-params.json.
//!
//! Every time is the install's own: a clip's frames over its rate, an FSM `Wait`, a HeroController
//! constant. Every shape is a collider of the prefab, placed through its parents' positions and
//! scales into Q16 units about the Knight's origin, as the prefab authors it (it faces left).
//! No retail payload is embedded.

use crate::breakables::{k, kf, kl, ks};
use crate::combat::ticks;
use crate::common::{err, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::dream_nail::knight_animation;
use crate::focus::{assert_that, field, hero, hero_constants, hero_fsm, FsmReader};
use crate::music::dump;
use hk_unity::serialized::SerializedFile;
use hk_unity::{Source, Value};
use std::path::Path;
use std::sync::Arc;

/// The library of tk2d clips the spell effects play (`Quake Blast`, `Scream1`, ...).
const SPELL_LIBRARY: i64 = 23263;
/// Unity's fixed timestep: `DecelerateV2` slows a velocity once per physics step.
const FIXED_HZ: f64 = 50.0;
/// Corners a shape may have (hk_sim::strike::MAX_POINTS).
const MAX_POINTS: usize = 5;

struct Prefab<'a> {
    source: &'a Source,
    file: Arc<SerializedFile>,
}

/// A collider's shape as corners about the Knight, in world units.
type Corners = Vec<[f64; 2]>;

impl<'a> Prefab<'a> {
    fn game_object(&self, id: i64) -> Result<Value> {
        u(self.source.read(&u(self.source.object(&self.file, id))?))
    }
    /// The first component of `class_id` of a GameObject's tree.
    fn component(&self, go: &Value, class_id: i32) -> Result<Option<Value>> {
        for c in kl(go, "m_Component")? {
            let o = u(self.source.deref(&self.file, k(c, "component")?))?;
            if o.class_id() == class_id {
                return Ok(Some(u(self.source.read(&o))?));
            }
        }
        Ok(None)
    }
    /// A PlayMaker FSM of that name on the GameObject.
    fn fsm(&self, go: &Value, name: &str) -> Result<Option<Value>> {
        for c in kl(go, "m_Component")? {
            let o = u(self.source.deref(&self.file, k(c, "component")?))?;
            if o.class_id() == 114 && u(self.source.typename(&o))? == "PlayMakerFSM" {
                let fsm = k(&u(self.source.read(&o))?, "fsm")?.clone();
                if ks(&fsm, "name")? == name {
                    return Ok(Some(fsm));
                }
            }
        }
        Ok(None)
    }
    /// The child GameObject of `go` named `name`: its id and tree.
    fn child(&self, go: &Value, name: &str) -> Result<Value> {
        let t = self
            .component(go, 4)?
            .ok_or_else(|| format!("{} has no Transform", ks(go, "m_Name").unwrap_or_default()))?;
        for ch in kl(&t, "m_Children")? {
            let child_t = u(self.source.deref(&self.file, ch))?;
            let child_t = u(self.source.read(&child_t))?;
            let g = u(self.source.deref(&self.file, k(&child_t, "m_GameObject")?))?;
            let g = u(self.source.read(&g))?;
            if ks(&g, "m_Name")? == name {
                return Ok(g);
            }
        }
        err(format!("no child {name} of {}", ks(go, "m_Name")?))
    }
    /// A GameObject's local position and scale.
    fn local(&self, go: &Value) -> Result<([f64; 2], [f64; 2])> {
        let t = self.component(go, 4)?.ok_or("no Transform")?;
        let (p, s) = (k(&t, "m_LocalPosition")?, k(&t, "m_LocalScale")?);
        Ok(([kf(p, "x")?, kf(p, "y")?], [kf(s, "x")?, kf(s, "y")?]))
    }
    /// The corners of the PolygonCollider2D or BoxCollider2D of `go`, carried up through `chain`
    /// (its parents' GameObjects, nearest first, ending below the Knight) into the Knight's frame.
    fn corners(&self, go: &Value, chain: &[&Value]) -> Result<Corners> {
        let mut pts: Corners = if let Some(c) = self.component(go, 60)? {
            let paths = kl(k(&c, "m_Points")?, "m_Paths")?;
            assert_that(paths.len() == 1, "one polygon path")?;
            let off = k(&c, "m_Offset")?;
            let mut v = Vec::new();
            for p in paths[0].list().ok_or("polygon path")? {
                v.push([kf(p, "x")? + kf(off, "x")?, kf(p, "y")? + kf(off, "y")?]);
            }
            v
        } else if let Some(c) = self.component(go, 61)? {
            let (o, s) = (k(&c, "m_Offset")?, k(&c, "m_Size")?);
            let (hx, hy) = (kf(s, "x")? / 2.0, kf(s, "y")? / 2.0);
            let (ox, oy) = (kf(o, "x")?, kf(o, "y")?);
            vec![[ox - hx, oy - hy], [ox + hx, oy - hy], [ox + hx, oy + hy], [ox - hx, oy + hy]]
        } else {
            return err(format!("{} has no collider", ks(go, "m_Name")?));
        };
        for g in std::iter::once(go).chain(chain.iter().copied()) {
            let (pos, scale) = self.local(g)?;
            for p in &mut pts {
                *p = [pos[0] + scale[0] * p[0], pos[1] + scale[1] * p[1]];
            }
        }
        Ok(pts)
    }
    /// `damageDealt` with no charm: the first value of the object's `Set Damage` FSM, else the
    /// `damages_enemy` FSM's own.
    fn damage(&self, go: &Value) -> Result<i64> {
        if let Some(fsm) = self.fsm(go, "Set Damage")? {
            let r = FsmReader::new(&fsm)?;
            let set = r.actions("Set Damage", "SetFsmInt")?;
            if let Some(first) = set.first() {
                return r
                    .scalar(field(first, "setValue")?)?
                    .int()
                    .ok_or_else(|| "setValue is not an int".to_string());
            }
        }
        let fsm = self.fsm(go, "damages_enemy")?.ok_or("no damages_enemy")?;
        let r = FsmReader::new(&fsm)?;
        r.variables
            .iter()
            .find(|e| e.0 == "damageDealt")
            .and_then(|e| e.1.int())
            .ok_or_else(|| "damageDealt".to_string())
    }
}

/// The frames of a clip of the spell library or the Knight's own table, by name: (frames, fps,
/// frame indices whose `triggerEvent` is set).
fn clip(table: &Value, name: &str) -> Result<(usize, f64, Vec<usize>)> {
    for c in kl(table, "clips")? {
        if ks(c, "name")? == name {
            let frames = kl(c, "frames")?;
            let triggers = frames
                .iter()
                .enumerate()
                .filter(|(_, f)| f.get("triggerEvent").is_some_and(Value::truthy))
                .map(|(i, _)| i)
                .collect();
            return Ok((frames.len(), kf(c, "fps")?, triggers));
        }
    }
    err(format!("no clip {name}"))
}

/// A clip's length in ticks.
fn clip_ticks(table: &Value, name: &str) -> Result<i64> {
    let (n, fps, _) = clip(table, name)?;
    Ok(ticks(n as f64 / fps))
}

/// Ticks from the clip's start to the frame it enters.
fn frame_tick(frame: usize, fps: f64) -> i64 {
    py_round(frame as f64 * 60.0 / fps)
}

fn q16(v: f64) -> i64 {
    py_round(v * 65536.0)
}

/// 1/256 of a world unit, the precision of a shape's corners.
fn q8(v: f64) -> i64 {
    py_round(v * 256.0)
}

/// A `hk_sim::HitSpec` literal: corners in 1/256 units, ticks and damage as bytes.
fn hit_spec(corners: &Corners, damage: i64, start: i64, end: i64) -> Result<String> {
    assert_that(
        (3..=MAX_POINTS).contains(&corners.len()),
        "a shape has three to five corners",
    )?;
    let mut points = String::new();
    for i in 0..MAX_POINTS {
        let p = corners.get(i).copied().unwrap_or([0.0, 0.0]);
        if p[0].abs() > 120.0 || p[1].abs() > 120.0 {
            return err("a spell shape reaches past 120 units");
        }
        points += &format!("[{},{}],", q8(p[0]), q8(p[1]));
    }
    for (name, v) in [("damage", damage), ("start", start), ("end", end)] {
        if !(0..=255).contains(&v) {
            return err(format!("a spell shape's {name} is {v}, not a byte"));
        }
    }
    Ok(format!(
        "hk_sim::HitSpec{{points:[{points}],len:{},damage:{damage},start:{start},end:{end}}}",
        corners.len()
    ))
}

pub fn main(root: &Path, _args: Option<&[String]>) -> Result<()> {
    let source = Source::from_doctor(root).map_err(|e| e.to_string())?;
    let file = u(source.file("resources.assets"))?;
    let prefab = Prefab { source: &source, file: file.clone() };
    let (_, hero_go) = hero(&source)?;
    let knight = prefab.game_object(hero_go)?;
    let spells = prefab.child(&knight, "Spells")?;
    let library = u(source.read(&u(source.object(&file, SPELL_LIBRARY))?))?;
    let table = knight_animation(&source)?;
    let (_, constants) = hero_constants(&source)?;

    // Spell Control: the hop, its slowing, the fall.
    let (_, control) = hero_fsm(&source, "Spell Control")?;
    let r = FsmReader::new(&control)?;
    let int_of = |v: &Value| v.int().ok_or_else(|| "an int".to_string());
    let cost = int_of(&r.variables.iter().find(|e| e.0 == "MP Cost").ok_or("MP Cost")?.1)?;
    let hop = {
        let set = r.actions("Q On Ground", "SetFloatValue")?;
        r.scalar(field(set.first().ok_or("Q On Ground")?, "floatValue")?)?
            .float()
            .ok_or("hop speed")?
    };
    let keep = {
        let d = r.actions("Quake Antic", "DecelerateV2")?;
        r.scalar(field(d.first().ok_or("Quake Antic decelerates")?, "deceleration")?)?
            .float()
            .ok_or("deceleration")?
    };
    let fall = {
        let v = r.actions("Quake1 Down", "SetVelocity2d")?;
        let y = r.scalar(field(v.first().ok_or("Quake1 Down moves")?, "y")?)?.float().ok_or("fall")?;
        assert_that(y < 0.0, "the dive falls")?;
        -y
    };
    let invuln = kf(&constants, "INVUL_TIME_QUAKE")?;

    // The clips and their hit windows.
    let antic = clip_ticks(&table, "Quake Antic")?;
    let land1 = {
        let w = r.actions("Quake1 Land", "Wait")?;
        ticks(r.scalar(field(w.first().ok_or("Quake1 Land waits")?, "time")?)?.float().ok_or("wait")?)
    };
    let (land2_frames, land2_fps, land2_trigger) = clip(&table, "Quake Land 2")?;
    let land2 = ticks(land2_frames as f64 / land2_fps);
    let pillar_at = frame_tick(*land2_trigger.first().ok_or("Quake Land 2 trigger")?, land2_fps);
    let window = |clip_name: &str| -> Result<(i64, i64)> {
        let (_, fps, t) = clip(&library, clip_name)?;
        assert_that(t.len() == 2, "a clip's two triggers")?;
        Ok((frame_tick(t[0], fps), frame_tick(t[1], fps)))
    };

    let mut text = String::new();
    // Q Fall Damage: a box on the Knight while he falls.
    let fall_go = prefab.child(&spells, "Q Fall Damage")?;
    let fall_corners = prefab.corners(&fall_go, &[&spells])?;
    let fall_spec = hit_spec(&fall_corners, prefab.damage(&fall_go)?, 0, 255)?;
    // The slams, by level: Hit L then Hit R.
    let mut slams = Vec::new();
    for (object, clip_name) in [("Q Slam", "Quake Blast"), ("Q Slam 2", "Quake Blast")] {
        let (start, end) = window(clip_name)?;
        let parent = prefab.child(&spells, object)?;
        let mut pair = Vec::new();
        for side in ["Hit L", "Hit R"] {
            let hit = prefab.child(&parent, side)?;
            let corners = prefab.corners(&hit, &[&parent, &spells])?;
            pair.push(hit_spec(&corners, prefab.damage(&hit)?, start, end)?);
        }
        slams.push(format!("[{}]", pair.join(",")));
    }
    let mega = prefab.child(&spells, "Q Mega")?;
    let (mega_start, mega_end) = window("Quake 2 Mega")?;
    let mut pillar = Vec::new();
    for side in ["Hit L", "Hit R"] {
        let hit = prefab.child(&mega, side)?;
        let corners = prefab.corners(&hit, &[&mega, &spells])?;
        pillar.push(hit_spec(&corners, prefab.damage(&hit)?, mega_start, mega_end)?);
    }
    text += &format!(
        "pub static QUAKE_PARAMS: hk_sim::QuakeParams = hk_sim::QuakeParams {{antic_ticks:{antic},hop_speed:{},hop_keep:{},fall_speed:{},land_ticks:[{land1},{land2}],invuln_ticks:{},fall:{fall_spec},slam:[{}],pillar:[{}],pillar_at:{pillar_at}}};\n",
        q16(hop),
        q16(keep.powf(FIXED_HZ / 60.0)),
        q16(fall),
        ticks(invuln),
        slams.join(","),
        pillar.join(","),
    );

    // Howling Wraiths: the clips the cast is made of and the three heads of each level.
    let scream_start = clip_ticks(&table, "Scream Start")?;
    let wait_of = |state: &str| -> Result<i64> {
        let w = r.actions(state, "Wait")?;
        Ok(ticks(r.scalar(field(w.first().ok_or_else(|| format!("{state} waits"))?, "time")?)?.float().ok_or("wait")?))
    };
    let roar = wait_of("Scream Burst 1")?;
    assert_that(roar == wait_of("Scream Burst 2")?, "both levels roar as long")?;
    let end_roar = wait_of("End Roar")?;
    let end = clip_ticks(&table, "Scream End")?;
    assert_that(end == clip_ticks(&table, "Scream End 2")?, "both levels end alike")?;
    let mut levels = Vec::new();
    for (object, clip_name) in [("Scr Heads", "Scream1"), ("Scr Heads 2", "Scream2")] {
        let (start, stop) = window(clip_name)?;
        let parent = prefab.child(&spells, object)?;
        let mut heads = Vec::new();
        for side in ["Hit L", "Hit R", "Hit U"] {
            let hit = prefab.child(&parent, side)?;
            let corners = prefab.corners(&hit, &[&parent, &spells])?;
            heads.push(hit_spec(&corners, prefab.damage(&hit)?, start, stop)?);
        }
        levels.push(format!("[{}]", heads.join(",")));
    }
    text += &format!(
        "pub static SCREAM_PARAMS: hk_sim::ScreamParams = hk_sim::ScreamParams {{antic_ticks:{scream_start},roar_ticks:{roar},end_roar_ticks:{end_roar},end_ticks:{end},heads:[{}]}};\n",
        levels.join(","),
    );
    text += &format!("pub const SPELL_MP_COST: u16 = {cost};\n");

    std::fs::write(root.join("data/ability_params.rs"), &text).map_err(|e| e.to_string())?;
    dump(
        &root.join(".hkpsx/ability-params.json"),
        &jobj(vec![
            ("quake", js(&format!("antic {antic} ticks, hop {hop}, keep {keep} per physics step, fall {fall}, lands {land1}/{land2}, pillar at {pillar_at}"))),
            ("scream", js(&format!("start {scream_start}, roar {roar}, end roar {end_roar}, end {end}"))),
            ("source", js("resources.assets Spell Control, Knight/Spells, tk2d library 23263")),
        ]),
    )?;
    Ok(())
}
