//! Geo art is uploaded once into proven VRAM gaps, outside the scene cache.
use crate::{geo, render, world};
use psx_gpu::{
    material::{BlendMode, TextureMaterial},
    prim::QuadTextured,
};
use psx_vram::{upload_bytes, VramRect};
#[no_mangle]
pub static mut HK_GEO_DRAWN: u32 = 0;
/// Upload the Geo art from the boot art chunk (host/build_guest.py), which
/// the title stages in the arena; nothing else reads it.
pub fn upload(boot: &[u8]) {
    let (at, len) = crate::boot_art::GEO_ART;
    let data = &boot[at..at + len];
    assert!(data.len() <= 8192);
    for entry in geo::GEO_UPLOADS {
        assert!(hk_cache::residency::GEO_RECTS
            .iter()
            .any(|&(x, y, w, h)| entry.x as usize >= x
                && entry.y as usize >= y
                && entry.x as usize + entry.w as usize <= x + w
                && entry.y as usize + entry.h as usize <= y + h));
        let len = entry.w as usize * entry.h as usize * 2;
        upload_bytes(
            VramRect::new(entry.x, entry.y, entry.w, entry.h),
            &data[entry.offset..entry.offset + len],
        );
    }
}
/// Refresh after a region change or a mining hit. Local edge IDs never escape
/// their region; only depleted source colliders are disabled.
/// `region` is the Knight's view (its colliders), `view` the one being drawn
/// (its cooked rock draws); they differ while the camera is outside the
/// Knight's view's range.
pub fn apply(geo: &geo::World, state: &mut world::State, region: usize, view: usize) {
    for binding in geo::region_bindings(view) {
        if geo.rock_depleted(world::scene_of(view), binding.state as usize) {
            for &draw in binding.off {
                render::set_visible(draw as usize, false);
            }
        }
    }
    let mut edges = [0u16; 32];
    let mut len = 0;
    for binding in geo::region_bindings(region) {
        if geo.rock_depleted(world::scene_of(region), binding.state as usize) {
            for &edge in binding.edges {
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
fn draw_art(index: usize, world: [[i32; 2]; 4], camera: (i32, i32)) -> bool {
    let vertices = world.map(|[x, y]| {
        (
            160 + (((i64::from(x) - i64::from(camera.0)) * i64::from(crate::KNIGHT_SCALE)) >> 28)
                as i32,
            120 - (((i64::from(y) - i64::from(camera.1)) * i64::from(crate::KNIGHT_SCALE)) >> 28)
                as i32,
        )
    });
    if vertices.iter().all(|p| p.0 < 0)
        || vertices.iter().all(|p| p.0 >= 320)
        || vertices.iter().all(|p| p.1 < 0)
        || vertices.iter().all(|p| p.1 >= 240)
    {
        return false;
    }
    let art = &geo::GEO_ART[index];
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
/// The Geo coin of the HUD: the small coin's first landed frame at the world
/// scale, its top left at (`x`, `y`), beside the wallet's count.
pub fn hud_coin(x: i16, y: i16) -> u32 {
    let index = geo::GEO_COIN_CLIPS[0][0].start as usize;
    let art = &geo::GEO_ART[index];
    let b = art.bounds;
    // One and a half times its world size, and at twice the texel's colour: a
    // coin in the world is 9 pixels and dim, and the HUD's has to read.
    let w = ((i64::from(b[2]) - i64::from(b[0])) * i64::from(crate::KNIGHT_SCALE) * 3 >> 29) as i16;
    let h = ((i64::from(b[3]) - i64::from(b[1])) * i64::from(crate::KNIGHT_SCALE) * 3 >> 29) as i16;
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
    render::hud_quad(
        &template,
        [(x, y), (x + w, y), (x, y + h), (x + w, y + h)],
        (255, 255, 255),
    );
    1
}
#[inline(never)]
pub fn draw(geo: &geo::World, region: usize, camera: (i32, i32)) -> u32 {
    let scene = world::scene_of(region);
    let mut drawn = 0;
    for binding in geo::region_bindings(region) {
        let (art, vertices, extra) = if geo.rock_depleted(scene, binding.state as usize) {
            (binding.broken_art, binding.vertices, binding.broken_extra)
        } else {
            (
                binding.intact_art,
                binding.intact_vertices,
                binding.intact_extra,
            )
        };
        drawn += u32::from(draw_art(art as usize, vertices, camera));
        for part in extra {
            drawn += u32::from(draw_art(part.art as usize, part.vertices, camera));
        }
    }
    for coin in geo
        .coins()
        .filter(|c| c.scene as usize == scene && c.denomination & geo::ROCK == 0)
    {
        let clip = geo::GEO_COIN_CLIPS[coin.denomination as usize][usize::from(!coin.landed)];
        let index = clip.start as usize
            + ((u64::from(coin.age) * u64::from(clip.fps) / 60) % u64::from(clip.count)) as usize;
        let b = geo::GEO_ART[index].bounds;
        let world = [
            [b[0] + coin.x, b[3] + coin.y],
            [b[2] + coin.x, b[3] + coin.y],
            [b[0] + coin.x, b[1] + coin.y],
            [b[2] + coin.x, b[1] + coin.y],
        ];
        drawn += u32::from(draw_art(index, world, camera));
    }
    assert!(drawn <= 80);
    unsafe {
        HK_GEO_DRAWN = drawn;
    }
    drawn
}
