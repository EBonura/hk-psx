//! An actor's own colliders in world space. Ported from host/actors.py `_colliders`.
//!
//! Boxes and polygons become world polygons plus their bounding box; a circle is
//! recorded as unsupported. A polygon of more than 16 vertices would be split into
//! bounded pieces by host/polygons.py, which is not ported: `bounded_polygons`
//! refuses it loudly rather than guess the piece order.

use crate::common::{err, get, Result};
use crate::cook_audio::u;
use crate::pyjson::Json;
use hk_unity::scene::Scene;
use hk_unity::Value;

/// `POLYGON_VERTEX_LIMIT`.
const POLYGON_VERTEX_LIMIT: usize = 16;

fn jf(v: f64) -> Json {
    Json::Float(v)
}

fn jobj(fields: Vec<(&str, Json)>) -> Json {
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// `bounded_polygons`: pieces of at most 16 vertices whose union is the polygon.
fn bounded_polygons(points: Vec<(f64, f64)>) -> Result<Vec<Vec<(f64, f64)>>> {
    if points.len() <= POLYGON_VERTEX_LIMIT {
        return Ok(vec![points]);
    }
    err("polygon splitting (host/polygons.py bounded_polygons) is not ported")
}

fn pair(p: (f64, f64)) -> Json {
    Json::List(vec![jf(p.0), jf(p.1)])
}

/// `_colliders(sc, gid, records)`.
pub fn colliders(sc: &Scene, gid: i64, records: &[(i64, &str, &Value)]) -> Result<Json> {
    let mut result = Vec::new();
    for &(sid, typ, tree) in records {
        if !matches!(typ, "BoxCollider2D" | "PolygonCollider2D" | "CircleCollider2D") || !tree.get("m_Enabled").is_some_and(Value::truthy) {
            continue;
        }
        let offset = get(tree, "m_Offset")?;
        let (ox, oy) = (get(offset, "x")?.float().ok_or("m_Offset")?, get(offset, "y")?.float().ok_or("m_Offset")?);
        let paths: Vec<Vec<(f64, f64)>> = match typ {
            "BoxCollider2D" => {
                let size = get(tree, "m_Size")?;
                let (hx, hy) = (get(size, "x")?.float().ok_or("m_Size")? / 2.0, get(size, "y")?.float().ok_or("m_Size")? / 2.0);
                vec![vec![(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]]
            }
            "PolygonCollider2D" => {
                let mut out = Vec::new();
                for path in get(get(tree, "m_Points")?, "m_Paths")?.list().unwrap_or(&[]) {
                    let mut pts = Vec::new();
                    for p in path.list().unwrap_or(&[]) {
                        pts.push((get(p, "x")?.float().ok_or("point")?, get(p, "y")?.float().ok_or("point")?));
                    }
                    out.push(pts);
                }
                out
            }
            _ => {
                result.push(jobj(vec![("source", Json::Str(sc.sid(sid))), ("type", Json::Str(typ.to_string())), ("unsupported", Json::Str("exact circle contact not yet cooked".into()))]));
                continue;
            }
        };
        let mut polygons: Vec<Vec<(f64, f64)>> = Vec::new();
        for path in &paths {
            let mut world = Vec::new();
            for &(x, y) in path {
                let p = u(sc.point(gid, x + ox, y + oy, 0.0))?;
                world.push((p[0], p[1]));
            }
            polygons.push(world);
        }
        let mut pieces = Vec::new();
        for polygon in polygons {
            pieces.extend(bounded_polygons(polygon)?);
        }
        let points: Vec<(f64, f64)> = pieces.iter().flatten().copied().collect();
        if points.is_empty() || points.iter().any(|p| !p.0.is_finite() || !p.1.is_finite() || p.0.abs() > 512.0 || p.1.abs() > 512.0) {
            return err("actor collider exceeds Q16 world coordinate bounds");
        }
        let min = |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::INFINITY, |a, b| if b < a { b } else { a });
        let max = |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::NEG_INFINITY, |a, b| if b > a { b } else { a });
        result.push(jobj(vec![
            ("source", Json::Str(sc.sid(sid))),
            ("type", Json::Str(typ.to_string())),
            ("trigger", Json::Bool(get(tree, "m_IsTrigger")?.truthy())),
            ("world_polygons", Json::List(pieces.iter().map(|poly| Json::List(poly.iter().map(|&p| pair(p)).collect())).collect())),
            ("bounds", Json::List(vec![jf(min(|p| p.0)), jf(min(|p| p.1)), jf(max(|p| p.0)), jf(max(|p| p.1))])),
        ]));
    }
    Ok(Json::List(result))
}
