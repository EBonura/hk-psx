//! Sprite extraction and projection constants from host/cook.py.

use crate::common::{err, f64_of, get, py_round, Result};
use hk_pil::Image;
use hk_unity::{texture, Obj, Source, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// cook.py FOCAL: 120 / tan(12 degrees).
pub fn focal() -> f64 {
    120.0 / 12f64.to_radians().tan()
}
pub const CAM_Z: f64 = -38.1;

/// cook.py `native_sprite`: the sprite image (RGBA) and its mesh box.
pub fn native_sprite(source: &Source, obj: &Obj) -> Result<(Image, [f64; 4])> {
    let (im, mesh) = texture::sprite_image(source, obj).map_err(|e| e.to_string())?;
    if mesh.vertices.is_empty() {
        return err("sprite has no mesh vertices");
    }
    let xs = mesh.vertices.iter().map(|v| v[0] as f64);
    let ys = mesh.vertices.iter().map(|v| v[1] as f64);
    let b = [
        crate::common::py_min(xs.clone()),
        crate::common::py_min(ys.clone()),
        crate::common::py_max(xs),
        crate::common::py_max(ys),
    ];
    Ok((im, b))
}

/// cook.py `tk_sprite`: one tk2d sprite definition cut out of its atlas texture.
/// `textures` caches decoded atlases by source id, as the Python dict does.
pub fn tk_sprite(
    source: &Source,
    file: &Arc<hk_unity::serialized::SerializedFile>,
    collection: &Value,
    index: usize,
    textures: &mut HashMap<String, Arc<Image>>,
) -> Result<(Image, [f64; 4])> {
    let d = get(collection, "spriteDefinitions")?
        .list()
        .and_then(|l| l.get(index))
        .ok_or("sprite index out of range")?;
    let mat = source
        .deref(file, get(d, "material")?)
        .map_err(|e| e.to_string())?;
    let m = source.read(&mat).map_err(|e| e.to_string())?;
    let envs = get(get(&m, "m_SavedProperties")?, "m_TexEnvs")?
        .list()
        .unwrap_or(&[]);
    // dict(m_TexEnvs): the last pair with the key wins.
    let main = envs
        .iter()
        .filter_map(|p| p.list())
        .rfind(|p| p.len() == 2 && p[0].str().as_deref() == Some("_MainTex"))
        .ok_or("material without _MainTex")?[1]
        .clone();
    let to = source
        .deref(&mat.file, get(&main, "m_Texture")?)
        .map_err(|e| e.to_string())?;
    let key = to.sid();
    if !textures.contains_key(&key) {
        let img = texture::texture_image(source, &to, true)
            .map_err(|e| e.to_string())?
            .to_rgba();
        textures.insert(key.clone(), Arc::new(img));
    }
    let tex = textures[&key].clone();
    let pos = get(d, "positions")?.list().unwrap_or(&[]);
    let uv = get(d, "uvs")?.list().unwrap_or(&[]);
    if pos.len() != 4 || get(d, "complexGeometry")?.truthy() {
        return err("non-quad tk2d sprite");
    }
    let px: Vec<(f64, f64)> = pos
        .iter()
        .map(|p| Ok((f64_of(p, "x")?, f64_of(p, "y")?)))
        .collect::<Result<_>>()?;
    let xmin = crate::common::py_min(px.iter().map(|p| p.0));
    let xmax = crate::common::py_max(px.iter().map(|p| p.0));
    let ymin = crate::common::py_min(px.iter().map(|p| p.1));
    let ymax = crate::common::py_max(px.iter().map(|p| p.1));
    // min(range(4), key=...): the first index with the smallest distance.
    let corner = |tx: f64, ty: f64| {
        (0..4)
            .min_by(|&i, &j| {
                ((px[i].0 - tx).abs() + (px[i].1 - ty).abs())
                    .partial_cmp(&((px[j].0 - tx).abs() + (px[j].1 - ty).abs()))
                    .unwrap()
            })
            .unwrap()
    };
    let (p00, p10, p01) = (corner(xmin, ymax), corner(xmax, ymax), corner(xmin, ymin));
    let (tw, th) = (tex.width as f64, tex.height as f64);
    let at = |i: usize| -> Result<(f64, f64)> {
        Ok((f64_of(&uv[i], "x")? * tw, (1.0 - f64_of(&uv[i], "y")?) * th))
    };
    let (a, b, c) = (at(p00)?, at(p10)?, at(p01)?);
    let texel = get(d, "texelSize")?;
    let w = py_round((xmax - xmin) / f64_of(texel, "x")?).max(1) as usize;
    let h = py_round((ymax - ymin) / f64_of(texel, "y")?).max(1) as usize;
    let (wf, hf) = (w as f64, h as f64);
    let im = tex.affine_bilinear(
        w,
        h,
        [
            (b.0 - a.0) / wf,
            (c.0 - a.0) / hf,
            a.0,
            (b.1 - a.1) / wf,
            (c.1 - a.1) / hf,
            a.1,
        ],
    );
    Ok((im, [xmin, ymin, xmax, ymax]))
}
