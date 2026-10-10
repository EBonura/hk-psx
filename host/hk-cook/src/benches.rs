//! RestBench placements: trigger box, seat position and the Knight's sit clips (host/benches.py).

use crate::breakables::{file_of, jfloats, jl, k, kf, ki, point};
use crate::common::Result;
use crate::cook_audio::{jobj, js};
use crate::music::value_json;
use crate::pyjson::Json;
use hk_unity::scene::Scene;
use hk_unity::Value;

pub const BENCH_CLIPS: [&str; 3] = ["Sit", "Sit Idle", "Get Off"];

/// `bench_sources(sc, bounds)`: the active RestBenches whose trigger box meets `bounds`.
pub fn bench_sources(sc: &Scene, bounds: [f64; 4]) -> Result<Vec<Json>> {
    let mut result = Vec::new();
    for o in &sc.objects {
        if o.typename != "RestBench" {
            continue;
        }
        let gid = ki(k(&o.tree, "m_GameObject")?, "m_PathID")?;
        if !sc.active(gid) {
            continue;
        }
        let position = point(sc, gid)?;
        let mut trigger: Option<[f64; 4]> = None;
        for c in &sc.objects {
            if c.typename != "BoxCollider2D"
                || ki(k(&c.tree, "m_GameObject")?, "m_PathID")? != gid
                || !k(&c.tree, "m_IsTrigger")?.truthy()
            {
                continue;
            }
            let off = k(&c.tree, "m_Offset")?;
            let size = k(&c.tree, "m_Size")?;
            let mut pts = Vec::new();
            for (x, y) in [
                (-kf(size, "x")? / 2.0, -kf(size, "y")? / 2.0),
                (kf(size, "x")? / 2.0, kf(size, "y")? / 2.0),
            ] {
                let p = sc
                    .point(gid, x + kf(off, "x")?, y + kf(off, "y")?, 0.0)
                    .map_err(|e| e.to_string())?;
                pts.push([p[0], p[1]]);
            }
            trigger = Some([
                pts[0][0].min(pts[1][0]),
                pts[0][1].min(pts[1][1]),
                pts[0][0].max(pts[1][0]),
                pts[0][1].max(pts[1][1]),
            ]);
        }
        let Some(trigger) = trigger else { continue };
        if !(trigger[0] <= bounds[2]
            && trigger[2] >= bounds[0]
            && trigger[1] <= bounds[3]
            && trigger[3] >= bounds[1])
        {
            continue;
        }
        let mut has_control = false;
        for f in &sc.objects {
            if f.typename == "PlayMakerFSM"
                && ki(k(&f.tree, "m_GameObject")?, "m_PathID")? == gid
                && k(k(&f.tree, "fsm")?, "name")?.str().as_deref() == Some("Bench Control")
            {
                has_control = true;
            }
        }
        if !has_control {
            continue;
        }
        let name: &Value = k(sc.go(gid).ok_or("'gid'")?, "m_Name")?;
        result.push(jobj(vec![
            ("source", Json::Str(format!("{}:{}", file_of(sc), o.id))),
            ("name", value_json(name)),
            ("position", jfloats(&position[..2])),
            ("bounds", jfloats(&trigger)),
            (
                "limitations",
                jl(vec![js("Sit/Sit Idle/Get Off only: no map, charm prompt, sleep or bench tilt; respawn stands at the seat instead of waking on it")]),
            ),
        ]));
    }
    Ok(result)
}
