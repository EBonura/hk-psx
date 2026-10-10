//! The quick map: the source's `Map Control` FSM on the Knight, its `Quick Map`
//! FSM and the `GameMap` rules `host/game_map.py` documents, over the room art
//! that cook packs into the one texture page `hk_cache::residency::MAP_PAGE`
//! reserves. The art is uploaded once after the title and never touched again.
//!
//! Opening follows `Map Control`: the map button held for `Button Down Time`
//! (0.1 s) while the Knight could act (`CanQuickMap`), not at a bench and with
//! a map bought (`hasMap`) opens it; letting go, leaving the ground, a hit or a
//! scene gate closes it. While it is up the Knight stands (the source lets him
//! walk slowly, `Map Walk`, which is not reproduced) and the world runs on.
//! The port's map button is L2: the pad has no spare face button, and this is
//! the one L1's neighbour leaves free.
//!
//! The page is shared with the one scene that needs all nineteen static
//! pages, the False Knight's arena: while it is admitted the map is not there
//! and the button does nothing, and the next gate out reads the art back.
//!
//! What it shows follows `Quick Map`: only the area the Knight stands in, only
//! if that area's map is owned (`mapDirtmouth` is a new-save default,
//! `mapCrossroads` is Cornifer's), otherwise `NO_MAP`. Inside an owned area a
//! charted room shows its rough sprite until `scenesMapped` holds it, an
//! uncharted room only once it does, a bench pin while `hasPinBench` is set
//! and the Knight marker while Wayward Compass is worn.
//!
//! The PlayerData lives in `persist`: the bools as player bits, and
//! `scenesVisited` / `scenesMapped` as one `MapScene` item per scene.
use crate::persist;
use psx_gpu::{material::BlendMode, ot::OrderingTable, prim::QuadGouraudBlended};
use psx_vram::{upload_bytes, Clut, TexDepth, Tpage, VramRect};

pub struct MapTex {
    pub u: u16,
    pub v: u16,
    pub w: u16,
    pub h: u16,
}
pub struct MapArea {
    pub name: &'static str,
    pub first: u8,
    pub count: u8,
    pub w: i16,
    pub h: i16,
}
pub struct MapRoom {
    pub scene: u8,
    pub area: u8,
    pub charted: bool,
    pub x: i16,
    pub y: i16,
    pub rough: Option<MapTex>,
    pub rough_dx: i16,
    pub rough_dy: i16,
    pub full: MapTex,
}
pub struct MapPin {
    pub room: u8,
    pub x: i16,
    pub y: i16,
}
/// A scene's room and its tilemap rectangle (Q16 world units), or its door in
/// the scene its only gate leads to (`GameMap.PositionCompass`'s inRoom case).
pub struct SceneMap {
    pub room: u8,
    pub bounds: [i32; 4],
    pub door: Option<[i32; 2]>,
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/game_map.rs"));
// The room art's size and checksum: the blob streams from WORLD.PAK before the
// title (disc::Cache::prepare_world_sfx) instead of being linked.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/game-map.rs"));
const _: () = assert!(BANK_BYTES == 128 * (MAP_ROWS as usize + 1));
const _: () = assert!(
    MAP_PAGE_XY.0 as usize == hk_cache::residency::MAP_PAGE.0
        && MAP_PAGE_XY.1 as usize == hk_cache::residency::MAP_PAGE.1
);
// The palettes are the page's own last row, below every texel row.
const _: () = assert!(
    MAP_CLUT_XY[0].0 as usize == hk_cache::residency::MAP_PAGE.0
        && MAP_CLUT_XY[0].1 as usize
            == hk_cache::residency::MAP_PAGE.1 + hk_cache::residency::MAP_PAGE.3 - 1
);
const _: () = assert!((MAP_ROWS as usize) < hk_cache::residency::MAP_PAGE.3);

/// `Map Control`'s `Button Down Time`, 0.1 s at the 60 Hz tick.
pub const BUTTON_DOWN_TICKS: u16 = 6;
pub const MAP_BUTTON: u16 = psx_pad::button::L2;
/// `MapScene` item value bits.
pub const VISITED: u8 = 1;
pub const MAPPED: u8 = 2;

struct Quick {
    held: u16,
    open: bool,
    release: bool,
}
static mut QUICK: Quick = Quick {
    held: 0,
    open: false,
    release: false,
};
#[no_mangle]
pub static mut HK_MAP_OPENS: u32 = 0;
#[no_mangle]
pub static mut HK_MAP_OPEN: u32 = 0;
/// Rooms drawn by the last open frame, and whether the marker was one of them.
#[no_mangle]
pub static mut HK_MAP_ROOMS_DRAWN: u32 = 0;
#[no_mangle]
pub static mut HK_MAP_MARKER: u32 = 0;
/// `scenesVisited` and `scenesMapped` counts, for routes.
#[no_mangle]
pub static mut HK_MAP_VISITED: u32 = 0;
#[no_mangle]
pub static mut HK_MAP_MAPPED: u32 = 0;

static mut READY: bool = false;
pub fn ready() -> bool {
    unsafe { READY }
}
/// A scene's atlas is about to overwrite page 18: the map (and the Shaman)
/// cannot be drawn until disc::Cache::prepare_map reads it back.
pub fn lose() {
    unsafe {
        READY = false;
    }
    cancel();
}
/// Upload the streamed blob: the two palettes, then the page rows. Nothing
/// else ever writes page 18 or those two CLUT rows, the title art included,
/// so this happens once, before the title, and the bytes are not kept.
pub fn upload(blob: &[u8]) {
    assert_eq!(blob.len(), BANK_BYTES);
    let (page, palettes) = blob.split_at(128 * MAP_ROWS as usize);
    upload_bytes(
        VramRect::new(MAP_CLUT_XY[0].0, MAP_CLUT_XY[0].1, 64, 1),
        palettes,
    );
    upload_bytes(
        VramRect::new(MAP_PAGE_XY.0, MAP_PAGE_XY.1, 64, MAP_ROWS),
        page,
    );
    unsafe {
        READY = true;
    }
}
pub fn open() -> bool {
    unsafe { QUICK.open }
}
/// A hit, a death, a scene gate: `Cancel All`.
pub fn cancel() {
    unsafe {
        if QUICK.open {
            QUICK.release = true;
        }
        QUICK.open = false;
        QUICK.held = 0;
        HK_MAP_OPEN = 0;
    }
}
fn scene_bits(scene: usize) -> u8 {
    persist::get(persist::Kind::MapScene, scene, 0).unwrap_or(0)
}
// ponytail: `GameManager.AddToScenesVisited` is not recorded. scenesVisited
// only reaches the map through the quill (`UpdateGameMap`), which is Iselda's
// and her shop is not on the disc, so each visit would spend a 128-slot world
// item for nothing. Record visits (as a per-scene bitmask in the save, not
// items) when the quill can be had; VISITED is the bit update_game_map reads.
/// The area map a room belongs to is owned.
fn area_owned(area: usize) -> bool {
    match area {
        0 => true,
        1 => persist::player(persist::MAP_CROSSROADS),
        _ => false,
    }
}
fn scene_area(scene: usize) -> Option<usize> {
    let room = SCENE_MAP.get(scene)?.room;
    (room != 255).then(|| MAP_ROOMS[room as usize].area as usize)
}
/// `PlayerData.UpdateGameMap`: with the quill, every visited scene whose area
/// map is owned (`HasMapForScene`) joins `scenesMapped`. Returns whether any did.
pub fn update_game_map() -> bool {
    if !persist::player(persist::HAS_QUILL) {
        return false;
    }
    let mut changed = false;
    for scene in 0..SCENE_MAP.len() {
        let bits = scene_bits(scene);
        if bits & VISITED != 0 && bits & MAPPED == 0 && scene_area(scene).is_some_and(area_owned) {
            persist::set(persist::Kind::MapScene, scene, 0, bits | MAPPED);
            changed = true;
        }
    }
    publish();
    changed
}
pub fn publish() {
    let (mut visited, mut mapped) = (0, 0);
    for (_, _, bits) in persist::store().all(persist::Kind::MapScene) {
        visited += u32::from(bits & VISITED != 0);
        mapped += u32::from(bits & MAPPED != 0);
    }
    unsafe {
        HK_MAP_VISITED = visited;
        HK_MAP_MAPPED = mapped;
    }
}
/// One tick of `Map Control`. `can_open` is `CanQuickMap` and everything else
/// that owns the Knight (a bench, a panel, the shop); `grounded` closes it
/// (`LEFT GROUND`).
pub fn tick(bits: u16, can_open: bool, grounded: bool) {
    let q = unsafe { &mut *(&raw mut QUICK) };
    let down = bits & MAP_BUTTON != 0;
    if !down {
        q.release = false;
    }
    if q.open {
        if !down || !grounded {
            q.open = false;
            q.held = 0;
        }
    } else if down && !q.release && can_open && ready() && persist::player(persist::HAS_MAP) {
        q.held = q.held.saturating_add(1);
        if q.held >= BUTTON_DOWN_TICKS {
            q.open = true;
            unsafe {
                HK_MAP_OPENS = HK_MAP_OPENS.wrapping_add(1);
            }
        }
    } else {
        q.held = 0;
    }
    unsafe {
        HK_MAP_OPEN = u32::from(q.open);
    }
}

#[repr(C, align(4))]
struct Sprite {
    tag: u32,
    mode: u32,
    color: u32,
    xy: u32,
    uv: u32,
    wh: u32,
}
const EMPTY: Sprite = Sprite {
    tag: 0,
    mode: 0,
    color: 0,
    xy: 0,
    uv: 0,
    wh: 0,
};
const CAP: usize = 64;
static mut SPRITES: [Sprite; CAP] = [const { EMPTY }; CAP];
static mut USED: usize = 0;
static mut BACKDROP: QuadGouraudBlended = QuadGouraudBlended::new(
    [(0, 0), (320, 0), (0, 240), (320, 240)],
    [(0, 0, 0); 4],
    BlendMode::Subtract,
);
static mut SHOW_BACKDROP: bool = false;
fn sprite(x: i16, y: i16, t: &MapTex, clut: usize) {
    if x >= 320 || y >= 240 || x + (t.w as i16) <= 0 || y + (t.h as i16) <= 0 {
        return;
    }
    unsafe {
        if USED >= CAP {
            return;
        }
        let p = &mut SPRITES[USED];
        USED += 1;
        p.mode = 0xe100_0000
            | Tpage::new(MAP_PAGE_XY.0, MAP_PAGE_XY.1, TexDepth::Bit4).uv_tpage_word(0) as u32;
        p.color = 0x6480_8080;
        p.xy = x as u16 as u32 | ((y as u16 as u32) << 16);
        p.uv = t.u as u32
            | ((t.v as u32) << 8)
            | ((Clut::new(MAP_CLUT_XY[clut].0, MAP_CLUT_XY[clut].1).uv_clut_word() as u32) << 16);
        p.wh = t.w as u32 | ((t.h as u32) << 16);
    }
}
/// Compose the open map for the Knight in `scene` at `hero` (Q16 world). The
/// text goes through the dialogue panel's glyphs, so call after
/// `dialogue::prepare`.
#[inline(never)]
pub fn prepare(scene: usize, hero: [i32; 2]) {
    unsafe {
        USED = 0;
        SHOW_BACKDROP = false;
    }
    if !open() {
        return;
    }
    unsafe {
        SHOW_BACKDROP = true;
        BACKDROP = QuadGouraudBlended::new(
            [(0, 0), (320, 0), (0, 240), (320, 240)],
            [(160, 160, 160); 4],
            BlendMode::Subtract,
        );
    }
    let Some(area) = scene_area(scene).filter(|&a| area_owned(a)) else {
        let text = NO_MAP;
        crate::dialogue::panel_text(160 - crate::dialogue::panel_width(text) / 2, 112, text);
        unsafe {
            HK_MAP_ROOMS_DRAWN = 0;
            HK_MAP_MARKER = 0;
        }
        return;
    };
    let a = &MAP_AREAS[area];
    let ox = 160 - a.w / 2;
    let oy = 128 - a.h / 2;
    crate::dialogue::panel_text(
        160 - crate::dialogue::panel_width(a.name) / 2,
        (oy - 22).max(20),
        a.name,
    );
    let quill = persist::player(persist::HAS_QUILL);
    let pins = persist::player(persist::HAS_PIN_BENCH);
    let mut drawn = 0;
    let rooms = &MAP_ROOMS[a.first as usize..a.first as usize + a.count as usize];
    let mut shown = [false; 64];
    for (i, room) in rooms.iter().enumerate() {
        let mapped = room.scene != 255 && scene_bits(room.scene as usize) & MAPPED != 0;
        // SetupMap switches the room on; RoughMapRoom picks the sprite.
        if !(room.charted || (quill && mapped)) {
            continue;
        }
        shown[i.min(63)] = true;
        drawn += 1;
        match (&room.rough, mapped) {
            (Some(t), false) => sprite(
                ox + room.x + room.rough_dx,
                oy + room.y + room.rough_dy,
                t,
                0,
            ),
            _ => sprite(ox + room.x, oy + room.y, &room.full, 0),
        }
    }
    if pins {
        for pin in MAP_PINS {
            let local = pin.room as usize - a.first as usize;
            if (pin.room as usize) < a.first as usize
                || local >= rooms.len()
                || !shown[local.min(63)]
            {
                continue;
            }
            sprite(ox + pin.x, oy + pin.y, &MAP_BENCH_PIN, 1);
        }
    }
    let mut marker = false;
    if crate::charms::state().equipped(2) {
        if let Some(s) = SCENE_MAP.get(scene).filter(|s| s.room != 255) {
            let room = &MAP_ROOMS[s.room as usize];
            let at = s.door.unwrap_or(hero);
            let b = s.bounds;
            let fx = ((at[0] - b[0]).clamp(0, b[2] - b[0]) as i64 * room.full.w as i64
                / (b[2] - b[0]).max(1) as i64) as i16;
            let fy = ((b[3] - at[1]).clamp(0, b[3] - b[1]) as i64 * room.full.h as i64
                / (b[3] - b[1]).max(1) as i64) as i16;
            sprite(
                ox + room.x + fx - (MAP_MARKER.w as i16) / 2,
                oy + room.y + fy - (MAP_MARKER.h as i16) / 2,
                &MAP_MARKER,
                1,
            );
            marker = true;
        }
    }
    unsafe {
        HK_MAP_ROOMS_DRAWN = drawn;
        HK_MAP_MARKER = u32::from(marker);
    }
}
/// OT insertion prepends: the backdrop goes in last so it draws first, under
/// the rooms, and the marker (last composed) goes in first so it draws on top.
pub fn append(ot: &mut OrderingTable<1>) {
    unsafe {
        for i in (0..USED).rev() {
            crate::display::ot_add(ot, 0, &mut SPRITES[i], 5);
        }
        if SHOW_BACKDROP {
            crate::display::ot_add(ot, 0, &mut *(&raw mut BACKDROP), QuadGouraudBlended::WORDS);
        }
    }
}
