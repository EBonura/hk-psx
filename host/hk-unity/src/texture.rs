//! Texture2D pixels and native Sprite images, as UnityPy 1.25.3 produces
//! them (Texture2DConverter, SpriteHelper and MeshHelper).

use crate::value::Value;
use crate::{Error, Obj, Result, Source};
use hk_pil::{bcn, Image, Mode};

/// Raw texture bytes: inline `image data`, else the `.resS` stream.
fn image_data(source: &Source, obj: &Obj, tex: &Value) -> Result<Vec<u8>> {
    if let Some(Value::Bytes(b)) = tex.get("image data") {
        if !b.is_empty() {
            return Ok(b.clone());
        }
    }
    let stream = tex
        .get("m_StreamData")
        .ok_or_else(|| Error::Format("Texture2D has no image data".into()))?;
    let path = stream.get("path").and_then(Value::str).unwrap_or_default();
    if path.is_empty() {
        return Err(Error::Format("Texture2D has no image data".into()));
    }
    let offset = stream.get("offset").and_then(Value::int).unwrap_or(0) as usize;
    let size = stream.get("size").and_then(Value::int).unwrap_or(0) as usize;
    let base = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
    let _ = obj;
    let file = source.directory.join(&base);
    let bytes = source.resource(&file)?;
    bytes
        .get(offset..offset + size)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::Format(format!("{base}: stream out of range")))
}

/// `Texture2D.image` (flip=true) or the unflipped image SpriteHelper reads.
pub fn texture_image(source: &Source, obj: &Obj, flip: bool) -> Result<Image> {
    let tex = source.read(obj)?;
    let w = tex.get("m_Width").and_then(Value::int).unwrap_or(0) as usize;
    let h = tex.get("m_Height").and_then(Value::int).unwrap_or(0) as usize;
    let format = tex
        .get("m_TextureFormat")
        .and_then(Value::int)
        .unwrap_or(-1);
    // UnityPy fetches the data before it looks at the size, so an empty texture
    // without data fails rather than coming back empty.
    let data = image_data(source, obj, &tex)?;
    if w == 0 || h == 0 {
        return Ok(Image::new(Mode::Rgba, 0, 0));
    }
    if data.is_empty() {
        return Err(Error::Format("Texture2D has no image data".into()));
    }
    let short = || Error::Format("not enough image data".into());
    let img = match format {
        // Alpha8: ("RGBA", "raw", "A"); only the alpha band is written.
        1 => {
            let mut img = Image::new(Mode::Rgba, w, h);
            let px = data.get(..w * h).ok_or_else(short)?;
            for (i, a) in px.iter().enumerate() {
                img.data[i * 4 + 3] = *a;
            }
            img
        }
        // ARGB4444: ("RGBA", "raw", "RGBA;4B") then bands merged as (2, 1, 0, 3).
        2 => {
            let mut img = Image::new(Mode::Rgba, w, h);
            let px = data.get(..w * h * 2).ok_or_else(short)?;
            for (i, c) in px.chunks_exact(2).enumerate() {
                let p = c[0] as u32 | (c[1] as u32) << 8;
                let (r, g, b, a) = (
                    (p & 15) * 17,
                    ((p >> 4) & 15) * 17,
                    ((p >> 8) & 15) * 17,
                    ((p >> 12) & 15) * 17,
                );
                img.data[i * 4..i * 4 + 4].copy_from_slice(&[b as u8, g as u8, r as u8, a as u8]);
            }
            img
        }
        // RGB24: ("RGB", "raw", "RGB").
        3 => {
            let mut img = Image::new(Mode::Rgb, w, h);
            let px = data.get(..w * h * 3).ok_or_else(short)?;
            for (i, c) in px.chunks_exact(3).enumerate() {
                img.data[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
            img
        }
        4 => Image {
            mode: Mode::Rgba,
            width: w,
            height: h,
            data: data.get(..w * h * 4).ok_or_else(short)?.to_vec(),
        },
        10 | 12 | 25 => {
            let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
            let n = match format {
                10 => 1,
                12 => 3,
                _ => 7,
            };
            let decoded = bcn::decode(n, &data, pw, ph).ok_or_else(short)?;
            let full = Image {
                mode: Mode::Rgba,
                width: pw,
                height: ph,
                data: decoded,
            };
            if (pw, ph) != (w, h) {
                full.crop_int(0, 0, w as i64, h as i64)
            } else {
                full
            }
        }
        other => {
            return Err(Error::Format(format!(
                "Not implemented texture format: {other}"
            )))
        }
    };
    Ok(if flip { img.flip_top_bottom() } else { img })
}

/// Sprite render data vertices (x, y, z) and triangles, as MeshHandler reads them.
pub struct SpriteMesh {
    pub vertices: Vec<[f32; 3]>,
    pub uv0: Vec<[f32; 2]>,
    pub triangles: Vec<[usize; 3]>,
}

fn format_size(format: i64) -> Result<(usize, char)> {
    Ok(match format {
        0 => (4, 'f'),
        1 => (2, 'e'),
        2 | 6 => (1, 'B'),
        3 | 7 => (1, 'b'),
        4 | 8 => (2, 'H'),
        5 | 9 => (2, 'h'),
        10 => (4, 'I'),
        11 => (4, 'i'),
        _ => return Err(Error::Format(format!("unknown vertex format {format}"))),
    })
}

pub fn sprite_mesh(rd: &Value) -> Result<SpriteMesh> {
    let vd = rd
        .get("m_VertexData")
        .ok_or_else(|| Error::Format("sprite without vertex data".into()))?;
    let count = vd.get("m_VertexCount").and_then(Value::int).unwrap_or(0) as usize;
    let channels: Vec<(i64, i64, i64, i64)> = vd
        .get("m_Channels")
        .and_then(Value::list)
        .unwrap_or(&[])
        .iter()
        .map(|c| {
            let g = |k: &str| c.get(k).and_then(Value::int).unwrap_or(0);
            (g("stream"), g("offset"), g("format"), g("dimension"))
        })
        .collect();
    let data: Vec<u8> = match vd.get("m_DataSize") {
        Some(Value::Bytes(b)) => b.clone(),
        Some(Value::List(l)) => l.iter().map(|v| v.int().unwrap_or(0) as u8).collect(),
        _ => Vec::new(),
    };
    // Streams: per stream index, offset and stride; offsets 16-byte aligned.
    let streams = 1 + channels.iter().map(|c| c.0).max().unwrap_or(0);
    let mut stream_info = Vec::new();
    let mut offset = 0usize;
    for s in 0..streams {
        let mut stride = 0;
        for c in &channels {
            if c.0 == s && c.3 > 0 {
                stride += (c.3 & 0xf) as usize * format_size(c.2)?.0;
            }
        }
        stream_info.push((offset, stride));
        offset += count * stride;
        offset = (offset + 15) & !15;
    }
    let read = |chn: usize| -> Result<Vec<Vec<f32>>> {
        let (stream, off, format, dim) = channels[chn];
        let (size, kind) = format_size(format)?;
        let dim = (dim & 0xf) as usize;
        let (soff, stride) = stream_info[stream as usize];
        let mut out = Vec::with_capacity(count);
        for v in 0..count {
            let mut comp = Vec::with_capacity(dim);
            for d in 0..dim {
                let at = soff + off as usize + stride * v + d * size;
                let b = data.get(at..at + size).ok_or(Error::Eof)?;
                comp.push(match kind {
                    'f' => f32::from_le_bytes(b.try_into().unwrap()),
                    _ => return Err(Error::Format("non-float sprite channel".into())),
                });
            }
            out.push(comp);
        }
        Ok(out)
    };
    let mut vertices = Vec::new();
    let mut uv0 = Vec::new();
    if count > 0 && !data.is_empty() {
        if channels.first().is_some_and(|c| c.3 > 0) {
            vertices = read(0)?
                .into_iter()
                .map(|v| [v[0], *v.get(1).unwrap_or(&0.0), *v.get(2).unwrap_or(&0.0)])
                .collect();
        }
        if channels.get(4).is_some_and(|c| c.3 > 0) {
            uv0 = read(4)?
                .into_iter()
                .map(|v| [v[0], *v.get(1).unwrap_or(&0.0)])
                .collect();
        }
    }
    let raw: Vec<u8> = match rd.get("m_IndexBuffer") {
        Some(Value::Bytes(b)) => b.clone(),
        Some(Value::List(l)) => l.iter().map(|v| v.int().unwrap_or(0) as u8).collect(),
        _ => Vec::new(),
    };
    let indices: Vec<usize> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]) as usize)
        .collect();
    let mut triangles = Vec::new();
    for sm in rd.get("m_SubMeshes").and_then(Value::list).unwrap_or(&[]) {
        let first = sm.get("firstByte").and_then(Value::int).unwrap_or(0) as usize / 2;
        let n = sm.get("indexCount").and_then(Value::int).unwrap_or(0) as usize;
        if sm.get("topology").and_then(Value::int).unwrap_or(0) != 0 {
            return Err(Error::Format(
                "sprite submesh is not a triangle list".into(),
            ));
        }
        let mut i = first;
        while i < first + n {
            let t = indices.get(i..i + 3).ok_or(Error::Eof)?;
            triangles.push([t[0], t[1], t[2]]);
            i += 3;
        }
    }
    Ok(SpriteMesh {
        vertices,
        uv0,
        triangles,
    })
}

/// `Sprite.image` for an unpacked tight sprite without UVs (every sprite of
/// the install): the texture rect, masked by the mesh triangles drawn with
/// ImageDraw, flipped to top-down. Converted to RGBA as cook.py does.
pub fn sprite_image(source: &Source, sprite_obj: &Obj) -> Result<(Image, SpriteMesh)> {
    let sp = source.read(sprite_obj)?;
    let rd = sp
        .get("m_RD")
        .ok_or_else(|| Error::Format("sprite without render data".into()))?;
    let atlas_data = atlas_render_data(source, sprite_obj, &sp)?;
    let data = atlas_data.as_ref().unwrap_or(rd);
    let settings = data.get("settingsRaw").and_then(Value::int).unwrap_or(0);
    let tex = source.deref(
        &sprite_obj.file,
        data.get("texture")
            .ok_or_else(|| Error::Format("sprite without texture".into()))?,
    )?;
    if data
        .get("alphaTexture")
        .and_then(Value::pptr)
        .is_some_and(|p| p.1 != 0)
    {
        return Err(Error::Format(
            "sprites with an alpha texture are not ported".into(),
        ));
    }
    let original = source.texture_cached(&sprite_obj.file.name, &tex)?;
    let r = data
        .get("textureRect")
        .ok_or_else(|| Error::Format("sprite without textureRect".into()))?;
    let f = |k: &str| r.get(k).and_then(Value::float).unwrap_or(0.0);
    let (x, y, w, h) = (f("x"), f("y"), f("width"), f("height"));
    let mut image = original.crop([x, y, x + w, y + h]);
    if settings & 1 != 0 {
        image = match (settings >> 2) & 0xf {
            0 => image,
            1 => image.flip_left_right(),
            2 => image.flip_top_bottom(),
            3 => image.flip_left_right().flip_top_bottom(),
            r => {
                return Err(Error::Format(format!(
                    "sprite packing rotation {r} is not ported"
                )))
            }
        };
    }
    let mesh = sprite_mesh(rd)?;
    let tight = (settings >> 1) & 1 == 0;
    if tight {
        if mesh.uv0.iter().any(|uv| uv[0] != 0.0 || uv[1] != 0.0) {
            return Err(Error::Format(
                "sprites with mesh UVs (render_sprite_mesh) are not ported".into(),
            ));
        }
        if mesh.vertices.is_empty() {
            return Err(Error::Format("No vertices found in sprite mesh!".into()));
        }
        let min_x = mesh
            .vertices
            .iter()
            .map(|v| v[0] as f64)
            .fold(f64::INFINITY, f64::min);
        let min_y = mesh
            .vertices
            .iter()
            .map(|v| v[1] as f64)
            .fold(f64::INFINITY, f64::min);
        let factor = sp
            .get("m_PixelsToUnits")
            .and_then(Value::float)
            .unwrap_or(0.0);
        let pts: Vec<(f64, f64)> = mesh
            .vertices
            .iter()
            .map(|v| {
                (
                    (v[0] as f64 - min_x) * factor,
                    (v[1] as f64 - min_y) * factor,
                )
            })
            .collect();
        let mut mask = Image::new(Mode::One, image.width, image.height);
        for t in &mesh.triangles {
            hk_pil::draw::polygon_fill(&mut mask, &[pts[t[0]], pts[t[1]], pts[t[2]]], 1);
        }
        image = match image.mode {
            Mode::Rgba => {
                let mut out = Image::new(Mode::Rgba, image.width, image.height);
                out.paste(&image, 0, 0, Some(&mask));
                out
            }
            // putalpha: RGB becomes RGBA in place (its pad byte), the "1" mask
            // becomes the alpha band as 0 or 255, and the colour stays.
            Mode::Rgb => {
                let mut out = image.clone();
                out.mode = Mode::Rgba;
                for (i, px) in out.data.chunks_exact_mut(4).enumerate() {
                    px[3] = if mask.data[i] != 0 { 255 } else { 0 };
                }
                out
            }
            _ => {
                return Err(Error::Format(
                    "tight sprite from this texture mode is not ported".into(),
                ))
            }
        };
    }
    Ok((image.flip_top_bottom().to_rgba(), mesh))
}

/// The SpriteAtlasData a packed sprite draws from, found through its atlas
/// pointer or, failing that, the first atlas tag (SpriteHelper.get_image_from_sprite).
fn atlas_render_data(source: &Source, sprite_obj: &Obj, sp: &Value) -> Result<Option<Value>> {
    let atlas = if sp
        .get("m_SpriteAtlas")
        .and_then(Value::pptr)
        .is_some_and(|p| p.1 != 0)
    {
        let obj = source.deref(&sprite_obj.file, sp.get("m_SpriteAtlas").unwrap())?;
        Some(source.read(&obj)?)
    } else if let Some(tag) = sp
        .get("m_AtlasTags")
        .and_then(Value::list)
        .and_then(|l| l.first())
    {
        let mut found = None;
        for info in sprite_obj
            .file
            .objects
            .iter()
            .filter(|i| i.class_id == 687078895)
        {
            let atlas = source.read(&Obj {
                file: sprite_obj.file.clone(),
                info: *info,
            })?;
            if atlas.get("m_Name").is_some_and(|n| n.py_eq(tag)) {
                found = Some(atlas);
                break;
            }
        }
        found
    } else {
        None
    };
    let Some(atlas) = atlas else { return Ok(None) };
    let key = sp
        .get("m_RenderDataKey")
        .ok_or_else(|| Error::Format("sprite without m_RenderDataKey".into()))?;
    for pair in atlas
        .get("m_RenderDataMap")
        .and_then(Value::list)
        .unwrap_or(&[])
    {
        if let Value::List(kv) = pair {
            if kv.len() == 2 && kv[0].py_eq(key) {
                return Ok(Some(kv[1].clone()));
            }
        }
    }
    Err(Error::Format("SpriteAtlasData not found!".into()))
}
