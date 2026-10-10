//! Admit original opaque-black tk2d tilemap meshes as exact merged cell unions (host/tilemap_fill.py).
//!
//! Decorative SpriteRenderers do not replace these meshes. Admission is based on an enabled
//! tk2dTileMap's renderData hierarchy, source mesh triangles, opaque sample support and the
//! source shader; object names never authorize a fill.

use crate::atlas::Atlas;
use crate::breakables::{
    descendant_set, file_of, jb, jf, jfloats, jget, ji, jl, jset, k, kf, ki, kl, ks,
};
use crate::common::{components, err, py_max, py_min, Result};
use crate::cook::{focal, CAM_Z};
use crate::cook_audio::{jobj, js, sha, u};
use crate::music::value_json;
use crate::pyjson::Json;
use hk_pil::Image;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const BLACK_PALETTE: [u8; 32] = {
    let mut p = [0u8; 32];
    p[2] = 1;
    let mut i = 2;
    while i < 16 {
        p[i * 2 + 1] = 0x80;
        i += 1;
    }
    p
};
pub const BLACK_PIXELS: [u8; 8] = [0x11; 8];

/// The parts of a Mesh the tilemap fill reads (what UnityPy's MeshHandler yields).
pub struct MeshData {
    pub vertices: Vec<[f64; 3]>,
    pub colors: Vec<[f64; 4]>,
    pub uv0: Vec<[f64; 2]>,
    pub submeshes: Vec<Vec<[usize; 3]>>,
}

/// Read a Mesh object's vertices, colours, first UV set and triangle submeshes.
pub fn read_mesh(tree: &Value) -> Result<MeshData> {
    let sm = u(hk_unity::texture::sprite_mesh(tree))?;
    let colors = u(hk_unity::texture::mesh_colors(tree))?;
    // sprite_mesh groups every submesh together; split them again by their index counts.
    let mut submeshes = Vec::new();
    let mut at = 0;
    for s in kl(tree, "m_SubMeshes")? {
        let n = ki(s, "indexCount")? as usize / 3;
        submeshes.push(sm.triangles[at..at + n].to_vec());
        at += n;
    }
    Ok(MeshData {
        vertices: sm
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        colors: colors
            .iter()
            .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64, c[3] as f64])
            .collect(),
        uv0: sm.uv0.iter().map(|v| [v[0] as f64, v[1] as f64]).collect(),
        submeshes,
    })
}

type Cell = (i64, i64);

/// `mesh_cells(vertices, triangles, colors)`: prove each pair of source triangles covers one unit cell.
pub fn mesh_cells(
    vertices: &[[f64; 3]],
    triangles: &[[usize; 3]],
    colors: &[[f64; 4]],
) -> Result<Vec<Cell>> {
    if vertices.is_empty()
        || colors.len() != vertices.len()
        || colors.iter().any(|c| *c != [1.0, 1.0, 1.0, 1.0])
    {
        return err("tilemap vertex color is not constant white");
    }
    if vertices
        .iter()
        .any(|v| v[2] != 0.0 || v[..2].iter().any(|c| !c.is_finite() || *c != c.trunc()))
    {
        return err("tilemap vertices are not planar integer coordinates");
    }
    let mut order: Vec<Cell> = Vec::new();
    let mut cells: HashMap<Cell, Vec<BTreeSet<(i64, i64)>>> = HashMap::new();
    let mut used: BTreeSet<usize> = BTreeSet::new();
    for tri in triangles {
        let distinct: BTreeSet<usize> = tri.iter().copied().collect();
        if distinct.len() != 3 || tri.iter().any(|&i| i >= vertices.len()) {
            return err("invalid tilemap triangle index");
        }
        used.extend(tri.iter().copied());
        let p: Vec<(i64, i64)> = tri
            .iter()
            .map(|&i| (vertices[i][0] as i64, vertices[i][1] as i64))
            .collect();
        let xs: Vec<i64> = p.iter().map(|v| v.0).collect();
        let ys: Vec<i64> = p.iter().map(|v| v.1).collect();
        let (x0, x1) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        let (y0, y1) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        if x1 - x0 != 1 || y1 - y0 != 1 {
            return err("tilemap triangle is not a unit-cell half");
        }
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[1].1 - p[0].1) * (p[2].0 - p[0].0);
        if area.abs() != 1 {
            return err("tilemap half-cell area");
        }
        let key = (x0, y0);
        if !cells.contains_key(&key) {
            order.push(key);
        }
        cells.entry(key).or_default().push(p.into_iter().collect());
    }
    if used != (0..vertices.len()).collect::<BTreeSet<_>>() {
        return err("unreferenced tilemap vertices");
    }
    for key in &order {
        let (x, y) = *key;
        let halves = &cells[key];
        if halves.len() != 2 {
            return err("missing or duplicate tilemap cell triangle");
        }
        let common: Vec<(i64, i64)> = halves[0].intersection(&halves[1]).copied().collect();
        let all: BTreeSet<(i64, i64)> = halves[0].union(&halves[1]).copied().collect();
        let corners: BTreeSet<(i64, i64)> = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]
            .into_iter()
            .collect();
        if common.len() != 2 || all != corners {
            return err("tilemap cell coverage");
        }
        let (a, b) = (common[0], common[1]);
        if (a.0 - b.0).abs() != 1 || (a.1 - b.1).abs() != 1 {
            return err("tilemap triangles overlap instead of sharing a diagonal");
        }
    }
    Ok(order)
}

/// `merge_cells(cells)`: a lossless disjoint rectangle cover; never fills an absent source cell.
pub fn merge_cells(cells: &[Cell]) -> Result<Vec<(i64, i64, i64, i64)>> {
    let mut left: BTreeSet<Cell> = cells.iter().copied().collect();
    let want = left.clone();
    let mut rects = Vec::new();
    while !left.is_empty() {
        let (x, y) = *left.iter().min_by_key(|p| (p.1, p.0)).unwrap();
        let mut w = 1;
        while left.contains(&(x + w, y)) {
            w += 1;
        }
        let mut h = 1;
        while (x..x + w).all(|xx| left.contains(&(xx, y + h))) {
            h += 1;
        }
        rects.push((x, y, x + w, y + h));
        for yy in y..y + h {
            for xx in x..x + w {
                left.remove(&(xx, yy));
            }
        }
    }
    let mut restored: BTreeSet<Cell> = BTreeSet::new();
    for &(x0, y0, x1, y1) in &rects {
        for y in y0..y1 {
            for x in x0..x1 {
                if !restored.insert((x, y)) {
                    return err("overlapping merged tilemap rectangles");
                }
            }
        }
    }
    if restored != want {
        return err("merged tilemap coverage differs from source");
    }
    Ok(rects)
}

/// `opaque_sample_support(image, uvs)`: conservatively include every bilinear tap in the UV box.
pub fn opaque_sample_support(image: &Image, uvs: &[[f64; 2]]) -> Result<[i64; 4]> {
    if uvs.is_empty()
        || uvs.iter().any(|uv| {
            uv.iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        })
    {
        return err("tilemap UV range");
    }
    let (w, h) = (image.width as f64, image.height as f64);
    let u0 = py_min(uvs.iter().map(|uv| uv[0]));
    let u1 = py_max(uvs.iter().map(|uv| uv[0]));
    let v0 = py_min(uvs.iter().map(|uv| uv[1]));
    let v1 = py_max(uvs.iter().map(|uv| uv[1]));
    let bounds = [
        0.max((u0 * w).floor() as i64 - 1),
        0.max((h - (v1 * h).ceil()) as i64 - 1),
        (image.width as i64).min((u1 * w).ceil() as i64 + 1),
        (image.height as i64).min((h - (v0 * h).floor()) as i64 + 1),
    ];
    let rgba = image.to_rgba();
    let crop = rgba.crop_int(bounds[0], bounds[1], bounds[2], bounds[3]);
    if crop.width == 0 || crop.height == 0 || crop.data.chunks_exact(4).any(|p| p != [0, 0, 0, 255])
    {
        return err("tilemap sample support is not fully opaque black");
    }
    Ok(bounds)
}

fn black_material(
    source: &Source,
    sc: &Scene,
    reference: &Value,
    uvs: &[[f64; 2]],
) -> Result<Json> {
    let material = u(source.deref(&sc.base, reference))?;
    let m = u(source.read(&material))?;
    let shader = u(source.deref(&material.file, k(&m, "m_Shader")?))?;
    let sh = u(source.read(&shader))?;
    let parsed = sh.get("m_ParsedForm");
    let name = parsed.and_then(|p| p.get("m_Name")).and_then(Value::str);
    if name.as_deref() != Some("tk2d/BlendVertexColor") {
        return err("unsupported tilemap shader");
    }
    let mut passes: Vec<&Value> = Vec::new();
    for sub in parsed
        .and_then(|p| p.get("m_SubShaders"))
        .and_then(Value::list)
        .unwrap_or(&[])
    {
        passes.extend(sub.get("m_Passes").and_then(Value::list).unwrap_or(&[]));
    }
    let expected: [(&str, f64); 7] = [
        ("srcBlend", 5.0),
        ("destBlend", 10.0),
        ("srcBlendAlpha", 5.0),
        ("destBlendAlpha", 10.0),
        ("blendOp", 0.0),
        ("blendOpAlpha", 0.0),
        ("colMask", 15.0),
    ];
    let mut bad = passes.is_empty();
    for p in &passes {
        let state = k(k(p, "m_State")?, "rtBlend0")?;
        for (key, want) in expected {
            if !k(state, key)
                .and_then(|s| k(s, "val"))
                .is_ok_and(|v| v.float() == Some(want))
            {
                bad = true;
            }
        }
    }
    if bad {
        return err("unsupported tilemap blend state");
    }
    let saved = k(&m, "m_SavedProperties")?;
    // dict(m_TexEnvs): the last pair with the key wins.
    let mut envs: Vec<(String, &Value)> = Vec::new();
    for pair in kl(saved, "m_TexEnvs")? {
        let Some(p) = pair.list() else { continue };
        if p.len() == 2 {
            let key = p[0].str().unwrap_or_default();
            match envs.iter_mut().find(|e| e.0 == key) {
                Some(slot) => slot.1 = &p[1],
                None => envs.push((key, &p[1])),
            }
        }
    }
    if envs.len() != 1
        || envs[0].0 != "_MainTex"
        || k(saved, "m_Colors")?.truthy()
        || k(saved, "m_Floats")?.truthy()
    {
        return err("unsupported tilemap material properties");
    }
    let env = envs[0].1;
    let unit = |a: f64, b: f64| -> Value {
        Value::Map(vec![
            ("x".into(), Value::F64(a)),
            ("y".into(), Value::F64(b)),
        ])
    };
    if !k(env, "m_Scale")?.py_eq(&unit(1.0, 1.0)) || !k(env, "m_Offset")?.py_eq(&unit(0.0, 0.0)) {
        return err("tilemap texture transform");
    }
    let tex = u(source.deref(&material.file, k(env, "m_Texture")?))?;
    let im = u(hk_unity::texture::texture_image(source, &tex, true))?.to_rgba();
    let bounds = opaque_sample_support(&im, uvs)?;
    Ok(jobj(vec![
        ("source", Json::Str(material.sid())),
        ("shader", Json::Str(shader.sid())),
        ("shader_name", js("tk2d/BlendVertexColor")),
        ("texture_source", Json::Str(tex.sid())),
        ("sample_bounds", jl(bounds.iter().map(|&b| ji(b)).collect())),
        ("sample_rgba", jl(vec![ji(0), ji(0), ji(0), ji(255)])),
        ("texture_rgba_sha256", Json::Str(sha(&im.data))),
        ("mode", ji(1)),
        ("supported", jb(true)),
        (
            "approximation",
            js("Exact constant opaque black; PS1 sentinel palette index1 with renderer red127 correction."),
        ),
    ]))
}

/// `tilemap_fill_sources(sc)`: scene-wide extraction; unsupported active meshes fail.
pub fn tilemap_fill_sources(sc: &Scene) -> Result<Vec<Json>> {
    let source = sc.source;
    let file = file_of(sc);
    let mut roots: HashMap<i64, String> = HashMap::new();
    for o in &sc.objects {
        if o.typename != "tk2dTileMap" || !k(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let owner = ki(k(&o.tree, "m_GameObject")?, "m_PathID")?;
        if !sc.active(owner) {
            continue;
        }
        let obj = u(source.deref(&sc.base, k(&o.tree, "renderData")?))?;
        if obj.class_id() != 1 {
            return err("tk2d renderData is not a scene GameObject");
        }
        for gid in descendant_set(sc, obj.path_id())? {
            roots
                .entry(gid)
                .or_insert_with(|| format!("{file}:{}", o.id));
        }
    }
    let mut meshes = Vec::new();
    for o in &sc.objects {
        let t = &o.tree;
        if o.typename != "MeshRenderer" || !k(t, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = ki(k(t, "m_GameObject")?, "m_PathID")?;
        if !roots.contains_key(&gid) || !sc.active(gid) {
            continue;
        }
        let filters: Vec<&Value> = components(sc, gid)?
            .into_iter()
            .filter(|c| c.1 == "MeshFilter")
            .map(|c| c.2)
            .collect();
        let materials = kl(t, "m_Materials")?;
        if filters.len() != 1 || materials.len() != 1 {
            return err("tilemap mesh/material count");
        }
        let obj = u(source.deref(&sc.base, k(filters[0], "m_Mesh")?))?;
        let mesh = read_mesh(&u(source.read(&obj))?)?;
        if mesh.submeshes.len() != 1 {
            return err("tilemap mesh submesh count");
        }
        let cells = mesh_cells(&mesh.vertices, &mesh.submeshes[0], &mesh.colors)?;
        if mesh.uv0.len() != mesh.vertices.len() {
            return err("tilemap UV count");
        }
        let material = black_material(source, sc, &materials[0], &mesh.uv0)?;
        let rects = merge_cells(&cells)?;
        let mut points: Vec<Vec<[f64; 3]>> = Vec::new();
        for &(x0, y0, x1, y1) in &rects {
            let mut ps = Vec::new();
            for (x, y) in [(x0, y1), (x1, y1), (x0, y0), (x1, y0)] {
                ps.push(u(sc.point(gid, x as f64, y as f64, 0.0))?);
            }
            points.push(ps);
        }
        if points.iter().any(|ps| ps.iter().any(|p| p[2] != ps[0][2])) {
            return err("nonplanar transformed tilemap fill");
        }
        let mut sorted = cells.clone();
        sorted.sort_unstable();
        meshes.push(jobj(vec![
            ("source", Json::Str(format!("{file}:{}", o.id))),
            ("tilemap_source", Json::Str(roots[&gid].clone())),
            ("mesh_source", Json::Str(obj.sid())),
            ("mesh_sha256", Json::Str(sha(u(obj.raw())?))),
            ("name", value_json(k(sc.go(gid).ok_or("'gid'")?, "m_Name")?)),
            ("material", material),
            (
                "cells",
                jl(sorted.iter().map(|c| jl(vec![ji(c.0), ji(c.1)])).collect()),
            ),
            (
                "rectangles",
                jl(rects
                    .iter()
                    .map(|r| jl(vec![ji(r.0), ji(r.1), ji(r.2), ji(r.3)]))
                    .collect()),
            ),
            (
                "points",
                jl(points
                    .iter()
                    .map(|ps| jl(ps.iter().map(|p| jfloats(p)).collect()))
                    .collect()),
            ),
            ("layer", value_json(k(t, "m_SortingLayer")?)),
            ("order", value_json(k(t, "m_SortingOrder")?)),
            ("source_triangle_count", ji(mesh.submeshes[0].len() as i64)),
            ("cell_count", ji(cells.len() as i64)),
            ("rectangle_count", ji(rects.len() as i64)),
        ]));
    }
    Ok(meshes)
}

fn jn(j: &Json, key: &str) -> i64 {
    match jget(j, key) {
        Some(Json::Int(i)) => *i,
        _ => 0,
    }
}

/// `append_tilemap_fills(sc, atlas, draws, region, cam_x, cam_y, focal, cam_z)`: the region's
/// tilemap fills as scenery draws. `cache` is the scene's extraction (`sc._tilemap_fills`).
pub fn append_tilemap_fills(
    sc: &Scene,
    cache: &mut Option<Vec<Json>>,
    atlas: &mut Atlas,
    draws: &mut Vec<Json>,
    cam_x: (f64, f64),
    cam_y: (f64, f64),
) -> Result<Json> {
    if cache.is_none() {
        *cache = Some(tilemap_fill_sources(sc)?);
    }
    let meshes = cache.as_ref().unwrap();
    let focal = focal();
    let mut texture: Option<usize> = None;
    let mut admitted: Vec<Json> = Vec::new();
    for mesh in meshes {
        let Some(Json::List(all_points)) = jget(mesh, "points") else {
            continue;
        };
        for (index, points) in all_points.iter().enumerate() {
            let Json::List(points) = points else { continue };
            let coords: Vec<[f64; 3]> = points
                .iter()
                .map(|p| {
                    let Json::List(c) = p else {
                        return [f64::NAN; 3];
                    };
                    let f = |j: &Json| match j {
                        Json::Float(f) => *f,
                        Json::Int(i) => *i as f64,
                        _ => f64::NAN,
                    };
                    [f(&c[0]), f(&c[1]), f(&c[2])]
                })
                .collect();
            let z = coords[0][2];
            if z <= CAM_Z + 2.0 {
                return err("tilemap fill crosses camera near plane");
            }
            let scale = focal / (z - CAM_Z);
            let xs = coords.iter().map(|p| p[0]);
            let ys = coords.iter().map(|p| p[1]);
            if py_max(xs.clone()) < cam_x.0 - 160.0 / scale
                || py_min(xs) > cam_x.1 + 160.0 / scale
                || py_max(ys.clone()) < cam_y.0 - 120.0 / scale
                || py_min(ys) > cam_y.1 + 120.0 / scale
            {
                continue;
            }
            let tex = match texture {
                Some(t) => t,
                None => {
                    let t =
                        atlas.add_quantized(4, 4, &BLACK_PALETTE, &BLACK_PIXELS, false, false)?;
                    texture = Some(t);
                    t
                }
            };
            let material = jget(mesh, "material").cloned().unwrap_or(Json::Null);
            draws.push(jobj(vec![
                (
                    "source",
                    jget(mesh, "source").cloned().unwrap_or(Json::Null),
                ),
                (
                    "sprite",
                    jget(&material, "texture_source")
                        .cloned()
                        .unwrap_or(Json::Null),
                ),
                ("name", jget(mesh, "name").cloned().unwrap_or(Json::Null)),
                ("texture", ji(tex as i64)),
                ("points", Json::List(points.clone())),
                ("scale", jf(scale)),
                ("tint", jl(vec![ji(128), ji(128), ji(128)])),
                ("z", jf(z)),
                ("layer", jget(mesh, "layer").cloned().unwrap_or(Json::Null)),
                ("order", jget(mesh, "order").cloned().unwrap_or(Json::Null)),
                ("material", material),
                ("tilemap_rect", ji(index as i64)),
                (
                    "tilemap_mesh",
                    jget(mesh, "mesh_source").cloned().unwrap_or(Json::Null),
                ),
            ]));
            admitted.push(jl(vec![
                jget(mesh, "source").cloned().unwrap_or(Json::Null),
                ji(index as i64),
            ]));
        }
    }
    let sum = |key: &str| meshes.iter().map(|m| jn(m, key)).sum::<i64>();
    let mut out = jobj(vec![
        ("mesh_count", ji(meshes.len() as i64)),
        ("source_cells", ji(sum("cell_count"))),
        ("source_triangles", ji(sum("source_triangle_count"))),
        ("merged_rectangles", ji(sum("rectangle_count"))),
        ("region_draws", ji(admitted.len() as i64)),
        ("admitted", jl(admitted)),
    ]);
    jset(&mut out, "meshes", jl(meshes.clone()));
    Ok(out)
}

#[allow(dead_code)]
fn _keep() {
    let _: Option<BTreeMap<i32, i32>> = None;
    let _ = (kf, ks, ji);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(x: f64, y: f64) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
        (
            vec![
                [x, y, 0.0],
                [x + 1.0, y, 0.0],
                [x, y + 1.0, 0.0],
                [x + 1.0, y + 1.0, 0.0],
            ],
            vec![[0, 1, 2], [1, 3, 2]],
        )
    }

    #[test]
    fn two_triangles_sharing_a_diagonal_are_one_cell() {
        let (v, t) = quad(3.0, 4.0);
        let c = vec![[1.0; 4]; 4];
        assert_eq!(mesh_cells(&v, &t, &c).unwrap(), vec![(3, 4)]);
    }

    #[test]
    fn a_vertex_that_is_not_white_refuses_the_mesh() {
        let (v, t) = quad(0.0, 0.0);
        let mut c = vec![[1.0; 4]; 4];
        c[2][0] = 0.5;
        assert!(mesh_cells(&v, &t, &c).is_err());
    }

    #[test]
    fn merging_keeps_exactly_the_source_cells() {
        let cells: Vec<Cell> = vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (5, 5)];
        let rects = merge_cells(&cells).unwrap();
        assert_eq!(rects, vec![(0, 0, 3, 1), (0, 1, 2, 2), (5, 5, 6, 6)]);
    }
}
