//! Geo art is uploaded once into proven VRAM gaps, outside the scene cache.
use crate::{geo, render, world};
use hk_format::world_meta::{KIND_GEO_ART, KIND_GEO_UPLOAD, KIND_GEO_VIEW};
use psx_math::Wide;
use psx_gpu::{
    material::{BlendMode, TextureMaterial},
    prim::QuadTextured,
};
use psx_vram::{upload_bytes, VramRect};
#[no_mangle]
pub static mut HK_GEO_DRAWN: u32 = 0;
/// A rectangle of the Geo reservation, or a panic: nothing else may be written where the cache keeps its pages.
fn assert_reserved(x: usize, y: usize, w: usize, h: usize) {
    assert!(hk_cache::residency::GEO_RECTS
        .iter()
        .any(|&(rx, ry, rw, rh)| x >= rx && y >= ry && x + w <= rx + rw && y + h <= ry + rh));
}
/// Upload the coins' art from the boot art chunk (host/build_guest.py), which
/// the title stages in the arena; nothing else reads it. It stays for good: the rocks' art
/// (`upload_scene`) lives in the rest of the reservation.
pub fn upload(boot: &[u8]) {
    let (at, len) = crate::boot_art::GEO_ART;
    let data = &boot[at..at + len];
    assert!(data.len() <= 8192);
    for entry in geo::GEO_UPLOADS {
        assert_reserved(entry.x as usize, entry.y as usize, entry.w as usize, entry.h as usize);
        let len = entry.w as usize * entry.h as usize * 2;
        upload_bytes(
            VramRect::new(entry.x, entry.y, entry.w, entry.h),
            &data[entry.offset..entry.offset + len],
        );
    }
}
/// Upload the rock art of the scene whose metadata bank was just admitted: its `KIND_GEO_UPLOAD` objects (first
/// region) carry rows of halfwords for a rectangle each, cooked by host/geo.py into the part of the reservation the
/// coins leave. A scene without rocks has none and leaves the previous scene's art where it was; nothing draws it
/// because the art objects come from the admitted bank only.
pub fn upload_scene(bank: &hk_format::WorldMeta<'_>) {
    let Some(region) = bank.region(0) else {
        return;
    };
    for object in region.objects().flatten().filter(|o| o.kind() == KIND_GEO_UPLOAD) {
        let (x, y) = (object.state_id() & 0xFFFF, object.state_id() >> 16);
        let (w, h) = (usize::from(object.flags() & 0xFF), usize::from(object.flags() >> 8));
        assert!(w >= 1 && w <= 64 && h >= 1);
        assert_reserved(x as usize, y as usize, w, h);
        let mut words = object.indices(0);
        let mut row = [0u8; 128];
        for line in 0..h {
            for pair in row[..w * 2].chunks_exact_mut(2) {
                pair.copy_from_slice(&words.next().unwrap_or(0).to_le_bytes());
            }
            upload_bytes(VramRect::new(x as u16, y as u16 + line as u16, w as u16, 1), &row[..w * 2]);
        }
    }
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
fn draw_art(art: geo::GeoArt, world: [[i32; 2]; 4], camera: (i32, i32)) -> bool {
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
        let art = geo::GeoArt {
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
        drawn += u32::from(draw_art(geo::GEO_ART[index], world, camera));
    }
    assert!(drawn <= 80);
    unsafe {
        HK_GEO_DRAWN = drawn;
    }
    drawn
}
