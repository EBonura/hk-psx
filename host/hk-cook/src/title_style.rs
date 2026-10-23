//! The title screen's menu styles, composed from the original's own scene.
//!
//! `Menu_Title` (level1) holds a `Menu_Styles` object with one child per style (`MenuStyles.styles`
//! names them in the order the options list them). A style is a stack of sprites in world space,
//! seen through the game camera (`_GameCameras/CameraParent/tk2dCamera`: perspective, 24 degrees
//! vertical, at the title's fixed pose), then the camera's colour correction. The glows and light
//! shafts use the `UIBlendScreen` material, the pictures and vignettes the default sprite material.
//!
//! This composes the static layers (sprites, no particles) of a style to 320x240 so the PS1 title
//! can use it as its background. The sprites are drawn far to near with their own colour and alpha;
//! screen blending is `1 - (1 - dst)(1 - src)` with `src` the sprite's colour times its coverage.
//! The camera's bloom is not reproduced.
//!
//! Writes `.hkpsx/title-styles/<style>.png` (the composition) for every style asked for.
use crate::common::{err, Result};
use crate::cook::native_sprite;
use crate::png::save_rgba;
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::HashMap;
use std::path::Path;

/// The screen the PS1 title composes for.
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
const CAMERA_PATH: &str = "_GameCameras/CameraParent/tk2dCamera";

/// A 4x4 matrix, row-major, acting on column vectors.
type Mat = [[f64; 4]; 4];

fn identity() -> Mat {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m
}

fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut m = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn trs(position: [f64; 3], q: [f64; 4], scale: [f64; 3]) -> Mat {
    let [x, y, z, w] = q;
    let n = (x * x + y * y + z * z + w * w).sqrt().max(1e-12);
    let (x, y, z, w) = (x / n, y / n, z / n, w / n);
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = identity();
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = r[i][j] * scale[j];
        }
        m[i][3] = position[i];
    }
    m
}

fn apply(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let mut o = [0.0; 3];
    for i in 0..3 {
        o[i] = m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3];
    }
    o
}

fn vec3(v: Option<&Value>) -> [f64; 3] {
    let f = |k: &str| v.and_then(|v| v.get(k)).and_then(Value::float).unwrap_or(0.0);
    [f("x"), f("y"), f("z")]
}

fn quat(v: Option<&Value>) -> [f64; 4] {
    let f = |k: &str, d: f64| v.and_then(|v| v.get(k)).and_then(Value::float).unwrap_or(d);
    [f("x", 0.0), f("y", 0.0), f("z", 0.0), f("w", 1.0)]
}

fn pptr(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(|f| f.get("m_PathID")).and_then(Value::int).unwrap_or(0)
}

/// One sprite of a style, ready to draw.
pub struct Layer {
    pub name: String,
    pub sprite: String,
    pub screen_blend: bool,
    pub colour: [f64; 4],
    /// Screen position of the four corners (top-left, top-right, bottom-left, bottom-right).
    pub corners: [[f64; 2]; 4],
    pub depth: f64,
}

/// What the scene says about the camera's grade.
pub struct Grade {
    pub saturation: f64,
}

/// The names of the styles under `Menu_Styles`.
pub fn style_names(scene: &Scene) -> Vec<String> {
    let paths = paths(scene);
    let mut out = Vec::new();
    for o in &scene.objects {
        if o.typename != "GameObject" {
            continue;
        }
        if let Some(p) = paths.get(&o.id) {
            if let Some(rest) = p.strip_prefix("Menu_Styles/") {
                if !rest.contains('/') {
                    out.push(rest.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

fn paths(scene: &Scene) -> HashMap<i64, String> {
    let mut go_tr: HashMap<i64, i64> = HashMap::new();
    for o in &scene.objects {
        if o.typename == "Transform" || o.typename == "RectTransform" {
            go_tr.insert(pptr(&o.tree, "m_GameObject"), o.id);
        }
    }
    let mut out = HashMap::new();
    for o in &scene.objects {
        if o.typename != "GameObject" {
            continue;
        }
        let mut names = Vec::new();
        let mut t = go_tr.get(&o.id).copied().unwrap_or(0);
        let mut guard = 0;
        while t != 0 && guard < 64 {
            guard += 1;
            let Some(tr) = scene.object(t) else { break };
            let g = pptr(&tr.tree, "m_GameObject");
            names.push(
                scene
                    .object(g)
                    .and_then(|g| g.tree.get("m_Name"))
                    .and_then(|n| n.str())
                    .unwrap_or_default(),
            );
            t = pptr(&tr.tree, "m_Father");
        }
        names.reverse();
        out.insert(o.id, names.join("/"));
    }
    out
}

/// The layers of `style` (its active SpriteRenderers, with their activeness below the style's own
/// object), their sprites' images and the camera's grade.
pub fn layers(
    source: &Source,
    scene: &Scene,
    style: &str,
) -> Result<(Vec<(Layer, Image)>, Grade)> {
    let paths = paths(scene);
    let mut go_tr: HashMap<i64, i64> = HashMap::new();
    for o in &scene.objects {
        if o.typename == "Transform" || o.typename == "RectTransform" {
            go_tr.insert(pptr(&o.tree, "m_GameObject"), o.id);
        }
    }
    let world = |go: i64| -> Mat {
        let mut chain = Vec::new();
        let mut t = go_tr.get(&go).copied().unwrap_or(0);
        let mut guard = 0;
        while t != 0 && guard < 64 {
            guard += 1;
            let Some(tr) = scene.object(t) else { break };
            chain.push(trs(
                vec3(tr.tree.get("m_LocalPosition")),
                quat(tr.tree.get("m_LocalRotation")),
                vec3(tr.tree.get("m_LocalScale")),
            ));
            t = pptr(&tr.tree, "m_Father");
        }
        let mut m = identity();
        for local in chain.iter().rev() {
            m = mul(&m, local);
        }
        m
    };
    let camera_go = paths
        .iter()
        .find(|(_, p)| p.as_str() == CAMERA_PATH)
        .map(|(id, _)| *id)
        .ok_or("no title camera")?;
    let cam = apply(&world(camera_go), [0.0, 0.0, 0.0]);
    let camera_rotation = {
        let m = world(camera_go);
        (m[0][0] - 1.0).abs() + (m[1][1] - 1.0).abs() + (m[2][2] - 1.0).abs()
    };
    if camera_rotation > 1e-6 {
        return err("the title camera is rotated; the composer assumes it faces +z");
    }
    let camera_fov = scene
        .objects
        .iter()
        .find(|o| o.typename == "Camera" && pptr(&o.tree, "m_GameObject") == camera_go)
        .and_then(|o| o.tree.get("field of view").or_else(|| o.tree.get("m_FieldOfView")))
        .and_then(Value::float)
        .unwrap_or(24.0);
    let focal = (HEIGHT as f64 / 2.0) / (camera_fov.to_radians() / 2.0).tan();
    let prefix = format!("Menu_Styles/{style}/");
    let root = format!("Menu_Styles/{style}");
    let active = |go: i64| scene.object(go).and_then(|g| g.tree.get("m_IsActive")).map(Value::truthy).unwrap_or(true);
    let mut out = Vec::new();
    for o in &scene.objects {
        if o.typename != "SpriteRenderer" {
            continue;
        }
        let go = pptr(&o.tree, "m_GameObject");
        let Some(path) = paths.get(&go) else { continue };
        if !path.starts_with(&prefix) {
            continue;
        }
        if !o.tree.get("m_Enabled").map(Value::truthy).unwrap_or(true) {
            continue;
        }
        // Active below the style's own object: the style itself is switched on when it loads.
        let mut visible = true;
        let mut check = path.clone();
        while check != root {
            let id = paths.iter().find(|(_, p)| **p == check).map(|(id, _)| *id);
            if let Some(id) = id {
                visible &= active(id);
            }
            match check.rfind('/') {
                Some(i) => check.truncate(i),
                None => break,
            }
        }
        if !visible {
            continue;
        }
        let Some(sprite_ptr) = o.tree.get("m_Sprite") else { continue };
        let Ok(sprite_obj) = scene.deref(sprite_ptr) else { continue };
        let sprite = source.read(&sprite_obj).map_err(|e| e.to_string())?;
        let sprite_name = sprite.get("m_Name").and_then(|n| n.str()).unwrap_or_default();
        let (image, _) = native_sprite(source, &sprite_obj)?;
        let ppu = sprite.get("m_PixelsToUnits").and_then(Value::float).unwrap_or(100.0);
        let (w, h) = (image.width as f64 / ppu, image.height as f64 / ppu);
        let material = o
            .tree
            .get("m_Materials")
            .and_then(|m| m.list())
            .and_then(|l| l.first())
            .and_then(|p| scene.deref(p).ok())
            .and_then(|m| source.read(&m).ok())
            .and_then(|m| m.get("m_Name").and_then(|n| n.str()))
            .unwrap_or_default();
        let c = o.tree.get("m_Color");
        let cf = |k: &str| c.and_then(|c| c.get(k)).and_then(Value::float).unwrap_or(1.0);
        let flip_x = o.tree.get("m_FlipX").map(Value::truthy).unwrap_or(false);
        let flip_y = o.tree.get("m_FlipY").map(Value::truthy).unwrap_or(false);
        let m = world(go);
        let mut local = [[-w / 2.0, h / 2.0], [w / 2.0, h / 2.0], [-w / 2.0, -h / 2.0], [w / 2.0, -h / 2.0]];
        if flip_x {
            local.swap(0, 1);
            local.swap(2, 3);
        }
        if flip_y {
            local.swap(0, 2);
            local.swap(1, 3);
        }
        let mut corners = [[0.0; 2]; 4];
        let mut depth = 0.0;
        for (i, p) in local.iter().enumerate() {
            let world_p = apply(&m, [p[0], p[1], 0.0]);
            let z = world_p[2] - cam[2];
            if z <= 1.0 {
                return err(format!("{path}: a corner is behind the camera"));
            }
            corners[i] = [
                WIDTH as f64 / 2.0 + (world_p[0] - cam[0]) * focal / z,
                HEIGHT as f64 / 2.0 - (world_p[1] - cam[1]) * focal / z,
            ];
            depth += z / 4.0;
        }
        out.push((
            Layer {
                name: path.clone(),
                sprite: sprite_name,
                screen_blend: material == "UIBlendScreen",
                colour: [cf("r"), cf("g"), cf("b"), cf("a")],
                corners,
                depth,
            },
            image,
        ));
    }
    // Far to near; sprites share sorting order 0 in these styles.
    out.sort_by(|a, b| b.0.depth.partial_cmp(&a.0.depth).unwrap());
    Ok((out, Grade { saturation: 0.8 }))
}

/// Solve for the homography taking the unit square (u right, v down) to `corners`, returning its inverse
/// as 9 coefficients so a screen point maps back to (u, v).
fn inverse_homography(c: &[[f64; 2]; 4]) -> Option<[f64; 9]> {
    // Forward: x = (a u + b v + c0) / (g u + h v + 1), y = (d u + e v + f) / (g u + h v + 1).
    let uv = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let mut a = [[0.0f64; 9]; 8];
    for i in 0..4 {
        let (u, v) = (uv[i][0], uv[i][1]);
        let (x, y) = (c[i][0], c[i][1]);
        a[2 * i] = [u, v, 1.0, 0.0, 0.0, 0.0, -u * x, -v * x, x];
        a[2 * i + 1] = [0.0, 0.0, 0.0, u, v, 1.0, -u * y, -v * y, y];
    }
    for col in 0..8 {
        let pivot = (col..8).max_by(|&i, &j| a[i][col].abs().partial_cmp(&a[j][col].abs()).unwrap())?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        let p = a[col][col];
        for k in col..9 {
            a[col][k] /= p;
        }
        for r in 0..8 {
            if r != col {
                let f = a[r][col];
                for k in col..9 {
                    a[r][k] -= f * a[col][k];
                }
            }
        }
    }
    let h = [a[0][8], a[1][8], a[2][8], a[3][8], a[4][8], a[5][8], a[6][8], a[7][8], 1.0];
    // Invert the 3x3.
    let m = h;
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = [
        (m[4] * m[8] - m[5] * m[7]) / det,
        (m[2] * m[7] - m[1] * m[8]) / det,
        (m[1] * m[5] - m[2] * m[4]) / det,
        (m[5] * m[6] - m[3] * m[8]) / det,
        (m[0] * m[8] - m[2] * m[6]) / det,
        (m[2] * m[3] - m[0] * m[5]) / det,
        (m[3] * m[7] - m[4] * m[6]) / det,
        (m[1] * m[6] - m[0] * m[7]) / det,
        (m[0] * m[4] - m[1] * m[3]) / det,
    ];
    Some(inv)
}

fn sample(image: &Image, u: f64, v: f64) -> [f64; 4] {
    let x = (u * image.width as f64 - 0.5).clamp(0.0, image.width as f64 - 1.0);
    let y = (v * image.height as f64 - 0.5).clamp(0.0, image.height as f64 - 1.0);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(image.width - 1), (y0 + 1).min(image.height - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let mut out = [0.0; 4];
    // Premultiplied interpolation.
    for (px, py, wgt) in [(x0, y0, (1.0 - fx) * (1.0 - fy)), (x1, y0, fx * (1.0 - fy)), (x0, y1, (1.0 - fx) * fy), (x1, y1, fx * fy)] {
        let p = image.pixel(px, py);
        let a = f64::from(p[3]) / 255.0;
        for k in 0..3 {
            out[k] += wgt * f64::from(p[k]) / 255.0 * a;
        }
        out[3] += wgt * a;
    }
    if out[3] > 1e-9 {
        for k in 0..3 {
            out[k] /= out[3];
        }
    }
    out
}

/// The composition of the layers on black, graded.
pub fn compose(layers: &[(Layer, Image)], grade: &Grade) -> Vec<[f64; 3]> {
    let mut canvas = vec![[0.0f64; 3]; WIDTH * HEIGHT];
    for (layer, image) in layers {
        let c = &layer.corners;
        // Bring the sprite near its on-screen size first, so the sampling is not aliased.
        let on_x = ((c[1][0] - c[0][0]).hypot(c[1][1] - c[0][1])).max(1.0);
        let on_y = ((c[2][0] - c[0][0]).hypot(c[2][1] - c[0][1])).max(1.0);
        let image = if on_x < image.width as f64 * 0.7 || on_y < image.height as f64 * 0.7 {
            let w = (on_x.ceil() as usize).clamp(1, image.width);
            let h = (on_y.ceil() as usize).clamp(1, image.height);
            image.resize(w, h, Filter::Lanczos)
        } else {
            image.clone()
        };
        let Some(inv) = inverse_homography(c) else { continue };
        let (min_x, max_x) = (c.iter().map(|p| p[0]).fold(f64::MAX, f64::min), c.iter().map(|p| p[0]).fold(f64::MIN, f64::max));
        let (min_y, max_y) = (c.iter().map(|p| p[1]).fold(f64::MAX, f64::min), c.iter().map(|p| p[1]).fold(f64::MIN, f64::max));
        let (x0, x1) = ((min_x.floor().max(0.0)) as usize, (max_x.ceil().min(WIDTH as f64)) as usize);
        let (y0, y1) = ((min_y.floor().max(0.0)) as usize, (max_y.ceil().min(HEIGHT as f64)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w = inv[6] * px + inv[7] * py + inv[8];
                let u = (inv[0] * px + inv[1] * py + inv[2]) / w;
                let v = (inv[3] * px + inv[4] * py + inv[5]) / w;
                if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                    continue;
                }
                let s = sample(&image, u, v);
                let a = s[3] * layer.colour[3];
                if a <= 0.0 {
                    continue;
                }
                let dst = &mut canvas[y * WIDTH + x];
                for k in 0..3 {
                    let src = s[k] * layer.colour[k];
                    if layer.screen_blend {
                        let t = (src * a).clamp(0.0, 1.0);
                        dst[k] = 1.0 - (1.0 - dst[k]) * (1.0 - t);
                    } else {
                        dst[k] = dst[k] * (1.0 - a) + src * a;
                    }
                }
            }
        }
    }
    for p in canvas.iter_mut() {
        let luma = 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
        for k in 0..3 {
            p[k] = (luma + (p[k] - luma) * grade.saturation).clamp(0.0, 1.0);
        }
    }
    canvas
}

/// The composition as an RGBA image.
pub fn to_image(canvas: &[[f64; 3]]) -> Image {
    let mut image = Image::new(Mode::Rgba, WIDTH, HEIGHT);
    for (i, p) in canvas.iter().enumerate() {
        for k in 0..3 {
            image.data[i * 4 + k] = (p[k] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        image.data[i * 4 + 3] = 255;
    }
    image
}

/// The styles the PS1 title carries, in the order the options list them: the scene's object name, the
/// logo sprite its `LogoTitle` shows (sharedassets1.assets: `title`, or `vheart_title` for
/// `MenuStyles.styles[..].titleIndex` 0) and its path id.
pub const SHIPPED: [(&str, &str, i64); 2] =
    [("Modern_Style", "title", 121), ("VHeart_menu_Style", "vheart_title", 110)];
/// The logo's width on screen for the first style's sprite (1274 px wide), in pixels; every logo keeps the
/// same pixels per sprite pixel.
const LOGO_WIDTH: usize = 278;
const LOGO_REFERENCE: usize = 1274;

/// A logo as two halves of 4bpp coverage (a texture page is 256 pixels wide), rows padded to halfwords.
fn logo(source: &Source, id: i64) -> std::result::Result<[(usize, usize, Vec<u8>); 2], String> {
    let file = source.file("sharedassets1.assets").map_err(|e| e.to_string())?;
    let object = source.object(&file, id).map_err(|e| e.to_string())?;
    let (image, _) = native_sprite(source, &object)?;
    let w = (image.width * LOGO_WIDTH + LOGO_REFERENCE / 2) / LOGO_REFERENCE;
    let h = (image.height * LOGO_WIDTH + LOGO_REFERENCE / 2) / LOGO_REFERENCE;
    let small = image.to_rgba().resize(w, h, Filter::Lanczos);
    let grey = crate::dialogue_art::indices(&small);
    let left = w / 2;
    let half = |x0: usize, width: usize| {
        let mut part = Vec::with_capacity(width * h);
        for y in 0..h {
            part.extend_from_slice(&grey[y * w + x0..y * w + x0 + width]);
        }
        (width, h, crate::dialogue_art::whole_rows(&part, width, h))
    };
    Ok([half(0, left), half(left, w - left)])
}

/// `hk-cook title-style [style ...] [--source DIR]`: compose the named styles (the two the title carries
/// when none is named; `all` for every style in the scene, as previews) into `.hkpsx/title-styles/`, and
/// cook the carried styles' logos into `data/menu-logos.hk` and `data/menu_logos.rs`.
pub fn main(root: &Path, source: Option<&Path>, names: &[String]) -> std::result::Result<(), String> {
    let source = match source {
        Some(directory) => Source::new(directory).map_err(|e| e.to_string())?,
        None => Source::from_doctor(root).map_err(|e| e.to_string())?,
    };
    let scene = Scene::open(&source, "level1", false).map_err(|e| e.to_string())?;
    let wanted: Vec<String> = if names.is_empty() {
        SHIPPED.iter().map(|s| s.0.to_string()).collect()
    } else if names == ["all"] {
        style_names(&scene)
    } else {
        names.to_vec()
    };
    let dir = root.join(".hkpsx/title-styles");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for style in wanted {
        let (layers, grade) = layers(&source, &scene, &style)?;
        if std::env::var_os("HK_TITLE_STYLE_VERBOSE").is_some() {
            for (layer, _) in &layers {
                eprintln!(
                    "{style}: {} [{}{}] colour ({:.2},{:.2},{:.2},{:.2}) depth {:.1}",
                    layer.name,
                    layer.sprite,
                    if layer.screen_blend { ", screen" } else { "" },
                    layer.colour[0],
                    layer.colour[1],
                    layer.colour[2],
                    layer.colour[3],
                    layer.depth
                );
            }
        }
        let canvas = compose(&layers, &grade);
        save_rgba(&dir.join(format!("{style}.png")), &to_image(&canvas))?;
    }
    if names.is_empty() {
        let mut blob: Vec<u8> = Vec::new();
        let mut rust = String::from("// Generated by hk-cook title-style (host/hk-cook/src/title_style.rs).\n");
        rust += "/// Per style, the logo's two halves: offset into data/menu-logos.hk, width and height in pixels.\n";
        rust += &format!("pub const LOGOS: [[(usize, u8, u8); 2]; {}] = [\n", SHIPPED.len());
        for (style, sprite, id) in SHIPPED {
            let halves = logo(&source, id)?;
            rust += "    [";
            for (width, height, rows) in &halves {
                rust += &format!("({}, {width}, {height}), ", blob.len());
                blob.extend_from_slice(rows);
            }
            rust += &format!("], // {style}: {sprite}\n");
        }
        rust += "];\n";
        rust += &format!("pub const LOGO_BYTES: usize = {};\n", blob.len());
        std::fs::write(root.join("data/menu-logos.hk"), &blob).map_err(|e| e.to_string())?;
        std::fs::write(root.join("data/menu_logos.rs"), rust).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_turn_about_z_rotates_the_x_axis_onto_y() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let m = trs([1.0, 2.0, 3.0], [0.0, 0.0, s, s], [2.0, 2.0, 2.0]);
        let p = apply(&m, [1.0, 0.0, 0.0]);
        assert!((p[0] - 1.0).abs() < 1e-9 && (p[1] - 4.0).abs() < 1e-9 && (p[2] - 3.0).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn the_inverse_homography_takes_each_corner_back_to_its_unit_square_corner() {
        let corners = [[10.0, 20.0], [110.0, 30.0], [5.0, 120.0], [120.0, 130.0]];
        let inv = inverse_homography(&corners).expect("a quad");
        for (i, (u, v)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].iter().enumerate() {
            let (x, y) = (corners[i][0], corners[i][1]);
            let w = inv[6] * x + inv[7] * y + inv[8];
            let (gu, gv) = ((inv[0] * x + inv[1] * y + inv[2]) / w, (inv[3] * x + inv[4] * y + inv[5]) / w);
            assert!((gu - u).abs() < 1e-6 && (gv - v).abs() < 1e-6, "corner {i}: {gu} {gv}");
        }
    }

    #[test]
    fn screen_blend_never_darkens_and_a_normal_layer_replaces_by_its_alpha() {
        let mut image = Image::new(Mode::Rgba, 2, 2);
        for p in image.data.chunks_exact_mut(4) {
            p.copy_from_slice(&[255, 255, 255, 255]);
        }
        let corners = [[0.0, 0.0], [320.0, 0.0], [0.0, 240.0], [320.0, 240.0]];
        let layer = |screen_blend: bool, colour: [f64; 4]| Layer {
            name: String::new(),
            sprite: String::new(),
            screen_blend,
            colour,
            corners,
            depth: 1.0,
        };
        let grade = Grade { saturation: 1.0 };
        let base = compose(&[(layer(false, [0.5, 0.5, 0.5, 1.0]), image.clone())], &grade);
        assert!((base[1000][0] - 0.5).abs() < 1e-9);
        let both = compose(
            &[(layer(false, [0.5, 0.5, 0.5, 1.0]), image.clone()), (layer(true, [0.5, 0.0, 0.0, 1.0]), image)],
            &grade,
        );
        // 1 - (1 - 0.5)(1 - 0.5) = 0.75 on red; the other channels stay.
        assert!((both[1000][0] - 0.75).abs() < 1e-9 && (both[1000][1] - 0.5).abs() < 1e-9);
    }
}
