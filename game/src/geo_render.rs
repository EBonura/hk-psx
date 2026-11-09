//! Geo art (the coins' frames and the scene's rocks) is uploaded with each scene into proven VRAM gaps, outside the scene cache.
use crate::{geo, render, world};
use hk_format::world_meta::{KIND_GEO_ART, KIND_GEO_VIEW};
use psx_math::Wide;
use psx_gpu::{
    material::{BlendMode, TextureMaterial},
    prim::QuadTextured,
};
#[cfg(not(test))]
use psx_vram::{upload_bytes, VramRect};
#[no_mangle]
pub static mut HK_GEO_DRAWN: u32 = 0;
/// Where the scene's bank put a coin frame (the frame's size is `geo::GEO_ART`): u, v and texture page. The palette is
/// shared by every frame of a bank.
#[derive(Clone, Copy)]
struct CoinTex {
    u: u8,
    v: u8,
    tpage: u16,
}
static mut COIN_TEX: [CoinTex; geo::GEO_ART.len()] =
    [CoinTex { u: 0, v: 0, tpage: 0 }; geo::GEO_ART.len()];
static mut COIN_CLUT: u16 = 0;
/// A texture as the quad draw needs it.
#[derive(Clone, Copy)]
struct Art {
    u: u8,
    v: u8,
    w: u8,
    h: u8,
    clut: u16,
    tpage: u16,
}
fn u16_at(data: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([data[at], data[at + 1]]))
}
/// Upload the Geo bank of the scene whose effect chunk `chunk` is (host/geo.py `block_bytes`; the chunk's last word is the
/// block's offset): the rectangles of coin and rock texels, and where each coin frame sits in them. Runs where the scene's
/// particle art is uploaded, before the scene owns the arena. False when the chunk is malformed, or a rectangle leaves the Geo
/// reservation (nothing else may be written where the cache keeps its pages).
#[cfg(not(test))]
pub fn upload_scene(chunk: &[u8]) -> bool {
    let Some(last) = chunk.len().checked_sub(4) else {
        return false;
    };
    let at = u32::from_le_bytes([chunk[last], chunk[last + 1], chunk[last + 2], chunk[last + 3]]) as usize;
    if at + 8 > last {
        return false;
    }
    let block = &chunk[at..last];
    let (uploads, coins) = (u16_at(block, 0), u16_at(block, 2));
    if coins != geo::GEO_ART.len() || 8 + uploads * 12 + coins * 4 > block.len() {
        return false;
    }
    unsafe {
        COIN_CLUT = u16_at(block, 4) as u16;
    }
    for i in 0..uploads {
        let row = 8 + i * 12;
        let (x, y, w, h) = (u16_at(block, row), u16_at(block, row + 2), u16_at(block, row + 4), u16_at(block, row + 6));
        let offset = u32::from_le_bytes([block[row + 8], block[row + 9], block[row + 10], block[row + 11]]) as usize;
        let end = offset + w * h * 2;
        let reserved = hk_cache::residency::GEO_RECTS
            .iter()
            .any(|&(rx, ry, rw, rh)| x >= rx && y >= ry && x + w <= rx + rw && y + h <= ry + rh);
        if !reserved || end > block.len() {
            return false;
        }
        upload_bytes(VramRect::new(x as u16, y as u16, w as u16, h as u16), &block[offset..end]);
    }
    let table = unsafe { &mut *(&raw mut COIN_TEX) };
    for (k, tex) in table.iter_mut().enumerate() {
        let b = &block[8 + uploads * 12 + k * 4..];
        *tex = CoinTex { u: b[0], v: b[1], tpage: u16_at(b, 2) as u16 };
    }
    true
}
#[cfg(test)]
pub fn upload_scene(_chunk: &[u8]) -> bool {
    true
}
/// Refresh after a region change or a mining hit. Local edge IDs never escape
/// their region; only depleted source colliders are disabled.
/// `region` is the Knight's view (its colliders), `view` the one being drawn
/// (its cooked rock draws); they differ while the camera is outside the
/// Knight's view's range.
pub fn apply(geo: &geo::World, state: &mut world::State, region: usize, view: usize) {
    for object in world::geo_view_objects(view).filter(|o| o.kind() == KIND_GEO_VIEW) {
        if geo.rock_depleted(world::scene_of(view), object.state_id() as usize) {
            let mut list = object.indices(0);
            let hidden = list.next().unwrap_or(0) as usize;
            for draw in list.take(hidden) {
                render::set_visible(draw as usize, false);
            }
        }
    }
    let mut edges = [0u16; 32];
    let mut len = 0;
    for object in world::geo_view_objects(region).filter(|o| o.kind() == KIND_GEO_VIEW) {
        if geo.rock_depleted(world::scene_of(region), object.state_id() as usize) {
            let mut list = object.indices(0);
            let hidden = list.next().unwrap_or(0) as usize;
            for edge in list.skip(hidden) {
                if !edges[..len].contains(&edge) {
                    assert!(len < edges.len());
                    edges[len] = edge;
                    len += 1;
                }
            }
        }
    }
    state.set_geo_edges(&edges[..len]);
}
/// The fourth corner of a rock art quad from its first three (TL, TR, BL).
fn parallelogram(p: [[i32; 2]; 3]) -> [[i32; 2]; 4] {
    [p[0], p[1], p[2], [p[1][0] + p[2][0] - p[0][0], p[1][1] + p[2][1] - p[0][1]]]
}
fn draw_art(art: Art, world: [[i32; 2]; 4], camera: (i32, i32)) -> bool {
    let vertices = world.map(|[x, y]| {
        (
            160 + hk_sim::fixed::project(x, camera.0, crate::KNIGHT_SCALE as i32, 28),
            120 - hk_sim::fixed::project(y, camera.1, crate::KNIGHT_SCALE as i32, 28),
        )
    });
    if vertices.iter().all(|p| p.0 < 0)
        || vertices.iter().all(|p| p.0 >= 320)
        || vertices.iter().all(|p| p.1 < 0)
        || vertices.iter().all(|p| p.1 >= 240)
    {
        return false;
    }
    let right = (u16::from(art.u) + u16::from(art.w) - 1) as u8;
    let bottom = (u16::from(art.v) + u16::from(art.h) - 1) as u8;
    let uv = [
        (art.u, art.v),
        (right, art.v),
        (art.u, bottom),
        (right, bottom),
    ];
    let template = QuadTextured::with_material(
        [(0, 0); 4],
        uv,
        TextureMaterial::blended(art.clut, art.tpage, (128, 128, 128), BlendMode::Average),
    );
    render::resident_quad(&template, vertices.map(|(x, y)| (x as i16, y as i16)));
    true
}
#[inline(never)]
pub fn draw(geo: &geo::World, region: usize, camera: (i32, i32)) -> u32 {
    let scene = world::scene_of(region);
    let mut drawn = 0;
    for object in world::geo_view_objects(region).filter(|o| o.kind() == KIND_GEO_ART) {
        let id = object.state_id();
        // Tag 0 art shows once the rock is broken, tag 1 while it is whole.
        if geo.rock_depleted(scene, (id & 0xFF) as usize) != (id >> 8 == 0) {
            continue;
        }
        let Some(corners) = world::geo_art_corners(&object) else {
            continue;
        };
        // The rock's texels sit wherever its scene's bank put them: u, v, w, h in word 0, clut and tpage in word 1.
        let (a, b) = (object.extra(0) as u32, object.extra(1) as u32);
        let art = Art {
            u: a as u8,
            v: (a >> 8) as u8,
            w: (a >> 16) as u8,
            h: (a >> 24) as u8,
            clut: b as u16,
            tpage: (b >> 16) as u16,
        };
        drawn += u32::from(draw_art(art, parallelogram(corners), camera));
    }
    for coin in geo
        .coins()
        .filter(|c| c.scene as usize == scene && c.denomination & geo::ROCK == 0)
    {
        let clip = geo::GEO_COIN_CLIPS[coin.denomination as usize][usize::from(!coin.landed)];
        let index = clip.start as usize
            // age * fps / 60 stays below 2^32 for any fps up to 60.
            + (hk_sim::fixed::mul_div_floor(coin.age, u32::from(clip.fps), 60) % u32::from(clip.count)) as usize;
        let b = geo::GEO_COIN_BOUNDS[index];
        let world = [
            [b[0] + coin.x, b[3] + coin.y],
            [b[2] + coin.x, b[3] + coin.y],
            [b[0] + coin.x, b[1] + coin.y],
            [b[2] + coin.x, b[1] + coin.y],
        ];
        drawn += u32::from(draw_art(
            {
                let size = geo::GEO_ART[index];
                let tex = unsafe { (*(&raw const COIN_TEX))[index] };
                Art { u: tex.u, v: tex.v, w: size.w, h: size.h, clut: unsafe { COIN_CLUT }, tpage: tex.tpage }
            },
            world,
            camera,
        ));
    }
    assert!(drawn <= 80);
    unsafe {
        HK_GEO_DRAWN = drawn;
    }
    drawn
}
