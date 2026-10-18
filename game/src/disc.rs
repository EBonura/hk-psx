//! Exclusive scene-gate loading or joint bootstrap residency. Each admitted
//! scene keeps immutable spatial views; only an exclusive select() at a drained,
//! blacked-out scene transition may reclaim the arena and replace static art.
use crate::room_decode::{Decoder, Error as DecodeError};
use hk_format::coverage::{CoverageValidation, CoverageView, Expected as CoverageExpected};
use hk_format::{Room, Scene, WorldMeta};
#[path = "cd_stream.rs"]
mod cd_stream;
use psx_pack::cd::{SectorReader, SECTOR_WORDS, WORLD_PACK_DEFAULT_LBA};
use psx_pack::{parse_entry, parse_header, PackEntry, SECTOR_BYTES};
/// Called from every cooperative checkpoint: take a CD event the interrupt
/// wrapper missed (see `cd_stream::recover_lost_edge`).
#[inline]
pub fn stream_checkpoint() {
    cd_stream::recover_lost_edge();
}
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/scene_manifest.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/scene_coverage_manifest.rs"
));
pub const SCENE_COUNT: usize = SCENE_MANIFEST.len();
pub const ROOM_CAPACITY: usize = SCENE_ARENA_BYTES;
// The index owns exactly the generated pack described by the manifests.  This
// used to be an arbitrary 256-entry ceiling, which would reject a legitimate
// whole-world pack before any scene could be admitted.  Deriving both tables
// from the generated inventories keeps the wire/index widths honest as the
// catalog grows while preserving bounded no_std storage.
use crate::world::particles::break_effects::EFFECT_ART_MANIFEST;
/// The ADPCM banks streamed from the disc: SFX, Geo and Runner at bootstrap, in
/// upload order, then the world one-shots, which `prepare_world_sfx` reads
/// before the title screen because the title menu plays two of them. That one
/// sits first on the disc, straight after the pack header `init` has just read,
/// so the read before the title seeks nowhere.
/// Each entry is the chunk's exact byte length and its FNV-1a checksum, taken
/// from the same generated manifest the owning module reads. Linking these
/// banks into `.data` cost 109,536 bytes of RAM for samples the SPU owns after
/// one upload; they are staged in the scene arena instead, like the ambience
/// clips and the Focus bank, and nothing stays resident.
///
/// The fifth is not audio: the quick map's room art (`game_map`), read right
/// after the world bank and uploaded to its own VRAM page, which the title
/// art does not touch. Linked, it cost 25 KB of RAM for texels VRAM owns.
const AUDIO_BANKS: [(usize, u32); 6] = [
    (crate::audio::BANK_BYTES, crate::audio::BANK_CHECKSUM),
    (
        crate::geo_audio::BANK_BYTES,
        crate::geo_audio::BANK_CHECKSUM,
    ),
    (
        crate::runner_audio::BANK_BYTES,
        crate::runner_audio::BANK_CHECKSUM,
    ),
    (
        crate::audio::WORLD_BANK_BYTES,
        crate::audio::WORLD_BANK_CHECKSUM,
    ),
    (crate::game_map::BANK_BYTES, crate::game_map::BANK_CHECKSUM),
    (crate::runner_audio::MOSSMAN_BANK_BYTES,crate::runner_audio::MOSSMAN_BANK_CHECKSUM),
];
/// `AUDIO_BANKS` index of the world one-shots, and of the map art.
const WORLD_BANK: usize = 3;
const MAP_BANK: usize = 4;
const MOSSMAN_BANK:usize=5;
// Chunk `i` carries id `i+1`, numbered by kind in these runs. Naming each
// boundary once keeps `chunk_spec`, `group_of` and the per-chunk call sites
// below reading the same arithmetic instead of three copies of it. Where a
// chunk sits on the disc is a separate question, answered by `member`.
const CLIP_COUNT: usize = crate::ambience::CLIPS.len();
const ATLAS_INDEX: usize = SCENE_COUNT + CLIP_COUNT;
const FOCUS_INDEX: usize = ATLAS_INDEX + ATLASES.len();
const COVERAGE_INDEX: usize = FOCUS_INDEX + 1;
const METADATA_INDEX: usize = COVERAGE_INDEX + COVERAGE_MANIFEST.len();
const EFFECT_INDEX: usize = METADATA_INDEX + WORLD_META_MANIFEST.len();
const BANK_INDEX: usize = EFFECT_INDEX + EFFECT_ART_MANIFEST.len();
/// Area music premixes, streamed a chunk at a time and never staged whole.
const MUSIC_INDEX: usize = BANK_INDEX + AUDIO_BANKS.len();
use crate::music::MUSIC_TRACKS;
/// Per-scene one-shot banks (scene_sfx.rs), by manifest scene: the first chunk
/// of each scene's disc group. Numbered last so no older chunk id moved.
const SCENE_SFX_INDEX: usize = MUSIC_INDEX + MUSIC_TRACKS.len();
/// The title art (host/cook_menu.py), read into the arena before the title and
/// uploaded to VRAM; numbered last so no older chunk id moved.
const MENU_INDEX: usize = SCENE_SFX_INDEX + SCENE_COUNT;
/// Code chunks (modules.rs), one per scene whose actors need a code module,
/// numbered after the title art. Each heads its scene's disc group, so the
/// prefetch of that group brings the room's code first.
const CODE_INDEX: usize = MENU_INDEX + 1;
const PACK_CHUNKS: usize = CODE_INDEX + crate::modules::PACKAGE_CHUNKS;
/// The payloads sit on the disc in the order they are read, not by kind.
/// Group 0 is boot's, in its read order: the world bank, the title's code and the title art
/// (all before the title), then bootstrap's SFX bank, Focus bank, Geo and Runner banks; then the ambience clips, which a scene gate reads
/// when its area first needs them (see `prepare_scene_ambience`). Group `1+s` is
/// everything a scene gate reads for manifest scene `s`, in the order
/// `admit_scenes` reads it: the scene's one-shot bank, coverage, effect art,
/// atlases, the scene, its world metadata. Filed by kind, one gate load was five long seeks across
/// the pack (about 0.3 s each in the emulator); grouped, every read after a
/// load's first starts on the sector the previous one ended. host/build_guest.py
/// writes the same order into the pack's directory. The last group is the area
/// music premixes, which only the refill reads, a chunk at a time. The world
/// one-shot bank heads group 0: adding it moved every later chunk by the same
/// sectors, so the distance of every seek a gate load makes is unchanged.
const GROUPS: usize = 3 + SCENE_COUNT;
const MUSIC_GROUP: usize = GROUPS - 2;
/// The carried data packages (modules::carry), each a chunk of its own.
const DATA_GROUP: usize = GROUPS - 1;
/// The title's own code chunk (modules.rs `BOOT_CODE_CHUNKS`, 0 or 1): the third member of group 0, read
/// right before the title art it heads.
const TITLE_CHUNKS: usize = crate::modules::BOOT_CODE_CHUNKS;
/// Package chunk index of the first carried data package: after the room code and art chunks and the title's.
const DATA_START: usize = crate::modules::CODE_CHUNKS + crate::modules::ART_CHUNKS + TITLE_CHUNKS;
const BOOT_CHUNKS: usize = AUDIO_BANKS.len() + CLIP_COUNT + 2 + TITLE_CHUNKS;
const fn scene_atlases(scene: usize) -> usize {
    (SCENE_ATLAS_RANGES[scene].1 - SCENE_ATLAS_RANGES[scene].0) as usize
}
/// 1 when manifest scene `scene`'s group starts with a code chunk.
const fn code_slots(scene: usize) -> usize {
    crate::modules::code_chunk(scene).is_some() as usize
}
/// 1 when manifest scene `scene`'s group ends with an art chunk.
const fn art_slots(scene: usize) -> usize {
    crate::modules::art_chunk(scene).is_some() as usize
}
const fn group_len(group: usize) -> usize {
    if group == 0 {
        BOOT_CHUNKS
    } else if group == MUSIC_GROUP {
        MUSIC_TRACKS.len()
    } else if group == DATA_GROUP {
        crate::modules::DATA_CHUNKS
    } else {
        5 + scene_atlases(group - 1) + code_slots(group - 1) + art_slots(group - 1)
    }
}
/// The members a gate stages: the whole group but its art chunk, which is
/// read on its own into the pool (modules.rs) and never by a gate.
const fn staged_len(group: usize) -> usize {
    group_len(group) - art_slots(group - 1)
}
/// Chunk index of the `j`th payload of `group`, in disc order.
const fn member(group: usize, j: usize) -> usize {
    if group == 0 {
        if TITLE_CHUNKS == 1 && j == 2 {
            return CODE_INDEX + crate::modules::CODE_CHUNKS + crate::modules::ART_CHUNKS;
        }
        // The members after the title's chunk are numbered as they were without it.
        let j = if j > 2 { j - TITLE_CHUNKS } else { j };
        return match j {
            0 => BANK_INDEX + WORLD_BANK,
            1 => BANK_INDEX + MAP_BANK,
            2 => MENU_INDEX,
            3 => BANK_INDEX,
            4 => FOCUS_INDEX,
            5 | 6 => BANK_INDEX + j - 4,
            7 => BANK_INDEX + MOSSMAN_BANK,
            _ => SCENE_COUNT + j - 8,
        };
    }
    if group == MUSIC_GROUP {
        return MUSIC_INDEX + j;
    }
    if group == DATA_GROUP {
        return CODE_INDEX + DATA_START + j;
    }
    let scene = group - 1;
    let atlases = scene_atlases(scene);
    if let Some(k) = crate::modules::code_chunk(scene) {
        if j == 0 {
            return CODE_INDEX + k;
        }
    }
    if let Some(k) = crate::modules::art_chunk(scene) {
        if j == group_len(group) - 1 {
            return CODE_INDEX + k;
        }
    }
    let j = j - code_slots(scene);
    if j == 0 {
        SCENE_SFX_INDEX + scene
    } else if j == 1 {
        COVERAGE_INDEX + scene
    } else if j == 2 {
        EFFECT_INDEX + SCENE_MANIFEST[scene].scene_id as usize
    } else if j < 3 + atlases {
        ATLAS_INDEX + SCENE_ATLAS_RANGES[scene].0 as usize + j - 3
    } else if j == 3 + atlases {
        scene
    } else {
        METADATA_INDEX + scene
    }
}
/// The inverse of `member`: which group a chunk index is filed in, and where.
fn group_of(index: usize) -> Option<(usize, usize)> {
    let (group, j) = if index < SCENE_COUNT {
        (1 + index, 3 + scene_atlases(index))
    } else if index < ATLAS_INDEX {
        (0, 8 + index - SCENE_COUNT)
    } else if index < FOCUS_INDEX {
        let s = ATLASES[index - ATLAS_INDEX].scene_index;
        (1 + s, 3 + index - ATLAS_INDEX - SCENE_ATLAS_RANGES[s].0 as usize)
    } else if index < COVERAGE_INDEX {
        (0, 4)
    } else if index < METADATA_INDEX {
        (1 + index - COVERAGE_INDEX, 1)
    } else if index < EFFECT_INDEX {
        let s = index - METADATA_INDEX;
        (1 + s, 4 + scene_atlases(s))
    } else if index < BANK_INDEX {
        // Effect art is filed by scene id, the groups by manifest index.
        let id = index - EFFECT_INDEX;
        (
            1 + SCENE_MANIFEST
                .iter()
                .position(|d| d.scene_id as usize == id)?,
            2,
        )
    } else if index == BANK_INDEX + WORLD_BANK {
        (0, 0)
    } else if index == BANK_INDEX + MAP_BANK {
        (0, 1)
    } else if index == BANK_INDEX+MOSSMAN_BANK {
        (0,7)
    } else if index < MUSIC_INDEX {
        (
            0,
            if index == BANK_INDEX {
                3
            } else {
                4 + index - BANK_INDEX
            },
        )
    } else if index < SCENE_SFX_INDEX {
        (MUSIC_GROUP, index - MUSIC_INDEX)
    } else if index < MENU_INDEX {
        (1 + index - SCENE_SFX_INDEX, 0)
    } else if index == MENU_INDEX {
        (0, 2)
    } else if index < PACK_CHUNKS {
        let k = index - CODE_INDEX;
        let title = crate::modules::CODE_CHUNKS + crate::modules::ART_CHUNKS;
        if TITLE_CHUNKS == 1 && k == title {
            return Some((0, 2));
        }
        if k >= DATA_START {
            return Some((DATA_GROUP, k - DATA_START));
        }
        let g = 1 + crate::modules::CHUNK_SCENE[k];
        return Some(if k < crate::modules::CODE_CHUNKS {
            (g, 0)
        } else {
            (g, group_len(g) - 1)
        });
    } else {
        return None;
    };
    // A scene group's code chunk, if it has one, comes first; group 0's title chunk is its third member.
    Some(if group > 0 && group < MUSIC_GROUP {
        (group, j + code_slots(group - 1))
    } else if group == 0 && j >= 2 {
        (group, j + TITLE_CHUNKS)
    } else {
        (group, j)
    })
}
/// Static pages manifest scene `index` uploads.
fn scene_pages(index: usize) -> usize {
    let (start, end) = SCENE_ATLAS_RANGES[index];
    let (start, end) = (start as usize, end as usize);
    ATLASES[start..end]
        .iter()
        .filter(|a| a.kind == 0)
        .map(|a| a.count)
        .sum()
}
/// Times the quick map's page was read back after a nineteen-page scene.
#[no_mangle]
pub static mut HK_MAP_RELOADS: u32 = 0;
const PACK_HEADER_SECTORS: usize = (28 + PACK_CHUNKS * 24).div_ceil(SECTOR_BYTES);
// The directory is staged in the scene arena, so a catalogue whose directory
// outgrew the arena must fail the build, not the boot.
const _: () = assert!(PACK_HEADER_SECTORS * SECTOR_BYTES <= SCENE_ARENA_BYTES);
// A gate names its destination as one u32 of its scene's world metadata bank,
// with the catalogue slot in the low sixteen bits and the target scene above
// it (`crate::world::gates`). A catalogue longer than that field can address
// would hand out slots a gate cannot name, and the wrap would be silent, so it
// fails the build here as well as at the cook. host/world.py MAX_REGIONS is the
// host half of the same ceiling and refuses first; this is the guest's own.
const _: () = assert!(REGION_SCENE_LOCAL.len() <= 1 << 16);
// The owner half of a REGION_SCENE_LOCAL slot is a u8, like its other half, the
// scene-local room index (which caps one scene at 256 regions); host/pack_scenes.py
// refuses both overflows where the mapping is built.
const _: () = assert!(SCENE_COUNT <= 1 << 8);
const EMPTY: usize = usize::MAX;
const SEEK_POLL: u32 = 4_000_000;
/// Byte operations one decoder step may run between pad checkpoints during a load. 4096
/// took about a VBlank on a dense room (the False Knight journey's first load), which
/// carried a poll over one; a quarter of that leaves several opportunities per frame.
const DECODE_STEP: usize = match option_env!("HK_DECODE_STEP") {
    Some(s) => parse_usize(s),
    None => 1024,
};
/// Bytes past the scene arena that only a scene group staged in the tail may
/// use (see `stage_group`). Zero keeps today's RAM; a prefetch that does not
/// fit the resident scene's slack can be given more here.
const PREFETCH_EXTRA: usize = match option_env!("HK_PREFETCH_EXTRA") {
    Some(s) => parse_usize(s),
    None => 0,
};
const fn parse_usize(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    let mut v = 0;
    while i < b.len() {
        v = v * 10 + (b[i] - b'0') as usize;
        i += 1;
    }
    v
}
/// The arena a scene group is staged in: the scene arena plus that extra.
const ARENA_TOTAL: usize = (SCENE_ARENA_BYTES + PREFETCH_EXTRA) & !(SECTOR_BYTES - 1);
const _: () = assert!(ARENA_TOTAL >= SCENE_ARENA_BYTES & !(SECTOR_BYTES - 1));
const BUFFER_BYTES: usize = if ARENA_TOTAL > SCENE_ARENA_BYTES {
    ARENA_TOTAL
} else {
    SCENE_ARENA_BYTES
};
#[repr(C, align(4))]
struct Buffer {
    words: [u32; BUFFER_BYTES / 4],
}
// Preserve the linked symbol so build memory accounting measures actual bytes.
static mut BUFFERS: Buffer = Buffer {
    words: [0; BUFFER_BYTES / 4],
};
/// The scene arena's length, for the compile-time check that the opening movies' player fits it (intro.rs).
pub const ARENA_BYTES: usize = BUFFER_BYTES;
/// The scene arena's address and length. The opening movies (intro.rs) buffer their frames in it: the title
/// is over and nothing in it is needed before the first scene is loaded.
pub fn arena() -> (usize, usize) {
    ((&raw mut BUFFERS) as usize, BUFFER_BYTES)
}
/// Where an admission's chunks come from.
#[derive(Clone, Copy)]
enum Stage {
    /// Each chunk read on its own to the front of its slice (the path a
    /// group too large for the arena still takes).
    Read,
    /// The whole scene group sits at `base`, the arena tail, in disc order:
    /// sectors `..pre` were prefetched, `pre..` arrive through one read
    /// started at the gate, and each chunk decodes in place once its
    /// sectors have landed. The gate read runs while the prefetched chunks,
    /// which come first, decode.
    Tail { base: usize, pre: usize },
    /// A group staged in two windows (`Cache::split_fits`): the first `a`
    /// sectors as `Tail` stages a whole group, then, once the scene chunk is
    /// asked for, the rest read into the tail over the spent first window
    /// (`Cache::split_b` says where, once that read has started).
    Split { base: usize, pre: usize, a: usize },
}
#[repr(C, align(4))]
struct CoverageBuffer {
    words: [u32; COVERAGE_ARENA_BYTES / 4],
}
static mut COVERAGE_BUFFER: CoverageBuffer = CoverageBuffer {
    words: [0; COVERAGE_ARENA_BYTES / 4],
};
#[no_mangle]
pub static mut HK_COVERAGE_LOADS: u32 = 0;
#[no_mangle]
pub static mut HK_COVERAGE_BYTES: u32 = 0;
#[no_mangle]
pub static mut HK_COVERAGE_SCENE: u32 = 0;
fn atlas_fingerprint(scene: usize) -> u32 {
    let (start, end) = SCENE_ATLAS_RANGES[scene];
    let (start, end) = (start as usize, end as usize);
    let mut hash = 2166136261u32;
    for a in &ATLASES[start..end] {
        for value in [
            a.kind as u32,
            a.first as u32,
            a.count as u32,
            a.raw_len as u32,
            a.raw_fnv,
        ] {
            for byte in value.to_le_bytes() {
                hash = (hash ^ byte as u32).wrapping_mul(16777619);
            }
        }
    }
    hash
}
pub const fn scene_index(region: usize) -> usize {
    REGION_SCENE_LOCAL[region].0 as usize
}
/// The manifest index of catalogue scene `scene_id`, if the disc carries it.
pub fn manifest_index(scene_id: usize) -> Option<usize> {
    SCENE_MANIFEST
        .iter()
        .position(|d| d.scene_id as usize == scene_id)
}
/// Where each scene's static pages and palettes sit beyond the manifest's bases
/// (spike `vram-ahead`: a scene admitted ahead of its gate takes the unused end
/// of the page and palette ranges, so the scene it follows stays intact).
static mut SHIFT_PAGE: [u8; SCENE_COUNT] = [0; SCENE_COUNT];
static mut SHIFT_PAL: [u16; SCENE_COUNT] = [0; SCENE_COUNT];
pub fn scene_page_base(scene: usize) -> usize {
    SCENE_MANIFEST[scene].page_base + if AHEAD_ON { unsafe { SHIFT_PAGE[scene] as usize } } else { 0 }
}
pub fn scene_palette_base(scene: usize) -> usize {
    SCENE_MANIFEST[scene].palette_base + if AHEAD_ON { unsafe { SHIFT_PAL[scene] as usize } } else { 0 }
}
/// Per-texture attributes of the resident scene bank (packer-derived flags
/// and cores), read through the admitted Scene view.
pub fn scene_texture_flags(scene: usize, texture: usize) -> u8 {
    resident_scene(scene).texture_flags(texture)
}
pub fn scene_black_core(scene: usize, texture: usize) -> [u8; 4] {
    resident_scene(scene).black_core(texture)
}
pub fn scene_opaque_core(scene: usize, texture: usize) -> [u8; 4] {
    resident_scene(scene).opaque_core(texture)
}
fn resident_scene(scene: usize) -> Scene<'static> {
    let cache = unsafe { &*(&raw const crate::ROOM_CACHE) };
    cache.scene(scene)
}
pub const fn scene_region(scene: usize) -> usize {
    SCENE_REGIONS[scene] as usize
}
/// Chunk-table index of one bootstrap audio bank, after the effect art chunks.
/// The host packer appends them in the same order, so existing chunk ids are
/// unchanged.
const fn audio_bank_index(bank: usize) -> usize {
    BANK_INDEX + bank
}
/// Stored byte length, payload checksum and staging capacity of one pack chunk,
/// by chunk index. Every field of that chunk's directory entry except its
/// sector offset follows from this triple and the index itself, which is why
/// `init` can check the whole directory against the generated manifests and
/// then let go of it.
fn chunk_spec(index: usize) -> Option<(usize, u32, usize)> {
    Some(if index < SCENE_COUNT {
        let d = &SCENE_MANIFEST[index];
        (d.stored_len, d.stored_fnv, d.arena_capacity)
    } else if index < ATLAS_INDEX {
        let clip = &crate::ambience::CLIPS[index - SCENE_COUNT];
        (clip.byte_len, clip.checksum, SCENE_ARENA_BYTES)
    } else if index < FOCUS_INDEX {
        let a = &ATLASES[index - ATLAS_INDEX];
        (a.stored_len, a.stored_fnv, SCENE_ARENA_BYTES)
    } else if index < COVERAGE_INDEX {
        (
            crate::focus_audio::BANK_BYTES,
            crate::focus_audio::BANK_CHECKSUM,
            SCENE_ARENA_BYTES,
        )
    } else if index < METADATA_INDEX {
        let c = &COVERAGE_MANIFEST[index - COVERAGE_INDEX];
        (c.stored_len, c.stored_fnv, SCENE_ARENA_BYTES)
    } else if index < EFFECT_INDEX {
        let m = &WORLD_META_MANIFEST[index - METADATA_INDEX];
        (m.stored_len, m.stored_fnv, SCENE_ARENA_BYTES)
    } else if index < BANK_INDEX {
        let f = &EFFECT_ART_MANIFEST[index - EFFECT_INDEX];
        (f.stored_len, f.stored_fnv, SCENE_ARENA_BYTES)
    } else if index < MUSIC_INDEX {
        let (size, checksum) = AUDIO_BANKS[index - BANK_INDEX];
        (size, checksum, SCENE_ARENA_BYTES)
    }
    // Streamed into the music FIFO a chunk at a time, never staged whole.
    else if index < SCENE_SFX_INDEX {
        let t = &MUSIC_TRACKS[index - MUSIC_INDEX];
        (t.byte_len, t.checksum, usize::MAX)
    } else if index < MENU_INDEX {
        let (len, fnv) =
            crate::scene_sfx::chunk(SCENE_MANIFEST[index - SCENE_SFX_INDEX].scene_id as usize);
        (len, fnv, SCENE_ARENA_BYTES)
    } else if index == MENU_INDEX {
        // The chunk holds the nine other title styles after the part the boot reads (`menu_art`), so
        // it is larger than the arena; only the common part and one style are ever staged at once.
        (
            crate::boot_art::BOOT_ART_BYTES,
            crate::boot_art::BOOT_ART_CHECKSUM,
            usize::MAX,
        )
    } else if index < PACK_CHUNKS {
        let (len, fnv) = crate::modules::disc_spec(index - CODE_INDEX);
        (len, fnv, SCENE_ARENA_BYTES)
    } else {
        return None;
    })
}
/// FNV-1a over `bytes`, servicing the pad between blocks (the title's art chunks are 100 KB and more).
/// Whether the opening movies' display buffers (`intro::MOVIE_BUFFERS`, 320 by 240 halfwords each, which the
/// player clears and fills) cover any VRAM that scene `scene`'s atlases are uploaded to. The first scene's
/// atlases go up before the movies, so main.rs asserts at build time that none is lost.
#[cfg(feature = "intro")]
pub const fn scene_clobbered(scene: usize) -> bool {
    let (mut i, end) = (SCENE_ATLAS_RANGES[scene].0 as usize, SCENE_ATLAS_RANGES[scene].1 as usize);
    while i < end {
        let a = &ATLASES[i];
        let mut k = a.first;
        while k < a.first + a.count {
            // The page and palette layouts of hk_cache::residency (`page_xy`, `clut_xy`), as consts.
            let (x, y, w, h) = if a.kind == 0 {
                (384 + (k % 10) * 64, (k / 10) * 256, 64, 256)
            } else {
                let (bank, p) = (k / hk_cache::residency::CLUTS, k % hk_cache::residency::CLUTS);
                match bank {
                    0 if p < 400 => ((p % 20) * 16, 480 + p / 20, 16, 1),
                    0 => (336, 480 + (p - 400), 16, 1),
                    1 => (320 + (p % 4) * 16, 256 + p / 4, 16, 1),
                    2 => (320 + (p % 4) * 16, 360 + p / 4, 16, 1),
                    _ => (352 + (p % 2) * 16, 32 + p / 2, 16, 1),
                }
            };
            let mut b = 0;
            while b < crate::intro::MOVIE_BUFFERS.len() {
                let (bx, by) = (
                    crate::intro::MOVIE_BUFFERS[b].0 as usize,
                    crate::intro::MOVIE_BUFFERS[b].1 as usize,
                );
                if x < bx + 320 && bx < x + w && y < by + 240 && by < y + h {
                    return true;
                }
                b += 1;
            }
            k += 1;
        }
        i += 1;
    }
    false
}
fn fnv(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for block in bytes.chunks(1024) {
        crate::input::checkpoint();
        for &byte in block {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
        }
    }
    hash
}
/// A title style other than the default (host/build_guest.py `boot_art`), or `None` when the disc does not
/// give it. The options page calls this with the title's song stopped.
pub fn style_art(style: usize) -> Option<&'static [u8]> {
    let cache = unsafe { &mut *(&raw mut crate::ROOM_CACHE) };
    // SAFETY of the lifetime: the bytes live in the scene arena, which nothing else touches while the title is up.
    cache.style_art(style).ok().map(|b| unsafe { core::slice::from_raw_parts(b.as_ptr(), b.len()) })
}
pub struct Cache {
    /// Sector offset of the first chunk of each disc group (see `member`), taken
    /// from the directory `init` validated. Together with `chunk_spec` this is
    /// the whole directory: `entry` rebuilds any of its entries on demand, so
    /// the 20-byte-per-chunk table and the sectors it was read from do not stay
    /// resident. At 47 scenes that table and its header were 21,888 bytes of
    /// `.data`; at the full catalogue they would be about 221,000.
    group_bases: [u32; GROUPS],
    reader: SectorReader,
    ready: bool,
    loaded_scenes: usize,
    loaded_atlases: usize,
    selected: usize,
    resident_scene: usize,
    atlas_scene: usize,
    coverage_scene: usize,
    coverage_len: usize,
    metadata_scene: usize,
    metadata_len: usize,
    /// Where a `Stage::Split` admission's second window was read to.
    split_b: Option<usize>,
    /// The admitted bank's view, taken once when admission completes and
    /// dropped with it. Rebuilding it re-read the header on every lookup.
    metadata_view: Option<WorldMeta<'static>>,
    /// Manifest index of the scene whose effect texels sit in the effect VRAM rects.
    effect_scene: usize,
    /// The scene whose sound effects, coverage, effect art and atlases `preload` put in place before the
    /// opening movies took the arena, or `PRELOAD_NONE`. The scene's admission consumes it.
    #[cfg(feature = "intro")]
    preloaded: u16,
    /// `preload` is running the admission: it stops after the atlases and leaves the code to the real one.
    #[cfg(feature = "intro")]
    early: bool,
}
#[cfg(feature = "intro")]
const PRELOAD_NONE: u16 = u16::MAX;
#[no_mangle]
pub static mut HK_SCENE_LOADS: u32 = 0;
#[no_mangle]
pub static mut HK_SCENE_READ_ID: u32 = 0;
#[no_mangle]
pub static mut HK_SCENE_DECODE_ID: u32 = 0;
#[no_mangle]
pub static mut HK_REGION_ACTIVATIONS: u32 = 0;
#[no_mangle]
pub static mut HK_ROOM_STORED_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_ROOM_STORED_COMPLETIONS: u32 = 0;
#[no_mangle]
pub static mut HK_ROOM_DECODE_PHASE: u32 = 11;
#[no_mangle]
pub static mut HK_ROOM_READ_REGION: u32 = 0;
#[no_mangle]
pub static mut HK_ROOM_DECODE_REGION: u32 = 0;
#[no_mangle]
pub static mut HK_CD_SECTORS_READ: u32 = 0;
/// Sectors the scene loader asked for itself: the directory, chunk reads and
/// whole-group reads at admissions. HK_CD_SECTORS_READ also counts the
/// background transfers gameplay now runs on purpose (area music refills, the
/// room and clip prefetches); this one must not move while a scene plays.
#[no_mangle]
pub static mut HK_SCENE_SECTORS_READ: u32 = 0;
fn scene_sectors(count: usize) {
    unsafe {
        HK_SCENE_SECTORS_READ = HK_SCENE_SECTORS_READ.saturating_add(count as u32);
    }
}
#[no_mangle]
pub static mut HK_ROOM_BYTES: u32 = 0;
#[no_mangle]
pub static mut HK_ROOM_LOAD_STATE: u32 = 0;
#[no_mangle]
pub static mut HK_ROOM_LOAD_ERROR: u32 = 0;
#[no_mangle]
pub static mut HK_CD_DIAG: u32 = 0;
#[no_mangle]
pub static mut HK_ROOM_CACHE_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_REGION_LOADS: u32 = 0;
#[derive(Copy, Clone, Debug)]
#[repr(u32)]
pub enum LoadError {
    Prepare = 1,
    HeaderRead = 2,
    HeaderInvalid = 3,
    RoomMismatch = 4,
    PayloadRead = 5,
    Checksum = 6,
    RoomFormat = 7,
    Decompress = 8,
    Ambience = 9,
    Coverage = 10,
    WorldMeta = 11,
    EffectArt = 12,
}

/// Check a staged HKWMTA01 bank against the scene manifest before gameplay
/// receives any world object view.  The caller owns the backing bytes; this
/// function only returns a borrow after both the wire format and the
/// scene/raw-bank/fingerprint identities match.
pub fn checked_world_metadata<'a>(
    bytes: &'a [u8],
    scene: usize,
) -> Result<WorldMeta<'a>, LoadError> {
    let expected = WORLD_META_MANIFEST.get(scene).ok_or(LoadError::WorldMeta)?;
    // The checked parse grows with the scene's objects; poll between regions.
    let bank =
        WorldMeta::parse_with(bytes, crate::input::checkpoint).map_err(|_| LoadError::WorldMeta)?;
    if bank
        .check_identity(expected.scene_id, expected.raw_fnv, &expected.fingerprint)
        .is_err()
    {
        return Err(LoadError::WorldMeta);
    }
    Ok(bank)
}
fn mark(state: u32, error: u32) {
    unsafe {
        core::ptr::write_volatile(&raw mut HK_ROOM_LOAD_STATE, state);
        core::ptr::write_volatile(&raw mut HK_ROOM_LOAD_ERROR, error);
    }
}
/// The loader's cooperative checkpoint: the input and music services and, while the opening poem is on
/// screen during the first scene's early load, the poem's next frame (intro.rs `tick`). Only the admission
/// calls it: nothing a scratchpad stack entry reaches may, because the poem's frame is drawn by the opening
/// program through a pointer, which the stack guard cannot bound. A call, not inline code, in the build that
/// has the poem: the admission has a few dozen of them.
#[cfg_attr(feature = "intro", inline(never))]
#[cfg_attr(feature = "intro", optimize(size))]
#[cfg_attr(not(feature = "intro"), inline(always))]
fn checkpoint() {
    crate::input::checkpoint();
    #[cfg(feature = "intro")]
    crate::intro::tick();
}
/// The pad poll a seek or a sector wait takes between looks (a named function so the
/// `dyn FnMut` shims stay two jumps).
#[inline(never)]
fn pad_idle() {
    crate::input::poll_only();
}
#[inline(never)]
fn sector(reader: &mut SectorReader, dst: &mut [u32]) -> bool {
    assert!(dst.len() == SECTOR_WORDS);
    for word in dst.iter_mut() {
        unsafe { core::ptr::write_volatile(word, 0) };
    }
    // The next sector can be several frames away (the head still settling, the
    // drive at its own pace), so wait for it a look at a time and take the pad
    // poll that comes due between looks; `read_sector` spins without one. The
    // bound is the reader's own (`DATA_POLL`, a worst-case seek).
    let mut looks = 0u32;
    loop {
        match unsafe { reader.try_read_sector(dst.try_into().unwrap()) } {
            Ok(true) => break,
            Ok(false) if looks < SEEK_POLL => {
                looks += 1;
                crate::input::poll_only();
            }
            _ => return false,
        }
    }
    unsafe { HK_CD_SECTORS_READ = HK_CD_SECTORS_READ.saturating_add(1) };
    scene_sectors(1);
    true
}
/// Read directory sector `index` into the scene arena. The arena is the port's
/// staging area for bytes that are checked and then dropped, the same deal the
/// ADPCM banks and the per-scene effect art make; `init` is the only caller and
/// refuses to run while a scene owns the arena.
fn header_sector(reader: &mut SectorReader, index: usize) -> bool {
    if (index + 1) * SECTOR_BYTES > SCENE_ARENA_BYTES {
        return false;
    }
    let words = unsafe {
        core::slice::from_raw_parts_mut(
            (&raw mut BUFFERS.words)
                .cast::<u32>()
                .add(index * SECTOR_WORDS),
            SECTOR_WORDS,
        )
    };
    sector(reader, words)
}
impl Cache {
    /// Safety: exactly one Cache owns BUFFERS and the CD controller.
    pub const unsafe fn new() -> Self {
        Self {
            group_bases: [0; GROUPS],
            reader: SectorReader::new(),
            ready: false,
            loaded_scenes: 0,
            loaded_atlases: 0,
            selected: EMPTY,
            resident_scene: EMPTY,
            atlas_scene: EMPTY,
            coverage_scene: EMPTY,
            coverage_len: 0,
            metadata_scene: EMPTY,
            metadata_len: 0,
            split_b: None,
            metadata_view: None,
            effect_scene: EMPTY,
            #[cfg(feature = "intro")]
            preloaded: PRELOAD_NONE,
            #[cfg(feature = "intro")]
            early: false,
        }
    }
    #[cfg_attr(not(test), optimize(size))]
    fn init(&mut self) -> Result<(), LoadError> {
        if self.ready {
            return Ok(());
        }
        if !SCENE_GATE_LOAD
            || COVERAGE_MANIFEST.len() != SCENE_COUNT
            || WORLD_META_MANIFEST.len() != SCENE_COUNT
            || COVERAGE_ARENA_BYTES % 4 != 0
            || WORLD_META_ARENA_BYTES % 4 != 0
            || WORLD_META_ARENA_BYTES == 0
            || COVERAGE_GRID_SHIFT != 2
            || COVERAGE_GROUP_POOL_CAPACITY > 4096
        {
            return Err(LoadError::Coverage);
        }
        if EFFECT_ART_MANIFEST.len() != SCENE_COUNT
            || EFFECT_ART_MANIFEST.iter().any(|f| {
                f.raw_len == 0
                    || f.raw_len % 4 != 0
                    || f.raw_len > SCENE_ARENA_BYTES
                    || f.stored_len == 0
                    || f.stored_len.div_ceil(SECTOR_BYTES) > SCENE_ARENA_BYTES / SECTOR_BYTES
            })
        {
            return Err(LoadError::RoomMismatch);
        }
        if SCENE_COUNT == 0 || SCENE_ARENA_BYTES % 4 != 0 || SCENE_REGIONS.len() != SCENE_COUNT {
            return Err(LoadError::RoomMismatch);
        }
        let mut offset = 0usize;
        for (i, d) in SCENE_MANIFEST.iter().enumerate() {
            if SCENE_GATE_LOAD {
                offset = 0;
            }
            if d.ram_offset != offset
                || d.arena_capacity != SCENE_ARENA_BYTES.saturating_sub(offset)
                || d.raw_len == 0
                || d.raw_len > d.arena_capacity
                || d.stored_len == 0
                || d.stored_len.div_ceil(SECTOR_BYTES) > d.arena_capacity / SECTOR_BYTES
                || SCENE_REGIONS[i] as usize >= REGION_SCENE_LOCAL.len()
                || scene_index(SCENE_REGIONS[i] as usize) != i
            {
                return Err(LoadError::RoomMismatch);
            }
            offset = offset
                .checked_add(d.raw_len)
                .and_then(|n| n.checked_add(3))
                .ok_or(LoadError::RoomMismatch)?
                & !3;
        }
        if offset > SCENE_ARENA_BYTES
            || REGION_SCENE_LOCAL
                .iter()
                .any(|&(s, _)| s as usize >= SCENE_COUNT)
        {
            return Err(LoadError::RoomMismatch);
        }
        // Each scene owns an exact contiguous atlas inventory. Exclusive
        // scene plans overlap physical slots only across different owners.
        if SCENE_ATLAS_RANGES.len() != SCENE_COUNT {
            return Err(LoadError::RoomMismatch);
        }
        let (mut pages, mut palettes, mut previous, mut peak_pages, mut peak_palettes) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        for (scene, &(start, end)) in SCENE_ATLAS_RANGES.iter().enumerate() {
            let (start, end) = (start as usize, end as usize);
            if start != previous || end <= start || end > ATLASES.len() {
                return Err(LoadError::RoomMismatch);
            }
            if SCENE_GATE_LOAD {
                pages = 0;
                palettes = 0;
            }
            let d = SCENE_MANIFEST[scene];
            if d.page_base != pages || d.palette_base != palettes {
                return Err(LoadError::RoomMismatch);
            }
            for a in &ATLASES[start..end] {
                let (next, unit) = match a.kind {
                    0 => (&mut pages, 32768usize),
                    1 => (&mut palettes, 32usize),
                    _ => return Err(LoadError::RoomMismatch),
                };
                if a.scene_index != scene
                    || a.count == 0
                    || a.first != *next
                    || a.count.checked_mul(unit) != Some(a.raw_len)
                    || a.raw_len > SCENE_ARENA_BYTES
                    || a.stored_len == 0
                    || a.stored_len.div_ceil(SECTOR_BYTES) > SCENE_ARENA_BYTES / SECTOR_BYTES
                {
                    return Err(LoadError::RoomMismatch);
                }
                *next = next.checked_add(a.count).ok_or(LoadError::RoomMismatch)?;
            }
            if pages > hk_cache::residency::STATIC_PAGES
                || palettes > hk_cache::residency::SCENE_CLUTS
            {
                return Err(LoadError::RoomMismatch);
            }
            peak_pages = peak_pages.max(pages);
            peak_palettes = peak_palettes.max(palettes);
            previous = end;
        }
        if previous != ATLASES.len()
            || peak_pages != SCENE_TOTAL_PAGES
            || peak_palettes != SCENE_TOTAL_PALETTES
        {
            return Err(LoadError::RoomMismatch);
        }
        for (scene, c) in COVERAGE_MANIFEST.iter().enumerate() {
            let d = SCENE_MANIFEST[scene];
            if c.scene_id != d.scene_id as u32
                || c.scene_raw_fnv != d.raw_fnv
                || c.atlas_fnv != atlas_fingerprint(scene)
                || c.chunk_id != COVERAGE_INDEX + scene + 1
                || c.draw_pool_count == 0
                || c.draw_pool_count as usize > COVERAGE_GROUP_POOL_CAPACITY
                || c.raw_len < 80
                || c.raw_len > COVERAGE_ARENA_BYTES
                || c.raw_len % 4 != 0
                || c.raw_len > SCENE_ARENA_BYTES
                || c.stored_len == 0
                || c.stored_len.div_ceil(SECTOR_BYTES) > SCENE_ARENA_BYTES / SECTOR_BYTES
            {
                return Err(LoadError::Coverage);
            }
        }
        for (scene, m) in WORLD_META_MANIFEST.iter().enumerate() {
            let d = SCENE_MANIFEST[scene];
            if m.scene_id != d.scene_id as u32
                || m.raw_len < 160
                || m.raw_len > WORLD_META_ARENA_BYTES
                || m.raw_len % 4 != 0
                || m.stored_len == 0
                || m.stored_len.div_ceil(SECTOR_BYTES) > SCENE_ARENA_BYTES / SECTOR_BYTES
                || m.chunk_id != METADATA_INDEX + scene + 1
            {
                return Err(LoadError::WorldMeta);
            }
        }
        if !unsafe { self.reader.prepare_single_speed() } {
            return Err(LoadError::Prepare);
        }
        // The directory is staged in the scene arena and dropped once checked,
        // like the ADPCM banks and the per-scene effect art. Refuse if anything
        // is admitted; bootstrap calls this through `prepare_ambience`, which
        // makes the same refusal, long before the first scene is read.
        if self.loaded_scenes != 0 {
            return Err(LoadError::HeaderRead);
        }
        if !unsafe {
            self.reader
                .start_read_seek_first(WORLD_PACK_DEFAULT_LBA, SEEK_POLL)
        } || !header_sector(&mut self.reader, 0)
        {
            return Err(LoadError::HeaderRead);
        }
        let h = {
            let first = unsafe {
                core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>(), SECTOR_BYTES)
            };
            parse_header(first).ok_or(LoadError::HeaderInvalid)?
        };
        if h.chunk_count as usize != PACK_CHUNKS
            || h.header_sectors as usize != PACK_HEADER_SECTORS
            || h.table_bytes as usize != PACK_CHUNKS * 24
        {
            return Err(LoadError::HeaderInvalid);
        }
        for i in 1..PACK_HEADER_SECTORS {
            if !header_sector(&mut self.reader, i) {
                return Err(LoadError::HeaderRead);
            }
        }
        unsafe {
            self.reader.stop();
        }
        // Every field of every entry is checked against the generated manifests
        // and the running sector cursor, exactly as when the parsed table was
        // kept: an entry that passes is bit-for-bit what `entry` reconstructs,
        // so dropping the table below loses no proof, only the bytes. All that
        // survives is where each disc group starts, which the cursor has. The
        // directory lists chunks in disc order and never repeats an id, so a
        // walk of `member` that matches it entry for entry and ends on its
        // last one has visited every chunk exactly once.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&raw const BUFFERS.words).cast::<u8>(),
                PACK_HEADER_SECTORS * SECTOR_BYTES,
            )
        };
        let mut next = PACK_HEADER_SECTORS;
        let mut position = 0usize;
        for group in 0..GROUPS {
            self.group_bases[group] = next as u32;
            for j in 0..group_len(group) {
                let i = member(group, j);
                let (size, checksum, capacity) = chunk_spec(i).ok_or(LoadError::HeaderInvalid)?;
                let e = parse_entry(bytes, position).ok_or(LoadError::HeaderInvalid)?;
                if size == 0
                    || size.div_ceil(SECTOR_BYTES) > capacity / SECTOR_BYTES
                    || group_of(i) != Some((group, j))
                    || e.chunk_id as usize != i + 1
                    || e.byte_size as usize != size
                    || e.checksum != checksum
                    || e.sector_offset as usize != next
                    || e.sector_count as usize != size.div_ceil(SECTOR_BYTES)
                {
                    return Err(LoadError::RoomMismatch);
                }
                next = next
                    .checked_add(e.sector_count as usize)
                    .ok_or(LoadError::HeaderInvalid)?;
                position += 1;
            }
        }
        if next != h.total_sectors as usize || position != PACK_CHUNKS {
            return Err(LoadError::HeaderInvalid);
        }
        // MUSIC.XA by name: the pack's cooked layout does not fix where the XA songs
        // sit, and a disc that moves this program keeps the name. Read through the
        // polled reader, before the CD interrupt owns the controller.
        if let Some((lba, size)) = self.find_xa() {
            crate::music::set_xa_file(lba, size);
        }
        cd_stream::install();
        self.ready = true;
        for k in 0..crate::modules::PACKAGE_CHUNKS {
            let e = self.entry(CODE_INDEX + k)?;
            unsafe {
                PACKAGE_LBA[k] = e.sector_offset;
            }
        }
        for track in 0..MUSIC_TRACKS.len() {
            let e = self.entry(MUSIC_INDEX + track)?;
            crate::music::set_track_lba(track, WORLD_PACK_DEFAULT_LBA + e.sector_offset);
        }
        for clip in 0..CLIP_COUNT {
            let e = self.entry(SCENE_COUNT + clip)?;
            crate::ambience::set_clip_lba(clip, WORLD_PACK_DEFAULT_LBA + e.sector_offset);
        }
        for (slot, bank) in [(0, 2), (1, MOSSMAN_BANK)] {
            let e = self.entry(audio_bank_index(bank))?;
            unsafe {
                BANK_LBA[slot] = WORLD_PACK_DEFAULT_LBA + e.sector_offset;
            }
        }
        unsafe {
            DRIVE.installed = true;
        }
        Ok(())
    }
    /// `(lba, size in bytes)` of MUSIC.XA in the root directory, from the primary
    /// volume descriptor and the first sector of the root directory (a handful of
    /// files). Stages both in the arena's first sector, which the pack header
    /// check above has finished with.
    #[inline(never)]
    fn find_xa(&mut self) -> Option<(u32, u32)> {
        let le32 = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let mut lba = 16;
        for step in 0..2 {
            if !unsafe { self.reader.start_read_seek_first(lba, SEEK_POLL) }
                || !header_sector(&mut self.reader, 0)
            {
                return None;
            }
            unsafe {
                self.reader.stop();
            }
            let s = unsafe {
                core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>(), SECTOR_BYTES)
            };
            if step == 0 {
                // ISO 9660: "CD001" after the type byte, the root directory record at 156.
                if &s[1..6] != b"CD001" {
                    return None;
                }
                lba = le32(&s[158..]);
            } else {
                let name = crate::music::XA_NAME.as_bytes();
                let mut at = 0;
                while at + 34 <= SECTOR_BYTES && s[at] != 0 {
                    let (len, n) = (s[at] as usize, s[at + 32] as usize);
                    if s[at + 33..].starts_with(name) && n >= name.len() {
                        return Some((le32(&s[at + 2..]), le32(&s[at + 10..])));
                    }
                    at += len;
                }
            }
        }
        None
    }
    /// Rebuild one directory entry. `init` compared this exact reconstruction
    /// against the directory the cook wrote, chunk by chunk, before `ready`
    /// was set, so the entry returned here is the checked one; the payload it
    /// points at is still checked again by its own decoder or hash on read.
    fn entry(&self, index: usize) -> Result<PackEntry, LoadError> {
        if !self.ready {
            return Err(LoadError::HeaderInvalid);
        }
        let (size, checksum, _) = chunk_spec(index).ok_or(LoadError::HeaderInvalid)?;
        // Sector offsets are a prefix sum, so add up the chunks before this one
        // inside its own disc group; the groups before it cost nothing. A
        // group is at most a scene's atlases plus four, and this is only
        // reached at bootstrap and at a blacked-out scene gate, never in a tick.
        let (group, position) = group_of(index).ok_or(LoadError::HeaderInvalid)?;
        let mut sector_offset = self.group_bases[group] as usize;
        for j in 0..position {
            sector_offset += chunk_spec(member(group, j))
                .ok_or(LoadError::HeaderInvalid)?
                .0
                .div_ceil(SECTOR_BYTES);
        }
        Ok(PackEntry {
            chunk_id: index as u32 + 1,
            sector_offset: sector_offset as u32,
            sector_count: size.div_ceil(SECTOR_BYTES) as u32,
            byte_size: size as u32,
            checksum,
        })
    }
    /// Sectors of manifest scene `scene`'s disc group, and where chunk `j` of
    /// it starts inside the group.
    fn group_sectors(&self, scene: usize) -> usize {
        let g = 1 + scene;
        (0..staged_len(g))
            .map(|j| chunk_spec(member(g, j)).map_or(0, |c| c.0.div_ceil(SECTOR_BYTES)))
            .sum()
    }
    /// Sectors of members `j0..j1` of scene `scene`'s group.
    #[inline(never)]
    fn window_sectors(&self, scene: usize, j0: usize, j1: usize) -> usize {
        let g = 1 + scene;
        (j0..j1)
            .map(|j| chunk_spec(member(g, j)).map_or(0, |c| c.0.div_ceil(SECTOR_BYTES)))
            .sum()
    }
    /// Whether members `j0..j1` of scene `scene`'s group, staged together at
    /// the arena tail, each decode in place where they land. Each chunk
    /// decodes over the slice from its own output base to the end of its
    /// stored bytes, so a chunk early in the window has only what lies below
    /// it; the in-place margin is the one host/pack_scenes.py sizes the arena
    /// with. A window that merely fits the arena can still fail this:
    /// Tutorial_01's whole group did once Crossroads_10 grew the arena past
    /// it, and its first member's decode ran into its own compressed bytes.
    fn window_fits(&self, scene: usize, j0: usize, j1: usize) -> bool {
        let total = self.window_sectors(scene, j0, j1);
        if total == 0 || total * SECTOR_BYTES > ARENA_TOTAL {
            return false;
        }
        self.window_fits_at(scene, j0, j1, ARENA_TOTAL - total * SECTOR_BYTES)
    }
    /// `window_fits` with the window starting at `base` rather than at the
    /// arena tail.
    #[inline(never)]
    fn window_fits_at(&self, scene: usize, j0: usize, j1: usize, base: usize) -> bool {
        let g = 1 + scene;
        let total = self.window_sectors(scene, j0, j1);
        if total == 0 || base % 4 != 0 || base + total * SECTOR_BYTES > ARENA_TOTAL {
            return false;
        }
        let scene_tail = (SCENE_MANIFEST[scene].raw_len + 3) & !3;
        let mut first = 0usize;
        for j in j0..j1 {
            let index = member(g, j);
            let Some((stored, _, _)) = chunk_spec(index) else {
                return false;
            };
            let (raw, lo) = if index < SCENE_COUNT {
                (
                    SCENE_MANIFEST[index].raw_len,
                    SCENE_MANIFEST[index].ram_offset,
                )
            } else if (ATLAS_INDEX..FOCUS_INDEX).contains(&index) {
                (ATLASES[index - ATLAS_INDEX].raw_len, 0)
            } else if (COVERAGE_INDEX..METADATA_INDEX).contains(&index) {
                (COVERAGE_MANIFEST[index - COVERAGE_INDEX].raw_len, 0)
            } else if (METADATA_INDEX..EFFECT_INDEX).contains(&index) {
                (
                    WORLD_META_MANIFEST[index - METADATA_INDEX].raw_len,
                    scene_tail,
                )
            } else if (EFFECT_INDEX..BANK_INDEX).contains(&index) {
                (EFFECT_ART_MANIFEST[index - EFFECT_INDEX].raw_len, 0)
            }
            // The scene's one-shot bank is uploaded as it lies, never decoded.
            else if (SCENE_SFX_INDEX..PACK_CHUNKS).contains(&index) {
                (0, 0)
            } else {
                return false;
            };
            let end = base + first * SECTOR_BYTES + stored;
            if end < lo || end - lo < raw + (stored >> 8) + 32 {
                return false;
            }
            first += stored.div_ceil(SECTOR_BYTES);
        }
        true
    }
    fn tail_fits(&self, scene: usize) -> bool {
        self.window_fits(scene, 0, staged_len(1 + scene))
    }
    /// A group too large for the arena whole can still be staged in two
    /// windows: first everything the gate consumes before the scene decode
    /// (coverage, effect art, the atlases, which go to VRAM and the coverage
    /// buffer), then the scene and its metadata bank, read into the tail once
    /// the first window is spent. The first window is the part a background
    /// prefetch can hold under the resident scene, so the gate reads only the
    /// second. Crossroads_10's group is 252 sectors against a 209-sector arena
    /// since the False Knight's pages and parts joined it; its first window is
    /// 151 and its second 101.
    fn split_fits(&self, scene: usize) -> bool {
        let n = staged_len(1 + scene);
        !self.tail_fits(scene)
            && self.window_fits(scene, 0, n - 2)
            && self.window_fits(scene, n - 2, n)
    }
    /// Where a split group's second window goes: as low as the scene chunk's
    /// in-place decode allows, so the first window's highest chunks, which
    /// the gate decodes last, lie above it and decode while it is read.
    #[inline(never)]
    fn split_b_base(&self, scene: usize) -> usize {
        let g = 1 + scene;
        let n = staged_len(g);
        let b = self.window_sectors(scene, n - 2, n) * SECTOR_BYTES;
        let scene_tail = (SCENE_MANIFEST[scene].raw_len + 3) & !3;
        // Each member needs its stored end at least raw+margin past its output
        // base; the lowest window start that gives every member that.
        let mut low = 0usize;
        let mut first = 0usize;
        for j in n - 2..n {
            let index = member(g, j);
            let Some((stored, _, _)) = chunk_spec(index) else {
                return ARENA_TOTAL - b;
            };
            let (raw, lo) = if index < SCENE_COUNT {
                (
                    SCENE_MANIFEST[index].raw_len,
                    SCENE_MANIFEST[index].ram_offset,
                )
            } else {
                (WORLD_META_MANIFEST[scene].raw_len, scene_tail)
            };
            low = low
                .max((lo + raw + (stored >> 8) + 32).saturating_sub(stored + first * SECTOR_BYTES));
            first += stored.div_ceil(SECTOR_BYTES);
        }
        let low = low.next_multiple_of(4);
        if self.window_fits_at(scene, n - 2, n, low) {
            low
        } else {
            ARENA_TOTAL - b
        }
    }
    /// Sectors a prefetch of `scene` may hold: the whole group when it stages
    /// whole, the first window when it stages in two, none otherwise.
    #[inline(never)]
    fn staged_prefix(&self, scene: usize) -> usize {
        if self.tail_fits(scene) {
            self.group_sectors(scene)
        } else if self.split_fits(scene) {
            self.window_sectors(scene, 0, staged_len(1 + scene) - 2)
        } else {
            0
        }
    }
    /// Sectors at the head of `scene`'s group that no read needs: its code
    /// chunk, when every module the room needs is resident.
    fn code_skip(&self, scene: usize) -> usize {
        crate::modules::code_chunk(scene)
            .filter(|_| {
                crate::modules::all_code_resident(scene) || crate::modules::code_coming(scene)
            })
            .map_or(0, |k| crate::modules::chunk_len(k).div_ceil(SECTOR_BYTES))
    }
    /// First disc sector of scene `scene`'s group, as a WORLD.PAK-relative LBA.
    fn group_lba(&self, scene: usize) -> u32 {
        WORLD_PACK_DEFAULT_LBA + self.group_bases[1 + scene]
    }
    /// Choose how the admission of `scene` stages its chunks. A group that fits
    /// the arena is staged whole at the tail: what a prefetch already put
    /// there stays, and the rest is one read (one seek) started now, so later
    /// chunks keep arriving while earlier ones decode.
    #[inline(never)]
    fn stage_group(&mut self, scene: usize) -> Result<Stage, LoadError> {
        self.init()?;
        unsafe {
            GROUP_LANDED = usize::MAX;
        }
        let split = self.split_fits(scene);
        let total = self.staged_prefix(scene);
        if total == 0 {
            prefetch_discard();
            return Ok(Stage::Read);
        }
        let base = ARENA_TOTAL - total * SECTOR_BYTES;
        // `skip`: the code chunk's sectors when the room's code is already
        // resident (installed while the Knight walked here); nothing reads
        // them. They count as staged, so every chunk keeps its place.
        let (skip, held, adopted) = prefetch_take(scene, total, self.code_skip(scene), !split);
        crate::gate_probe::event(9, scene as u32, (skip + held) as u32 | (adopted as u32) << 15);
        let pre = skip + held;
        if adopted {
            // The prefetch still reading is exactly the read this gate would
            // start: decode its chunks as they land instead of waiting for all.
            crate::gate_probe::note(20, pre as u32);
            crate::gate_probe::note(21, total as u32);
            return Ok(Stage::Tail { base, pre });
        }
        crate::gate_probe::note(20, pre as u32);
        crate::gate_probe::note(21, total as u32);
        if held > 0 && pre < total {
            // A partial prefetch is the group's prefix, left at the very end
            // of the arena while the resident scene still needed the rest.
            // Slide it down to where the whole group goes (a few ms).
            let arena = unsafe {
                core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(), ARENA_TOTAL)
            };
            let from = ARENA_TOTAL - held * SECTOR_BYTES;
            let to = base + skip * SECTOR_BYTES;
            let mut done = 0;
            while done < held * SECTOR_BYTES {
                let n = (held * SECTOR_BYTES - done).min(16384);
                arena.copy_within(from + done..from + done + n, to + done);
                done += n;
                checkpoint();
            }
        }
        if pre < total {
            crate::gate_probe::set(crate::gate_probe::DRIVE);
            // Music that outlasts this read is refilled at the gate's end
            // rather than with two seeks now.
            if crate::music::outlasts_read(total - pre) {
                // Stopping a top-up pays for a short read; a long one would
                // leave the next gate short of music instead.
                if total - pre <= CUT_MUSIC_MAX {
                    cut_music();
                }
                drive_idle();
            } else {
                room_drive();
            }
            crate::gate_probe::set(crate::gate_probe::CD);
            crate::gate_probe::read_started();
            let destination = unsafe {
                (&raw mut BUFFERS.words)
                    .cast::<u8>()
                    .add(base + pre * SECTOR_BYTES)
                    .cast::<u32>()
            };
            if let Err(diag) = unsafe {
                cd_stream::start(destination, total - pre, self.group_lba(scene) + pre as u32)
            } {
                unsafe {
                    HK_CD_DIAG = diag;
                }
                return Err(LoadError::PayloadRead);
            }
            scene_sectors(total - pre);
        }
        if split {
            unsafe {
                HK_SPLIT_GATES = HK_SPLIT_GATES.saturating_add(1);
            }
            self.split_b = None;
            return Ok(Stage::Split {
                base,
                pre,
                a: total,
            });
        }
        Ok(Stage::Tail { base, pre })
    }
    /// Make chunk `index`'s stored bytes available for a decode over the
    /// slice that starts at `lo`: the slice's end and where the stored bytes
    /// begin inside it (zero: at the front, the decoder relocates them).
    #[inline(never)]
    fn stage(&mut self, index: usize, st: Stage, lo: usize) -> Result<(usize, usize), LoadError> {
        let e = self.entry(index)?;
        match st {
            Stage::Read => {
                self.read(e, lo)?;
                Ok((SCENE_ARENA_BYTES, 0))
            }
            Stage::Tail { base, pre } => {
                let (group, _) = group_of(index).ok_or(LoadError::HeaderInvalid)?;
                let first = (e.sector_offset - self.group_bases[group]) as usize;
                let need = (first + e.sector_count as usize).saturating_sub(pre);
                // The group's read is over and the drive moved on (the room's
                // art): what it stored was counted when it ended.
                let landed = unsafe { GROUP_LANDED };
                if landed != usize::MAX {
                    if landed < need {
                        return Err(LoadError::PayloadRead);
                    }
                } else if need > 0 {
                    crate::gate_probe::set(crate::gate_probe::CD);
                    loop {
                        if cd_stream::received() >= need {
                            break;
                        }
                        match cd_stream::status() {
                            cd_stream::Status::Failed(diag) => {
                                unsafe {
                                    HK_CD_DIAG = diag;
                                }
                                return Err(LoadError::PayloadRead);
                            }
                            cd_stream::Status::Done | cd_stream::Status::Idle => {
                                if cd_stream::received() < need {
                                    return Err(LoadError::PayloadRead);
                                }
                                break;
                            }
                            cd_stream::Status::Busy => {
                                checkpoint();
                                core::hint::spin_loop();
                            }
                        }
                    }
                    crate::gate_probe::set(crate::gate_probe::OTHER);
                }
                let start = base + first * SECTOR_BYTES;
                if start < lo {
                    return Err(LoadError::RoomMismatch);
                }
                Ok((start + e.byte_size as usize, start - lo))
            }
            Stage::Split { base, pre, a } => {
                let (group, _) = group_of(index).ok_or(LoadError::HeaderInvalid)?;
                let first = (e.sector_offset - self.group_bases[group]) as usize;
                let scene = group - 1;
                let b_sectors = self.group_sectors(scene) - a;
                // The second window's read starts once every first-window
                // chunk it overlaps is spent: at the first chunk above it, or
                // at the scene chunk. The gate consumes in group order, which
                // is address order within the window, so that is exact.
                let b_low = self.split_b_base(scene);
                let b_end = b_low + b_sectors * SECTOR_BYTES;
                if self.split_b.is_none() && (first >= a || base + first * SECTOR_BYTES >= b_end) {
                    crate::gate_probe::set(crate::gate_probe::CD);
                    // The rest of the first window may still be arriving.
                    loop {
                        match cd_stream::status() {
                            cd_stream::Status::Busy => {
                                checkpoint();
                                core::hint::spin_loop();
                            }
                            cd_stream::Status::Failed(diag) => {
                                unsafe {
                                    HK_CD_DIAG = diag;
                                }
                                return Err(LoadError::PayloadRead);
                            }
                            cd_stream::Status::Done | cd_stream::Status::Idle => break,
                        }
                    }
                    crate::gate_probe::set(crate::gate_probe::DRIVE);
                    room_drive();
                    crate::gate_probe::set(crate::gate_probe::CD);
                    crate::gate_probe::read_started();
                    let destination = unsafe {
                        (&raw mut BUFFERS.words)
                            .cast::<u8>()
                            .add(b_low)
                            .cast::<u32>()
                    };
                    if let Err(diag) = unsafe {
                        cd_stream::start(destination, b_sectors, self.group_lba(scene) + a as u32)
                    } {
                        unsafe {
                            HK_CD_DIAG = diag;
                        }
                        return Err(LoadError::PayloadRead);
                    }
                    scene_sectors(b_sectors);
                    self.split_b = Some(b_low);
                }
                if first < a {
                    // A first-window chunk: already landed (the read above only
                    // starts once the first window's own read is over).
                    let start = base + first * SECTOR_BYTES;
                    if self.split_b.is_none() {
                        return self.stage(index, Stage::Tail { base, pre }, lo);
                    }
                    if start < lo {
                        return Err(LoadError::RoomMismatch);
                    }
                    return Ok((start + e.byte_size as usize, start - lo));
                }
                let b_base = self.split_b.ok_or(LoadError::RoomMismatch)?;
                let need = first - a + e.sector_count as usize;
                crate::gate_probe::set(crate::gate_probe::CD);
                loop {
                    if cd_stream::received() >= need {
                        break;
                    }
                    match cd_stream::status() {
                        cd_stream::Status::Failed(diag) => {
                            unsafe {
                                HK_CD_DIAG = diag;
                            }
                            return Err(LoadError::PayloadRead);
                        }
                        cd_stream::Status::Done | cd_stream::Status::Idle => {
                            if cd_stream::received() < need {
                                return Err(LoadError::PayloadRead);
                            }
                            break;
                        }
                        cd_stream::Status::Busy => {
                            checkpoint();
                            core::hint::spin_loop();
                        }
                    }
                }
                crate::gate_probe::set(crate::gate_probe::OTHER);
                let start = b_base + (first - a) * SECTOR_BYTES;
                if start < lo {
                    return Err(LoadError::RoomMismatch);
                }
                Ok((start + e.byte_size as usize, start - lo))
            }
        }
    }
    /// The destination is a private suffix; terminal Done includes Pause so
    /// no IRQ can continue writing when decoding or SPU upload begins.
    fn read(&mut self, entry: PackEntry, offset: usize) -> Result<(), LoadError> {
        crate::gate_probe::set(crate::gate_probe::DRIVE);
        room_drive();
        crate::gate_probe::set(crate::gate_probe::CD);
        crate::gate_probe::read_started();
        if offset % 4 != 0
            || offset > SCENE_ARENA_BYTES
            || entry.sector_count as usize > (SCENE_ARENA_BYTES - offset) / SECTOR_BYTES
        {
            return Err(LoadError::RoomMismatch);
        }
        let destination = unsafe {
            (&raw mut BUFFERS.words)
                .cast::<u8>()
                .add(offset)
                .cast::<u32>()
        };
        if let Err(diag) = unsafe {
            cd_stream::start(
                destination,
                entry.sector_count as usize,
                WORLD_PACK_DEFAULT_LBA + entry.sector_offset,
            )
        } {
            unsafe {
                HK_CD_DIAG = diag;
            }
            return Err(LoadError::PayloadRead);
        }
        scene_sectors(entry.sector_count as usize);
        loop {
            checkpoint();
            match cd_stream::status() {
                cd_stream::Status::Done => {
                    crate::gate_probe::set(crate::gate_probe::OTHER);
                    return Ok(());
                }
                cd_stream::Status::Failed(diag) => {
                    unsafe {
                        HK_CD_DIAG = diag;
                    }
                    return Err(LoadError::PayloadRead);
                }
                cd_stream::Status::Idle | cd_stream::Status::Busy => core::hint::spin_loop(),
            }
        }
    }
    /// Stage one ADPCM bank in the arena front and hand back its exact checked
    /// bytes. The caller uploads them to the SPU and keeps nothing, the same
    /// deal `prepare_effect_art` makes with VRAM.
    ///
    /// Bootstrap only, and the refusal below is the whole reason: the banks
    /// share the scene arena, so a load while a scene is admitted would write
    /// over the resident scene and every view borrowed from it.
    ///
    /// The arena is word aligned and the bank starts at its front, which is
    /// what keeps the SDK's SPU DMA upload path available; the banks used to
    /// get that from their own `repr(align(4))` static.
    /// Two passes through a stack sector preserve every admitted arena byte.
    /// The first validates the complete bank; the second installs it while
    /// publication is revoked. I/O/hash failure cannot publish a partial bank.
    #[inline(never)]
    fn prepare_runner_audio(&mut self,scene:usize)->Result<(),LoadError> {
        let mossman=crate::runner_audio::scene_is_mossman(SCENE_MANIFEST[scene].scene_id as usize);
        if crate::runner_audio::family_ready(mossman) {return Ok(());}
        self.init()?;
        let index=if mossman {MOSSMAN_BANK}else{2};
        let (size,checksum)=AUDIO_BANKS[index];
        let entry=self.entry(audio_bank_index(index))?;
        if entry.byte_size as usize!=size || size%16!=0 || size>20464 {return Err(LoadError::RoomMismatch);}
        let end = entry.sector_offset.checked_add(entry.sector_count).ok_or(LoadError::HeaderInvalid)?;
        if entry.sector_count as usize != size.div_ceil(SECTOR_BYTES)
            || entry.sector_offset < self.group_bases[0] || end > self.group_bases[1] {
            return Err(LoadError::HeaderInvalid);
        }
        let lba = WORLD_PACK_DEFAULT_LBA.checked_add(entry.sector_offset).ok_or(LoadError::HeaderInvalid)?;
        lba.checked_add(entry.sector_count).ok_or(LoadError::HeaderInvalid)?;
        // Read ahead with the prefetch: validate and upload it from RAM.
        let b = prefetch_state().bank;
        if b.ok && b.scene == scene && b.sectors == size.div_ceil(SECTOR_BYTES) {
            prefetch_state().bank.ok = false;
            let bytes = unsafe {
                core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>().add(b.at), size)
            };
            let mut hash = 0x811c9dc5u32;
            let mut good = true;
            for (i, part) in bytes.chunks(SECTOR_BYTES).enumerate() {
                crate::input::checkpoint();
                good &= crate::runner_audio::validate_fragment(mossman, i * SECTOR_BYTES, part);
                for &x in part {
                    hash = (hash ^ u32::from(x)).wrapping_mul(0x01000193);
                }
            }
            if good && hash == checksum && crate::runner_audio::begin_overlay() {
                for (i, part) in bytes.chunks(SECTOR_BYTES).enumerate() {
                    crate::input::checkpoint();
                    crate::runner_audio::upload_fragment(i * SECTOR_BYTES, part);
                }
                crate::runner_audio::finish_overlay(mossman);
                unsafe {
                    HK_BANK_AHEAD_USED += 1;
                }
                return Ok(());
            }
        }
        unsafe {
            HK_BANK_GATE_READS += 1;
        }
        prefetch_discard();
        room_drive();
        let mut words=[0u32;SECTOR_WORDS];
        for pass in 0..2 {
            if pass==1 && !crate::runner_audio::begin_overlay() {return Err(LoadError::RoomMismatch);}
            if !unsafe{self.reader.start_read_seek_first_with(lba,SEEK_POLL,&mut pad_idle)} {
                unsafe{self.reader.stop();}
                return Err(LoadError::PayloadRead);
            }
            let result=(|| {
                let mut hash=0x811c9dc5u32;
                let mut offset=0;
                while offset<size {
                    checkpoint();
                    if !sector(&mut self.reader,&mut words){return Err(LoadError::PayloadRead);}
                    let n=(size-offset).min(SECTOR_BYTES);
                    let bytes=unsafe{core::slice::from_raw_parts(words.as_ptr().cast::<u8>(),n)};
                    if !crate::runner_audio::validate_fragment(mossman,offset,bytes){return Err(LoadError::RoomMismatch);}
                    for &b in bytes {hash=(hash^u32::from(b)).wrapping_mul(0x01000193);}
                    if pass==1 {crate::runner_audio::upload_fragment(offset,bytes);}
                    offset+=n;
                }
                if hash!=checksum {return Err(LoadError::RoomMismatch);}
                Ok(())
            })();
            unsafe{self.reader.stop();}
            result?;
        }
        crate::runner_audio::finish_overlay(mossman);
        Ok(())
    }
    fn audio_bank(&mut self, bank: usize) -> Result<&[u8], LoadError> {
        self.init()?;
        if self.loaded_scenes != 0 {
            return Err(LoadError::Ambience);
        }
        let (size, checksum) = AUDIO_BANKS[bank];
        let entry = self.entry(audio_bank_index(bank))?;
        if entry.byte_size as usize != size {
            return Err(LoadError::RoomMismatch);
        }
        self.read(entry, 0)?;
        let bytes =
            unsafe { core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>(), size) };
        // The header entry was matched against this same pair at init(); this
        // checks the payload the drive actually delivered, as the scene, atlas
        // and coverage decoders do through the decoder's own checksum.
        let mut hash = 0x811c9dc5u32;
        for block in bytes.chunks(1024) {
            checkpoint();
            for &byte in block {
                hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
            }
        }
        if hash != checksum {
            return Err(LoadError::Checksum);
        }
        Ok(bytes)
    }
    /// Bootstrap-only ambient loading reuses free RAM before any scene is
    /// admitted. Later calls are no-ops: all ambient samples live in SPU or
    /// the dedicated main-RAM stream cache, with no playback-time CD access.
    ///
    /// The three sample banks load here for the same reason and under the same
    /// invariant. SPU order matters and is preserved: `audio::init` already
    /// reset the SPU before the title screen, and nothing below resets it, so
    /// the SFX bank goes first, then ambience, Focus, Geo and Runner above it.
    #[cfg_attr(feature = "intro", inline(never))]
    #[cfg_attr(all(feature = "intro", not(test)), optimize(size))]
    pub fn prepare_ambience(&mut self) -> Result<(), LoadError> {
        if crate::audio::ready()
            && crate::ambience::is_ready()
            && crate::focus_audio::ready()
            && crate::geo_audio::ready()
            && crate::runner_audio::ready()
            && crate::audio::world_ready()
            && crate::game_map::ready()
        {
            return Ok(());
        }
        if self.loaded_scenes != 0 {
            mark(3, LoadError::Ambience as u32);
            return Err(LoadError::Ambience);
        }
        mark(1, 0);
        let result = (|| {
            self.init()?;
            if !crate::audio::ready() {
                let bank = self.audio_bank(0)?;
                crate::audio::upload(bank);
            }
            // No clip yet: each is read by the scene gate whose area first plays it.
            crate::ambience::begin_load();
            if !crate::ambience::finish_load() {
                return Err(LoadError::Ambience);
            }
            if !crate::focus_audio::ready() {
                let entry = self.entry(FOCUS_INDEX)?;
                self.read(entry, 0)?;
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        (&raw const BUFFERS.words).cast::<u8>(),
                        entry.byte_size as usize,
                    )
                };
                if !crate::focus_audio::upload(bytes) {
                    return Err(LoadError::Ambience);
                }
            }
            if !crate::geo_audio::ready() {
                let bank = self.audio_bank(1)?;
                crate::geo_audio::upload(bank);
            }
            if !crate::runner_audio::ready() {
                let bank = self.audio_bank(2)?;
                crate::runner_audio::upload(bank);
            }
            // Only when the read before the title failed; it is normally done.
            self.prepare_world_sfx()?;
            Ok(())
        })();
        if let Err(error) = result {
            mark(3, error as u32);
        }
        result
    }
    /// The world one-shots (enemy death, the Knight's death, menu confirm and
    /// start, the Lifeblood cocoon), read before the title screen: the title
    /// menu plays two of them, and before it nothing else owns the drive or the
    /// arena. The pad sampler is not running yet, so the read consumes no poll
    /// and an input tape stays aligned. A failure costs those sounds until
    /// bootstrap reads the bank again through `prepare_ambience`'s retry.
    ///
    /// The quick map's art follows it on the disc and is read here too, for
    /// the same reasons, and uploaded to its page, which the title art does
    /// not touch.
    pub fn prepare_world_sfx(&mut self) -> Result<(), LoadError> {
        if !crate::audio::world_ready() {
            let bank = self.audio_bank(WORLD_BANK)?;
            crate::audio::upload_world(bank);
        }
        if !crate::game_map::ready() {
            let blob = self.audio_bank(MAP_BANK)?;
            crate::game_map::upload(blob);
        }
        Ok(())
    }
    /// The title art's common part, staged in the arena front: before the title (nothing
    /// else owns the drive or the arena then) and on a failed load's retry
    /// screen. The caller uploads it to VRAM before the arena is used again. The rest of the
    /// chunk is the other title styles (`style_art`).
    pub fn menu_art(&mut self) -> Result<&[u8], LoadError> {
        self.stage_menu(0, crate::boot_art::BOOT_COMMON_BYTES, 0, crate::boot_art::BOOT_COMMON_CHECKSUM)
    }
    /// The title's code (modules.rs `TITLE`): its chunk, which heads the title art on the disc, read into the arena
    /// behind the art's common part and installed into the pool. Before the title and on a failed load's retry
    /// screen, right after `menu_art` (whose bytes it leaves alone); `title::close` evicts the module again. Does
    /// nothing in a build that links the title resident (`HK_RESIDENT_MODULES`).
    #[inline(never)]
    pub fn title_code(&mut self) -> Result<(), LoadError> {
        let Some(chunk) = crate::modules::boot_chunk(crate::modules::TITLE) else {
            return Ok(());
        };
        self.init()?;
        if self.loaded_scenes != 0 {
            return Err(LoadError::Ambience);
        }
        let entry = self.entry(CODE_INDEX + chunk)?;
        let at = crate::boot_art::BOOT_COMMON_BYTES;
        self.read(entry, at)?;
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&raw const BUFFERS.words).cast::<u8>().add(at),
                entry.byte_size as usize,
            )
        };
        if fnv(bytes) != entry.checksum {
            return Err(LoadError::Checksum);
        }
        if crate::modules::install_boot(crate::modules::TITLE, bytes) {
            Ok(())
        } else {
            Err(LoadError::Checksum)
        }
    }
    /// One of the other title styles, read from its sectors of the title chunk into the arena after
    /// the common part (which stays in place): its background and its extras, from the
    /// background's first byte. Only before the first scene is loaded, and with the title's song stopped.
    #[inline(never)]
    pub fn style_art(&mut self, style: usize) -> Result<&[u8], LoadError> {
        let (start, extra_at, extra_len, checksum) = crate::boot_art::STYLE_ART[style];
        self.stage_menu(start as usize, (extra_at + extra_len) as usize, crate::boot_art::BOOT_COMMON_BYTES, checksum)
    }
    /// Bytes `from..to` of the title chunk (`from` on a sector boundary), staged `at` bytes into the arena and
    /// checked against `checksum`.
    fn stage_menu(&mut self, from: usize, to: usize, at: usize, checksum: u32) -> Result<&[u8], LoadError> {
        self.init()?;
        if self.loaded_scenes != 0 {
            return Err(LoadError::Ambience);
        }
        let entry = self.entry(MENU_INDEX)?;
        if entry.byte_size as usize != crate::boot_art::BOOT_ART_BYTES {
            return Err(LoadError::RoomMismatch);
        }
        let first = from / SECTOR_BYTES;
        let part = PackEntry {
            sector_offset: entry.sector_offset + first as u32,
            sector_count: (to.div_ceil(SECTOR_BYTES) - first) as u32,
            ..entry
        };
        self.read(part, at)?;
        let bytes = unsafe {
            core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>().add(at), to - from)
        };
        if fnv(bytes) != checksum {
            return Err(LoadError::Checksum);
        }
        Ok(bytes)
    }
    /// Read the quick map's art back into static page 18 after a scene that
    /// needed all nineteen pages (the False Knight's arena) wrote over it, at
    /// the first gate into a scene that leaves the page alone. Staged at the
    /// arena front like an ambience clip, before the scene's own reads.
    #[inline(never)]
    fn prepare_map(&mut self, scene: usize) -> Result<(), LoadError> {
        if crate::game_map::ready() || scene_pages(scene) >= hk_cache::residency::STATIC_PAGES {
            return Ok(());
        }
        prefetch_guard(crate::game_map::BANK_BYTES.div_ceil(SECTOR_BYTES) * SECTOR_BYTES);
        let blob = self.audio_bank(MAP_BANK)?;
        crate::game_map::upload(blob);
        unsafe {
            HK_MAP_RELOADS = HK_MAP_RELOADS.saturating_add(1);
        }
        Ok(())
    }
    /// Stage one ambience clip in the arena front and hand it to the mixer,
    /// which checks it and moves it to RAM or to its SPU address.
    fn ambience_clip(&mut self, index: usize) -> Result<(), LoadError> {
        if self.loaded_scenes != 0 {
            return Err(LoadError::Ambience);
        }
        let entry = self.entry(SCENE_COUNT + index)?;
        self.read(entry, 0)?;
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&raw const BUFFERS.words).cast::<u8>(),
                entry.byte_size as usize,
            )
        };
        crate::gate_probe::set(crate::gate_probe::AMBIENCE);
        if !crate::ambience::upload(index, bytes) {
            return Err(LoadError::Ambience);
        }
        Ok(())
    }
    /// Load the ambience clips the scene's cue plays that its area has not
    /// loaded yet, while the arena is still free to stage them. Most gates
    /// stay inside one area and read nothing here.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    fn prepare_scene_ambience(&mut self, scene: usize) -> Result<(), LoadError> {
        self.init()?;
        let wanted = crate::ambience::missing(SCENE_MANIFEST[scene].scene_id);
        for index in 0..CLIP_COUNT {
            if wanted & (1 << index) != 0 {
                self.ambience_clip(index)?;
            }
        }
        Ok(())
    }
    /// The scene's one-shot bank, first in its group: checked, then uploaded
    /// to the SPU addresses the cook placed it at (scene_sfx.rs). The bytes
    /// are the group's own prefix, consumed before any decode reuses them.
    fn prepare_scene_sfx(&mut self, scene: usize, st: Stage) -> Result<(), LoadError> {
        crate::scene_sfx::invalidate();
        let id = SCENE_MANIFEST[scene].scene_id as usize;
        let (len, checksum) = crate::scene_sfx::chunk(id);
        let (end, input) = self.stage(SCENE_SFX_INDEX + scene, st, 0)?;
        if end < input + len {
            return Err(LoadError::RoomMismatch);
        }
        let bytes = unsafe {
            core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>().add(input), len)
        };
        let mut hash = 0x811c9dc5u32;
        if preverified(SCENE_SFX_INDEX + scene) {
            hash = checksum;
        } else {
            for block in bytes.chunks(1024) {
                checkpoint();
                for &byte in block {
                    hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
                }
            }
        }
        if hash != checksum {
            return Err(LoadError::Checksum);
        }
        crate::scene_sfx::upload(id, bytes);
        Ok(())
    }
    /// The scene's code modules (modules.rs): resident already when the
    /// prefetch let them install while the Knight walked here; otherwise
    /// installed now from the group's first chunk. Nothing of the outgoing
    /// scene runs during an admission, and nothing of this one yet.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    fn prepare_code(&mut self, scene: usize, st: Stage) -> Result<(), LoadError> {
        let Some(k) = crate::modules::code_chunk(scene) else {
            crate::modules::admit(scene, || None);
            return Ok(());
        };
        let len = crate::modules::chunk_len(k);
        let ok = crate::modules::admit(scene, || {
            let (end, input) = self.stage(CODE_INDEX + k, st, 0).ok()?;
            if end < input + len {
                return None;
            }
            Some(unsafe {
                core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>().add(input), len)
            })
        });
        if ok {
            Ok(())
        } else {
            Err(LoadError::Checksum)
        }
    }
    /// Stages proofs only while no geometry view is admitted, then preserves
    /// their checked bytes in the separate scene-owned coverage arena.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    fn prepare_coverage(&mut self, scene: usize, st: Stage) -> Result<(), LoadError> {
        self.init()?;
        let c = COVERAGE_MANIFEST[scene];
        if self.coverage_scene == scene && self.coverage_len == c.raw_len {
            return Ok(());
        }
        if self.loaded_scenes != 0 {
            return Err(LoadError::Coverage);
        }
        self.coverage_scene = EMPTY;
        self.coverage_len = 0;
        unsafe {
            HK_COVERAGE_SCENE = 0;
            HK_COVERAGE_BYTES = 0;
        }
        let (end, input) = self.stage(c.chunk_id - 1, st, 0)?;
        let arena =
            unsafe { core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(), end) };
        let mut decoder = Decoder::new_bytes(c.stored_len, c.stored_fnv, c.raw_len, c.raw_fnv)
            .at(input)
            .hashed(preverified(c.chunk_id - 1));
        loop {
            checkpoint();
            unsafe {
                HK_ROOM_DECODE_PHASE = decoder.phase_id();
            }
            crate::gate_probe::set(crate::gate_probe::decoder(decoder.phase_id()));
            match decoder.step(arena, DECODE_STEP) {
                Ok(Some(len)) => {
                    if len != c.raw_len {
                        return Err(LoadError::Coverage);
                    }
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(match error {
                        DecodeError::Checksum => LoadError::Checksum,
                        DecodeError::Decompress => LoadError::Decompress,
                        DecodeError::RoomFormat => LoadError::Coverage,
                    })
                }
            }
        }
        crate::gate_probe::set(crate::gate_probe::COVERAGE_CHECK);
        let expected = CoverageExpected {
            scene_id: c.scene_id,
            scene_raw_fnv: c.scene_raw_fnv,
            atlas_fnv: c.atlas_fnv,
            draw_pool_count: c.draw_pool_count,
        };
        let mut check = CoverageValidation::new(&arena[..c.raw_len], expected)
            .map_err(|_| LoadError::Coverage)?;
        loop {
            checkpoint();
            if check.step(32).map_err(|_| LoadError::Coverage)? {
                break;
            }
        }
        check.finish().map_err(|_| LoadError::Coverage)?;
        let destination = unsafe {
            core::slice::from_raw_parts_mut(
                (&raw mut COVERAGE_BUFFER.words).cast::<u8>(),
                c.raw_len,
            )
        };
        for (source, destination) in arena[..c.raw_len]
            .chunks(1024)
            .zip(destination.chunks_mut(1024))
        {
            checkpoint();
            destination.copy_from_slice(source);
        }
        self.coverage_scene = scene;
        self.coverage_len = c.raw_len;
        unsafe {
            HK_COVERAGE_LOADS = HK_COVERAGE_LOADS.saturating_add(1);
            HK_COVERAGE_BYTES = c.raw_len as u32;
            HK_COVERAGE_SCENE = scene as u32 + 1;
        }
        Ok(())
    }
    /// Decode the scene's effect texels into the arena front and upload them to
    /// the fixed effect VRAM rects. Runs before the scene decode owns the arena;
    /// VRAM keeps the texels, so nothing stays resident in RAM.
    #[inline(never)]
    #[cfg_attr(all(feature = "intro", not(test)), optimize(size))]
    fn prepare_effect_art(&mut self, scene: usize, st: Stage) -> Result<(), LoadError> {
        self.init()?;
        let f = EFFECT_ART_MANIFEST[SCENE_MANIFEST[scene].scene_id as usize];
        if self.effect_scene == scene {
            return Ok(());
        }
        if self.loaded_scenes != 0 {
            return Err(LoadError::EffectArt);
        }
        self.effect_scene = EMPTY;
        let (end, input) = self.stage(
            EFFECT_INDEX + SCENE_MANIFEST[scene].scene_id as usize,
            st,
            0,
        )?;
        let arena =
            unsafe { core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(), end) };
        let mut decoder = Decoder::new_bytes(f.stored_len, f.stored_fnv, f.raw_len, f.raw_fnv)
            .at(input)
            .hashed(preverified(
                EFFECT_INDEX + SCENE_MANIFEST[scene].scene_id as usize,
            ));
        loop {
            checkpoint();
            crate::gate_probe::set(crate::gate_probe::decoder(decoder.phase_id()));
            match decoder.step(arena, DECODE_STEP) {
                Ok(Some(len)) => {
                    if len != f.raw_len {
                        return Err(LoadError::EffectArt);
                    }
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(match error {
                        DecodeError::Checksum => LoadError::Checksum,
                        DecodeError::Decompress => LoadError::Decompress,
                        DecodeError::RoomFormat => LoadError::EffectArt,
                    })
                }
            }
        }
        crate::gate_probe::set(crate::gate_probe::UPLOAD);
        psx_gpu::draw_sync();
        if !crate::world::particles::break_effects::load_scene(
            SCENE_MANIFEST[scene].scene_id as usize,
            &arena[..f.raw_len],
        ) {
            return Err(LoadError::EffectArt);
        }
        self.effect_scene = scene;
        Ok(())
    }
    /// Stage and validate the immutable HKWMTA bank before any scene object
    /// view is published. Metadata is copied into the unused, aligned tail of
    /// the admitted scene arena; the scene decoder only writes the scene's
    /// raw prefix and later CD scratch reads stay in the arena's front.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    fn prepare_metadata(&mut self, scene: usize, st: Stage) -> Result<(), LoadError> {
        self.init()?;
        let m = WORLD_META_MANIFEST[scene];
        if self.metadata_scene == scene && self.metadata_len == m.raw_len {
            return Ok(());
        }
        // The resident scene owns the arena; the bank lives in the tail beyond it.
        if self.loaded_scenes == 0 || (SCENE_GATE_LOAD && self.resident_scene != scene) {
            return Err(LoadError::WorldMeta);
        }
        self.revoke_metadata();
        let scene_tail = (SCENE_MANIFEST[scene].raw_len.saturating_add(3)) & !3;
        // The scene's own bank, which host/pack_scenes.py sized this arena to
        // hold behind every scene (not the widest bank of all).
        let own = ((m.raw_len + 3) & !3).max(m.stored_len.div_ceil(SECTOR_BYTES) * SECTOR_BYTES);
        if scene_tail
            .checked_add(own)
            .is_none_or(|end| end > SCENE_ARENA_BYTES)
        {
            return Err(LoadError::WorldMeta);
        }
        // Read and decode in place within the tail: the decoder relocates the
        // compressed bytes to the end of this slice, never over the scene.
        let (end, input) = self.stage(m.chunk_id - 1, st, scene_tail)?;
        let tail = unsafe {
            core::slice::from_raw_parts_mut(
                (&raw mut BUFFERS.words).cast::<u8>().add(scene_tail),
                end - scene_tail,
            )
        };
        let mut decoder = Decoder::new_bytes(m.stored_len, m.stored_fnv, m.raw_len, m.bank_fnv)
            .at(input)
            .hashed(preverified(m.chunk_id - 1));
        loop {
            checkpoint();
            crate::gate_probe::set(crate::gate_probe::decoder(decoder.phase_id()));
            match decoder.step(tail, DECODE_STEP) {
                Ok(Some(len)) => {
                    if len != m.raw_len {
                        return Err(LoadError::WorldMeta);
                    }
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(match error {
                        DecodeError::Checksum => LoadError::Checksum,
                        DecodeError::Decompress => LoadError::Decompress,
                        DecodeError::RoomFormat => LoadError::WorldMeta,
                    })
                }
            }
        }
        crate::gate_probe::set(crate::gate_probe::META_CHECK);
        let bank = checked_world_metadata(&tail[..m.raw_len], scene)?;
        let expected_regions = REGION_SCENE_LOCAL
            .iter()
            .filter(|&&(owner, _)| owner as usize == scene)
            .count();
        if bank.region_count() != expected_regions {
            return Err(LoadError::WorldMeta);
        }
        let mut previous_global = 0;
        let mut mapped = 0;
        for region in bank.regions() {
            checkpoint();
            let global = region.global_id() as usize;
            if global <= previous_global
                || global > REGION_SCENE_LOCAL.len()
                || scene_index(global - 1) != scene
            {
                return Err(LoadError::WorldMeta);
            }
            // The bank is the only source of region bounds and camera; the
            // slot's owner was checked above through REGION_SCENE_LOCAL
            // (manifest scene indices, unlike world::scene_of's scene ids).
            // Neighbours are global chunk ids (1-based) of catalogue slots.
            for encoded in region.neighbours() {
                let encoded = encoded.map_err(|_| LoadError::WorldMeta)? as usize;
                if encoded == 0 || encoded > REGION_SCENE_LOCAL.len() {
                    return Err(LoadError::WorldMeta);
                }
            }
            previous_global = global;
            mapped += 1;
        }
        if mapped != expected_regions {
            return Err(LoadError::WorldMeta);
        }
        self.metadata_scene = scene;
        self.metadata_len = m.raw_len;
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&raw const BUFFERS.words).cast::<u8>().add(scene_tail),
                m.raw_len,
            )
        };
        // Admission already ran the complete checked parse and the legacy
        // differential; re-validating every section on each spatial lookup
        // cost several VBlanks per tick and overflowed the input queue.
        self.metadata_view = Some(unsafe { WorldMeta::validated_view(bytes) });
        Ok(())
    }
    /// Every path that stops the bank bytes being the admitted ones.
    fn revoke_metadata(&mut self) {
        self.metadata_scene = EMPTY;
        self.metadata_len = 0;
        self.metadata_view = None;
    }
    /// Exclusive atlas admission. No Scene/Room view may exist while this
    /// buffer is scratch; completed chunks remain in VRAM across retries.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    fn prepare_atlases(&mut self, scene: usize, st: Stage) -> Result<(), LoadError> {
        self.init()?;
        if self.atlases_ready() && (!SCENE_GATE_LOAD || self.atlas_scene == scene) {
            return Ok(());
        }
        if self.loaded_scenes != 0 {
            return Err(LoadError::RoomMismatch);
        }
        let (start, end) = if SCENE_GATE_LOAD {
            (
                SCENE_ATLAS_RANGES[scene].0 as usize,
                SCENE_ATLAS_RANGES[scene].1 as usize,
            )
        } else {
            (0, ATLASES.len())
        };
        if self.atlas_scene != scene {
            self.loaded_atlases = start;
            self.atlas_scene = scene;
            // A scene admitted ahead keeps its placement; any other takes the manifest's.
            let ah = ahead();
            let keep = AHEAD_ON && ah.started && ah.scene == scene;
            unsafe {
                SHIFT_PAGE[scene] = if keep { ah.shift_page } else { 0 };
                SHIFT_PAL[scene] = if keep { ah.shift_pal } else { 0 };
            }
        }
        while self.loaded_atlases < end {
            if ahead_done(scene, self.loaded_atlases) {
                unsafe {
                    HK_AHEAD_SKIPPED += 1;
                }
                self.loaded_atlases += 1;
                continue;
            }
            let a = &ATLASES[self.loaded_atlases];
            let (end, input) = self.stage(ATLAS_INDEX + self.loaded_atlases, st, 0)?;
            let arena = unsafe {
                core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(), end)
            };
            let mut decoder = Decoder::new_bytes(a.stored_len, a.stored_fnv, a.raw_len, a.raw_fnv)
                .at(input)
                .hashed(preverified(ATLAS_INDEX + self.loaded_atlases));
            loop {
                checkpoint();
                unsafe {
                    HK_ROOM_DECODE_PHASE = decoder.phase_id();
                }
                crate::gate_probe::set(crate::gate_probe::decoder(decoder.phase_id()));
                match decoder.step(arena, DECODE_STEP) {
                    Ok(Some(len)) => {
                        if len != a.raw_len {
                            return Err(LoadError::RoomMismatch);
                        }
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        return Err(match error {
                            DecodeError::Checksum => LoadError::Checksum,
                            DecodeError::Decompress => LoadError::Decompress,
                            DecodeError::RoomFormat => LoadError::RoomFormat,
                        })
                    }
                }
            }
            crate::gate_probe::set(crate::gate_probe::UPLOAD);
            // Page 18 is the quick map's until a nineteen-page scene takes it.
            if a.kind == 0 && scene_page_base(scene) + a.first + a.count >= hk_cache::residency::STATIC_PAGES {
                crate::game_map::lose();
            }
            crate::vram_cache::upload_atlas_at(
                a,
                &arena[..a.raw_len],
                scene_page_base(scene) + a.first,
                scene_palette_base(scene) + a.first,
            );
            self.loaded_atlases += 1;
        }
        Ok(())
    }
    pub fn atlases_ready(&self) -> bool {
        if SCENE_GATE_LOAD {
            self.atlas_scene < SCENE_COUNT
                && self.loaded_atlases == SCENE_ATLAS_RANGES[self.atlas_scene].1 as usize
        } else {
            self.loaded_atlases == ATLASES.len()
        }
    }
    /// audio_probe.rs: read every chunk of scene group `scene` (modulo the
    /// catalogue) through the arena, checking nothing and admitting nothing.
    #[cfg(feature = "audio-probe")]
    pub fn probe_read_group(&mut self, scene: usize) -> Result<u32, LoadError> {
        self.init()?;
        let group = 1 + scene % SCENE_COUNT;
        let mut sectors = 0;
        for j in 0..group_len(group) {
            let e = self.entry(member(group, j))?;
            self.read(e, 0)?;
            sectors += e.sector_count;
        }
        Ok(sectors)
    }
    /// audio_probe.rs: one scene chunk at the far end of the scene groups.
    #[cfg(feature = "audio-probe")]
    pub fn probe_far_read(&mut self) -> Result<(), LoadError> {
        self.init()?;
        let e = self.entry(member(SCENE_COUNT, staged_len(SCENE_COUNT) - 2))?;
        self.read(e, 0)
    }
    /// Retry UI overwrites atlas VRAM with title art. Revoke all startup
    /// admissions before it runs, so the retry uploads every affected page.
    /// Exclusive &mut self ensures no Room/Scene view survives this reset.
    pub fn reset_bootstrap(&mut self) {
        prefetch_discard();
        cd_stream::cancel();
        while matches!(cd_stream::status(), cd_stream::Status::Busy) {
            core::hint::spin_loop();
        }
        if unsafe { DRIVE.music } {
            unsafe {
                DRIVE.music = false;
            }
            crate::music::read_done(false, true);
        }
        if unsafe { DRIVE.clip } {
            unsafe {
                DRIVE.clip = false;
            }
            crate::ambience::abort_prefetch();
            crate::ambience::prefetch_done(false);
        }
        if unsafe { DRIVE.pool } {
            unsafe {
                DRIVE.pool = false;
            }
            crate::modules::pool_landed(false);
        }
        self.loaded_scenes = 0;
        self.loaded_atlases = 0;
        self.selected = EMPTY;
        self.resident_scene = EMPTY;
        self.atlas_scene = EMPTY;
        self.coverage_scene = EMPTY;
        self.coverage_len = 0;
        self.revoke_metadata();
        self.effect_scene = EMPTY;
        #[cfg(feature = "intro")]
        {
            self.preloaded = PRELOAD_NONE;
        }
        unsafe {
            HK_COVERAGE_SCENE = 0;
            HK_COVERAGE_BYTES = 0;
        }
    }

    // Keep the large startup decoder outside main's bounded MIPS branch span.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    fn admit_scenes(&mut self, wanted: usize) -> Result<(), LoadError> {
        // Before any read or slide moves the prefetched bytes: finish a
        // background code install for this room, or drop one for another.
        crate::modules::gate_begin(wanted);
        self.prepare_runner_audio(wanted)?;
        unsafe {
            GATE_VERIFIED = psx_math::Mask64::EMPTY;
            GATE_SCENE = EMPTY;
        }
        if SCENE_GATE_LOAD && self.resident_scene != wanted {
            // &mut self excludes every outgoing Room/Scene view. Readiness is
            // revoked before any CD/decode/upload may overwrite those bytes.
            self.loaded_scenes = 0;
            self.resident_scene = EMPTY;
            self.selected = EMPTY;
        }
        if self.loaded_scenes == 0 {
            // Every decode below relocates its compressed payload to the end of
            // the arena it is given, so the metadata bank staged in the arena
            // tail cannot survive them: revoke it now and admit it last.
            self.revoke_metadata();
        }
        if self.loaded_scenes == 0 {
            // A clip stages at the arena front, under a prefetched tail.
            if crate::ambience::missing(SCENE_MANIFEST[wanted].scene_id) != 0 {
                prefetch_guard(AMBIENCE_STAGE_BYTES);
            }
            self.prepare_scene_ambience(wanted)?;
            self.prepare_map(wanted)?;
        }
        // What `preload` put in place before the opening movies took the arena is used once, by this
        // admission; the rest of the group is read chunk by chunk, as a group too large for the arena is.
        #[cfg(feature = "intro")]
        let (early, preloaded) = (
            self.early,
            core::mem::replace(&mut self.preloaded, PRELOAD_NONE) == wanted as u16,
        );
        #[cfg(not(feature = "intro"))]
        let (early, preloaded) = (false, false);
        let st = if early || preloaded {
            Stage::Read
        } else if self.loaded_scenes == 0 {
            self.stage_group(wanted)?
        } else {
            Stage::Read
        };
        // The code chunk heads the group, so it is consumed first, in disc order. The early pass leaves it:
        // the code installs into the pool, where the opening program is.
        if self.loaded_scenes == 0 && !early {
            self.prepare_code(wanted, st)?;
        }
        if self.loaded_scenes == 0 && !preloaded {
            self.prepare_scene_sfx(wanted, st)?;
        }
        self.prepare_coverage(wanted, st)?;
        self.prepare_effect_art(wanted, st)?;
        self.prepare_atlases(wanted, st)?;
        #[cfg(feature = "intro")]
        if early {
            self.preloaded = wanted as u16;
            return Ok(());
        }
        while self.loaded_scenes < if SCENE_GATE_LOAD { 1 } else { SCENE_COUNT } {
            let index = if SCENE_GATE_LOAD {
                wanted
            } else {
                self.loaded_scenes
            };
            let d = SCENE_MANIFEST[index];
            unsafe {
                HK_SCENE_READ_ID = index as u32 + 1;
            }
            let (end, input) = self.stage(index, st, d.ram_offset)?;
            // The group's last sectors (metadata) follow the scene chunk: once
            // they are in, the room's art reads while the scene decodes.
            if SCENE_GATE_LOAD
                && matches!(st, Stage::Tail { .. })
                && crate::modules::art_missing(wanted)
            {
                while matches!(cd_stream::status(), cd_stream::Status::Busy) {
                    checkpoint();
                }
                unsafe {
                    GROUP_LANDED = cd_stream::received();
                }
                crate::modules::art_begin(wanted);
            }
            unsafe {
                HK_SCENE_READ_ID = 0;
                HK_SCENE_DECODE_ID = index as u32 + 1;
            }
            let arena = unsafe {
                core::slice::from_raw_parts_mut(
                    (&raw mut BUFFERS.words).cast::<u8>().add(d.ram_offset),
                    end - d.ram_offset,
                )
            };
            let mut decoder = Decoder::new_scene(d.stored_len, d.stored_fnv, d.raw_len, d.raw_fnv)
                .at(input)
                .hashed(preverified(index));
            loop {
                checkpoint();
                unsafe {
                    HK_ROOM_DECODE_PHASE = decoder.phase_id();
                }
                crate::gate_probe::set(crate::gate_probe::decoder(decoder.phase_id()));
                match decoder.step(arena, DECODE_STEP) {
                    Ok(Some(len)) => {
                        if len != d.raw_len {
                            return Err(LoadError::RoomMismatch);
                        }
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        return Err(match error {
                            DecodeError::Checksum => LoadError::Checksum,
                            DecodeError::Decompress => LoadError::Decompress,
                            DecodeError::RoomFormat => LoadError::RoomFormat,
                        })
                    }
                }
            }
            crate::gate_probe::set(crate::gate_probe::SCENE_CHECK);
            let scene = unsafe { Scene::validated_view(&arena[..d.raw_len]) };
            if scene.id() != d.scene_id {
                return Err(LoadError::RoomMismatch);
            }
            if scene.draw_pool_count() != COVERAGE_MANIFEST[index].draw_pool_count as usize {
                return Err(LoadError::Coverage);
            }
            let (start, end) = SCENE_ATLAS_RANGES[index];
            let (start, end) = (start as usize, end as usize);
            let mut counts = [0; 2];
            for a in &ATLASES[start..end] {
                counts[a.kind as usize] += a.count;
            }
            if counts != [scene.page_count(), scene.palette_count()] {
                return Err(LoadError::RoomMismatch);
            }
            let mut mapped = 0;
            for (region, &(s, local)) in REGION_SCENE_LOCAL.iter().enumerate() {
                if s as usize == index {
                    mapped += 1;
                    if scene.chunk_id(local as usize) != Some(region + 1) {
                        return Err(LoadError::RoomMismatch);
                    }
                }
            }
            if mapped != scene.room_count() {
                return Err(LoadError::RoomMismatch);
            }
            self.resident_scene = index;
            self.loaded_scenes += 1;
            unsafe {
                HK_SCENE_LOADS = HK_SCENE_LOADS.saturating_add(1);
                HK_SCENE_DECODE_ID = 0;
            }
            // Joint mode preserves this immutable prefix during later reads.
            // Exclusive mode admits no second scene until the next gate.
        }
        // Last: the bank decodes inside the arena tail beyond the resident
        // scene, which no later decode touches until the next admission.
        self.prepare_metadata(wanted, st)?;
        unsafe {
            GATE_VERIFIED = psx_math::Mask64::EMPTY;
            GATE_SCENE = EMPTY;
        }

        // The room's art is part of the room: a gate does not end without it.
        if !crate::modules::admit_art(wanted) {
            // Never seen in the gates; the props it would draw stay hidden
            // (props.rs) and the art follows after the gate.
            unsafe {
                HK_MODULE_GATE_ART_MISSED = HK_MODULE_GATE_ART_MISSED.saturating_add(1);
            }
        }
        Ok(())
    }
    /// The part of the first scene's admission that does not need the scene arena for good, done while
    /// the opening poem plays (intro.rs): the bootstrap's sound banks, the scene's ambience, sound effects,
    /// coverage, effect art and atlases. The movies then take the arena, and the admission itself (`select`)
    /// finds this done and reads what is left: the scene, its metadata and its code. Every step checks what
    /// it reads as the admission does; a failure leaves the rest to it.
    #[cfg(feature = "intro")]
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    pub fn preload(&mut self, scene: usize) -> Result<(), LoadError> {
        self.prepare_ambience()?;
        if self.loaded_scenes != 0 {
            return Err(LoadError::RoomMismatch);
        }
        self.early = true;
        let result = self.admit_scenes(scene);
        self.early = false;
        result
    }
    /// Exclusive admission at title or a drained scene gate; spatial selection uses try_select().
    pub fn select(&mut self, region: usize) -> Result<(), LoadError> {
        if region >= REGION_SCENE_LOCAL.len() {
            return Err(LoadError::RoomMismatch);
        }
        mark(1, 0);
        bank_settle();
        ahead_gate_begin(scene_index(region));
        if let Err(error) = self.admit_scenes(scene_index(region)) {
            mark(3, error as u32);
            return Err(error);
        }
        // Admitted: the scene's placement stays in SHIFT_*, the ahead work is over.
        ahead().reset();
        self.try_select(region).and_then(|ready| {
            if ready {
                Ok(())
            } else {
                Err(LoadError::RoomMismatch)
            }
        })
    }
    pub fn try_select(&mut self, region: usize) -> Result<bool, LoadError> {
        if region >= REGION_SCENE_LOCAL.len() {
            return Err(LoadError::RoomMismatch);
        }
        if !self.is_ready(region) {
            return Ok(false);
        }
        if self.selected != region {
            unsafe {
                HK_REGION_ACTIVATIONS = HK_REGION_ACTIVATIONS.saturating_add(1);
                if self.selected != EMPTY {
                    HK_ROOM_CACHE_HITS = HK_ROOM_CACHE_HITS.saturating_add(1);
                }
            }
            self.selected = region;
        }
        unsafe {
            HK_ROOM_BYTES = SCENE_MANIFEST[scene_index(region)].raw_len as u32;
        }
        mark(2, 0);
        Ok(true)
    }
    /// Spatial requests never initiate I/O. A nonresident scene requires select().
    pub fn request(&mut self, region: usize) -> Result<bool, LoadError> {
        if region >= REGION_SCENE_LOCAL.len() {
            Err(LoadError::RoomMismatch)
        } else {
            Ok(self.is_ready(region))
        }
    }
    /// Plan the read for the latest prefetch hint: the hinted group's prefix
    /// that fits the arena tail the resident scene leaves free.
    pub fn pump(&mut self) -> Result<(), LoadError> {
        let p = prefetch_state();
        let scene = p.hint;
        if scene == EMPTY
            || !self.ready
            || self.loaded_scenes != 1
            || scene >= SCENE_COUNT
            || scene == self.resident_scene
            || !self.metadata_admitted()
        {
            return Ok(());
        }
        // A code-only prefetch whose code has since installed: plan the rest.
        // (Planning again whenever code took slack from data restarted reads
        // the gate then waited for: Crossroads_09 +43 vblanks on the tour.)
        let replan =
            p.scene == scene && p.state == Fetch::Ready && p.code_only && self.code_skip(scene) > 0;
        if p.scene == scene && p.state != Fetch::Idle && !replan {
            p.want = None;
            return Ok(());
        }
        // A read for a scene no longer expected is stopped; it lands as Idle.
        if p.state == Fetch::Reading {
            crate::gate_probe::event(10, p.scene as u32, scene as u32);
            cd_stream::cancel();
        }
        if p.want.is_some_and(|w| w.0 == scene) {
            return Ok(());
        }
        // The room's code goes straight into the pool (modules.rs) ahead of
        // any prefetch, even for a group the gate reads chunk by chunk.
        crate::modules::plan_code(scene);
        // What the gate will stage at the tail: the whole group, or the first
        // of its two windows. A group it reads chunk by chunk gets nothing,
        // because a prefetch of it would only be wasted drive time.
        let total = self.staged_prefix(scene);
        if total == 0 {
            return Ok(());
        }
        let resident = ((SCENE_MANIFEST[self.resident_scene].raw_len + 3) & !3) + self.metadata_len;
        // A room whose code is resident needs none of its code chunk. One
        // whose code is not gets its code chunk read on its own first, so the
        // modules install while the Knight walks and the group's prefix then
        // follows without it (modules.rs).
        let skip = self.code_skip(scene);
        let slack = ARENA_TOTAL.saturating_sub(resident) / SECTOR_BYTES;
        let code = crate::modules::code_chunk(scene)
            .map_or(0, |k| crate::modules::chunk_len(k).div_ceil(SECTOR_BYTES));
        // Reading the code chunk on its own first (CODE_FIRST) lets the
        // modules install sooner, but with many rooms carrying code it cost
        // the group prefix whenever the gate came quickly (well-drop: +16
        // vblanks), so the whole group is read with its code at its head.
        let code_only =
            CODE_FIRST && skip == 0 && code > 0 && code <= slack && !crate::modules::gave_up(scene);
        // A room of the other runner audio family: its bank rides below the prefix.
        let bank = bank_needed(scene);
        let slack = if bank > 0 && slack > bank { slack - bank } else if bank > 0 { 0 } else { slack };
        let sectors = if code_only {
            code
        } else {
            (total - skip).min(slack)
        };
        if sectors == 0 {
            return Ok(());
        }
        plan_bank(scene, sectors, bank);
        let destination = unsafe {
            (&raw mut BUFFERS.words)
                .cast::<u8>()
                .add(ARENA_TOTAL - sectors * SECTOR_BYTES)
                .cast::<u32>()
        };
        // The group's prefix: the chunks the gate decodes first, so the rest
        // can still be arriving while they decode.
        crate::gate_probe::event(2, scene as u32, sectors as u32 | (skip as u32) << 10);
        p.want = Some((
            scene,
            total,
            sectors,
            self.group_lba(scene) + skip as u32,
            destination,
            skip,
            code_only,
        ));
        Ok(())
    }
    pub fn prefetch_regions(&mut self, _regions: &[usize]) {}
    pub fn prefetch(&mut self, _region: usize) -> bool {
        false
    }
    pub fn protect_upload(&mut self, region: Option<usize>) -> bool {
        region.is_none_or(|r| self.is_ready(r))
    }
    pub fn pending(&self) -> Option<usize> {
        None
    }
    pub fn is_ready(&self, region: usize) -> bool {
        REGION_SCENE_LOCAL.get(region).is_some_and(|&(scene, _)| {
            let scene = scene as usize;
            if !crate::runner_audio::family_ready(crate::runner_audio::scene_is_mossman(SCENE_MANIFEST[scene].scene_id as usize)) {return false;}
            if SCENE_GATE_LOAD {
                self.loaded_scenes == 1
                    && scene == self.resident_scene
                    && self.atlas_scene == scene
                    && self.atlases_ready()
                    && self.coverage_scene == scene
                    && self.coverage_len == COVERAGE_MANIFEST[scene].raw_len
                    && self.metadata_scene == scene
                    && self.metadata_len == WORLD_META_MANIFEST[scene].raw_len
                    && self.effect_scene == scene
            } else {
                false
            }
        })
    }
    pub fn room_for(&self, region: usize) -> Option<Room<'_>> {
        if !self.is_ready(region) {
            return None;
        }
        let (index, local) = REGION_SCENE_LOCAL[region];
        self.scene(index as usize).room(local as usize)
    }
    fn scene(&self, index: usize) -> Scene<'_> {
        assert!(if SCENE_GATE_LOAD {
            self.loaded_scenes == 1 && index == self.resident_scene
        } else {
            index < self.loaded_scenes
        });
        let d = &SCENE_MANIFEST[index];
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&raw const BUFFERS.words).cast::<u8>().add(d.ram_offset),
                d.raw_len,
            )
        };
        unsafe { Scene::validated_view(bytes) }
    }
    pub fn coverage(&self) -> CoverageView<'_> {
        assert!(
            self.is_ready(self.selected),
            "No admitted coverage selected"
        );
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&raw const COVERAGE_BUFFER.words).cast::<u8>(),
                self.coverage_len,
            )
        };
        // Only exact checked bytes are copied here; &self ties the view to the
        // scene owner and prevents select/reset from reclaiming it while used.
        unsafe { CoverageView::validated_view(bytes) }
    }
    /// Return the checked world bank that belongs to the selected scene. The
    /// metadata buffer is immutable until the next exclusive admission, just
    /// like the scene and coverage views returned above.
    pub fn world_metadata(&self) -> WorldMeta<'_> {
        self.metadata_view
            .expect("No admitted world metadata selected")
    }
    /// True while a world bank is admitted and its bytes are immutable.
    pub fn metadata_admitted(&self) -> bool {
        self.metadata_view.is_some()
    }
    /// Bank index of a canonical region slot inside the admitted scene bank,
    /// or None while that scene's metadata is not the admitted one. Resolved
    /// once per region entry so per-tick trigger reads need no id scan.
    pub fn world_region_index(&self, region_id: usize) -> Option<usize> {
        let &(owner, _) = REGION_SCENE_LOCAL.get(region_id)?;
        let owner = owner as usize;
        if self.metadata_scene != owner
            || self.metadata_len != WORLD_META_MANIFEST.get(owner)?.raw_len
        {
            return None;
        }
        let bank = self.world_metadata();
        (0..bank.region_count()).find(|&index| {
            bank.region(index)
                .is_some_and(|r| r.global_id() as usize == region_id + 1)
        })
    }
    /// Resolve a spatial point through the admitted scene bank. `scene` is
    /// the catalogue scene id (as in `world::Region::scene`), not a manifest
    /// index. The returned index is the canonical global region slot used by
    /// the existing room and gameplay tables.
    pub fn locate(&self, scene: usize, x: i32, y: i32) -> Option<usize> {
        self.locate_box(scene, x, y).0
    }
    /// The view to draw with the camera at (x, y): one whose cooked camera
    /// range holds it. A view carries every sprite overlapping its own camera
    /// range widened by half a screen (host/cook.py), so drawing any other one
    /// leaves a strip of missing scenery at the screen edge, which is what a
    /// scene-wide camera lock or the follow damping did to the Knight's view.
    /// `keep` (the view drawn now) wins while it still holds the camera, then
    /// `knight` (the Knight's own view), then the first in bank order; when
    /// none holds it, the one it overshoots least. Views of `scene` only.
    #[inline(never)]
    pub fn camera_view(&self, scene: usize, x: i32, y: i32, keep: usize, knight: usize) -> usize {
        if !self.metadata_admitted() {
            return knight;
        }
        let bank = self.world_metadata();
        if bank.scene_id() as usize != scene {
            return knight;
        }
        let over = |c: [i32; 4]| (c[0] - x).max(x - c[2]).max(c[1] - y).max(y - c[3]).max(0);
        let ours = |id: usize| {
            id < REGION_SCENE_LOCAL.len()
                && crate::world::scene_of(id) == scene
                && self.is_ready(id)
        };
        let mut best = (i32::MAX, knight);
        for id in [keep, knight] {
            if !ours(id) {
                continue;
            }
            if let Some(r) = bank.region_by_global_id(id as u32 + 1) {
                let o = over(r.camera());
                if o == 0 {
                    return id;
                }
                if o < best.0 {
                    best = (o, id);
                }
            }
        }
        for region in bank.regions() {
            let Some(id) = (region.global_id() as usize).checked_sub(1) else {
                continue;
            };
            if !ours(id) {
                continue;
            }
            let o = over(region.camera());
            if o == 0 {
                return id;
            }
            if o < best.0 {
                best = (o, id);
            }
        }
        best.1
    }
    /// `locate`, answered from `located` while the point stays inside the box
    /// the last full scan returned for the same admitted bank and scene.
    #[inline(never)]
    pub fn locate_cached(
        &self,
        scene: usize,
        x: i32,
        y: i32,
        located: &mut Located,
    ) -> Option<usize> {
        let b = located.rect;
        if located.bank as usize == self.metadata_scene
            && located.scene as usize == scene
            && self.metadata_admitted()
            && x >= b[0]
            && x <= b[2]
            && y >= b[1]
            && y <= b[3]
        {
            return (located.slot != u16::MAX).then_some(located.slot as usize);
        }
        let (slot, rect) = self.locate_box(scene, x, y);
        *located = if self.metadata_admitted() {
            Located {
                bank: self.metadata_scene as u16,
                scene: scene as u16,
                slot: slot.map_or(u16::MAX, |s| s as u16),
                rect,
            }
        } else {
            Located::NONE
        };
        slot
    }
    /// The first region of the bank containing the point, in bank order, and
    /// a box around the point where every point has that same answer: inside
    /// the found region and outside every region before it (or every region,
    /// for no answer). A region the scene filter rejects never answers, so it
    /// does not shrink the box.
    #[inline(never)]
    fn locate_box(&self, scene: usize, x: i32, y: i32) -> (Option<usize>, [i32; 4]) {
        let point = [x, y, x, y];
        if !self.metadata_admitted() {
            return (None, point);
        }
        let bank = self.world_metadata();
        let mut rect = [i32::MIN, i32::MIN, i32::MAX, i32::MAX];
        if bank.scene_id() as usize != scene {
            return (None, rect);
        }
        let mut index = 0;
        while index < bank.region_count() {
            let Some(region) = bank.region(index) else {
                return (None, point);
            };
            let bounds = region.bounds();
            if x >= bounds[0] && x <= bounds[2] && y >= bounds[1] && y <= bounds[3] {
                let global = region.global_id() as usize;
                if global >= 1
                    && global <= REGION_SCENE_LOCAL.len()
                    && crate::world::scene_of(global - 1) == scene
                {
                    return (
                        Some(global - 1),
                        [
                            rect[0].max(bounds[0]),
                            rect[1].max(bounds[1]),
                            rect[2].min(bounds[2]),
                            rect[3].min(bounds[3]),
                        ],
                    );
                }
            } else {
                exclude(&mut rect, bounds, x, y);
            }
            index += 1;
        }
        (None, rect)
    }
    pub fn room(&self) -> Room<'_> {
        self.room_for(self.selected)
            .expect("No admitted scene selected")
    }
}

/// One actor's last `Cache::locate` answer and the box it holds in. Keyed by
/// the admitted bank's manifest index, whose bytes are the same every time
/// that scene is admitted, and by the scene asked about.
#[derive(Clone, Copy)]
pub struct Located {
    bank: u16,
    scene: u16,
    slot: u16,
    rect: [i32; 4],
}
impl Located {
    pub const NONE: Self = Self {
        bank: u16::MAX,
        scene: u16::MAX,
        slot: u16::MAX,
        rect: [0; 4],
    };
}
const _: () =
    assert!(REGION_SCENE_LOCAL.len() < u16::MAX as usize && SCENE_COUNT < u16::MAX as usize);
/// Shrink `rect` (which holds the point) until it misses the closed box `b`
/// (which does not), keeping the largest of the cuts that separate them.
fn exclude(rect: &mut [i32; 4], b: [i32; 4], x: i32, y: i32) {
    if b[0] > rect[2] || b[2] < rect[0] || b[1] > rect[3] || b[3] < rect[1] {
        return;
    }
    let area = |r: [i32; 4]| {
        ((r[2] as u32).wrapping_sub(r[0] as u32) >> 17)
            * ((r[3] as u32).wrapping_sub(r[1] as u32) >> 17)
    };
    let mut best: Option<[i32; 4]> = None;
    let mut keep = |r: [i32; 4]| {
        if best.is_none_or(|k| area(r) > area(k)) {
            best = Some(r);
        }
    };
    if x < b[0] {
        keep([rect[0], rect[1], rect[2].min(b[0] - 1), rect[3]]);
    }
    if x > b[2] {
        keep([rect[0].max(b[2] + 1), rect[1], rect[2], rect[3]]);
    }
    if y < b[1] {
        keep([rect[0], rect[1], rect[2], rect[3].min(b[1] - 1)]);
    }
    if y > b[3] {
        keep([rect[0], rect[1].max(b[3] + 1), rect[2], rect[3]]);
    }
    if let Some(r) = best {
        *rect = r;
    }
}

/// The admitted world bank for gameplay modules that only hold a catalogue
/// region. The view is retired by `State::begin_world_admission` before the
/// next exclusive admission overwrites the arena tail.
pub fn admitted_world_metadata() -> Option<WorldMeta<'static>> {
    let cache = unsafe { &*(&raw const crate::ROOM_CACHE) };
    cache.metadata_admitted().then(|| cache.world_metadata())
}

/// Largest ambience clip, rounded to sectors: what `prepare_scene_ambience`
/// may stage at the arena front.
const AMBIENCE_STAGE_BYTES: usize = clip_stage_bytes();
const fn clip_stage_bytes() -> usize {
    let mut i = 0;
    let mut max = 0;
    while i < CLIP_COUNT {
        let n = crate::ambience::CLIPS[i].byte_len.div_ceil(SECTOR_BYTES) * SECTOR_BYTES;
        if n > max {
            max = n;
        }
        i += 1;
    }
    max
}
/// Background read of the scene group the Knight is expected to enter next.
/// It lands in the arena tail the resident scene leaves free (its slack), so
/// it costs no RAM of its own: the whole group when it fits, else the group's
/// prefix (coverage, effect art, the first atlases), and the gate reads the
/// rest. The drive runs it between music refills; a gate consumes it.
#[derive(Clone, Copy, PartialEq)]
enum Fetch {
    Idle,
    Reading,
    Ready,
}
struct Prefetch {
    /// Manifest scene the gameplay side asked for (EMPTY: none).
    hint: usize,
    /// What is (being) staged: scene, group sectors, prefix sectors held.
    scene: usize,
    total: usize,
    sectors: usize,
    state: Fetch,
    /// Where the staged prefix starts, and a count bumped whenever those
    /// bytes may change, so a background code install can tell.
    base: *const u8,
    generation: u32,
    /// Leading group sectors the prefetch did not read (the code chunk of a
    /// room whose code was resident when it was planned).
    skip: usize,
    /// The read holds only the code chunk (the group follows once it installs).
    code_only: bool,
    /// The next read `pump` should start: scene, total, prefix sectors, LBA, destination.
    want: Option<(usize, usize, usize, u32, *mut u32, usize, bool)>,
    /// The runner audio bank the room's family needs when the SPU holds the other
    /// one, read ahead of the group into the free arena below it so the gate
    /// uploads it from RAM instead of reading it (and discarding the prefetch).
    bank: Bank,
    /// A partial prefix leaves the head parked at the first sector it lacks
    /// (see `maybe_park`): where that is, a park in flight, and the transfer
    /// count after the park (the head is still there while it has not moved).
    park_lba: u32,
    park_flight: bool,
    parked: u32,
}
#[derive(Clone, Copy)]
struct Bank {
    /// Read to start: destination, sectors, LBA.
    want: Option<(*mut u32, usize, u32)>,
    /// Manifest scene it was staged for (EMPTY: none), arena offset, byte length, landed whole.
    scene: usize,
    at: usize,
    sectors: usize,
    ok: bool,
    /// A bank read is the transfer in flight, and the plan it belongs to.
    flight: bool,
    seq: u32,
    flight_seq: u32,
}
const NO_BANK: Bank = Bank {
    want: None,
    scene: EMPTY,
    at: 0,
    sectors: 0,
    ok: false,
    flight: false,
    seq: 0,
    flight_seq: 0,
};
static mut PREFETCH: Prefetch = Prefetch {
    hint: EMPTY,
    scene: EMPTY,
    total: 0,
    sectors: 0,
    state: Fetch::Idle,
    base: core::ptr::null(),
    generation: 0,
    skip: 0,
    code_only: false,
    want: None,
    bank: NO_BANK,
    park_lba: 0,
    park_flight: false,
    parked: u32::MAX,
};
#[no_mangle]
pub static mut HK_PREFETCH_READS: u32 = 0;
/// Gates that took their runner audio bank from the prefetch's RAM copy.
#[no_mangle]
pub static mut HK_BANK_AHEAD_USED: u32 = 0;
/// Gates that read the runner audio bank themselves (and dropped the prefetch).
#[no_mangle]
pub static mut HK_BANK_GATE_READS: u32 = 0;
/// WORLD.PAK-relative LBA of the two runner audio banks (`AUDIO_BANKS` 2 and `MOSSMAN_BANK`).
static mut BANK_LBA: [u32; 2] = [0; 2];
/// Park the head at the first sector a partial prefix lacks, so the gate's read of
/// the rest pays no long seek (a music refill in between moves the head away; this
/// brings it back). Off with HK_NO_PARK.
const PARK_ON: bool = option_env!("HK_NO_PARK").is_none();
fn maybe_park() -> bool {
    let p = prefetch_state();
    if !PARK_ON
        || p.state != Fetch::Ready
        || p.scene == EMPTY
        || p.skip + p.sectors >= p.total
        || p.park_lba == 0
        || p.parked == cd_stream::starts()
    {
        return false;
    }
    crate::gate_probe::ctx(4);
    if cd_stream::park(p.park_lba).is_ok() {
        p.park_flight = true;
        unsafe {
            DRIVE.prefetch = true;
        }
        true
    } else {
        false
    }
}
/// Let a bank read or a park in flight land (the gate starts reads of its own).
fn bank_settle() {
    while (prefetch_state().bank.flight || prefetch_state().park_flight) && unsafe { DRIVE.prefetch } {
        crate::input::checkpoint();
        land_prefetch();
    }
}
/// Whether the gate into `scene` swaps the runner audio family.
pub fn bank_pending(scene: usize) -> bool {
    bank_needed(scene) > 0
}
/// Sectors of the runner audio bank `scene`'s family needs, 0 when the SPU already holds it.
fn bank_needed(scene: usize) -> usize {
    if option_env!("HK_NO_BANK_AHEAD").is_some() {
        return 0;
    }
    let m = crate::runner_audio::scene_is_mossman(SCENE_MANIFEST[scene].scene_id as usize);
    if crate::runner_audio::family_ready(m) {
        0
    } else {
        AUDIO_BANKS[if m { MOSSMAN_BANK } else { 2 }].0.div_ceil(SECTOR_BYTES)
    }
}
/// Plan the bank read for the group prefix of `sectors` just planned (the bank
/// goes right below it): `bank` sectors from `bank_needed`, 0 for none.
fn plan_bank(scene: usize, sectors: usize, bank: usize) {
    let p = prefetch_state();
    let b = &mut p.bank;
    b.want = None;
    b.ok = false;
    b.scene = EMPTY;
    b.seq = b.seq.wrapping_add(1);
    if bank == 0 || sectors == 0 {
        return;
    }
    let m = crate::runner_audio::scene_is_mossman(SCENE_MANIFEST[scene].scene_id as usize);
    let lba = unsafe { BANK_LBA[m as usize] };
    if lba == 0 {
        return;
    }
    b.at = ARENA_TOTAL - (sectors + bank) * SECTOR_BYTES;
    b.sectors = bank;
    b.scene = scene;
    let dest = unsafe { (&raw mut BUFFERS.words).cast::<u8>().add(b.at).cast::<u32>() };
    b.want = Some((dest, bank, lba));
}
#[no_mangle]
pub static mut HK_PREFETCH_SECTORS: u32 = 0;
/// Gate loads that found some of their group prefetched, and the sectors reused.
#[no_mangle]
pub static mut HK_PREFETCH_HITS: u32 = 0;
/// Gates that took over their room's prefetch while it was still reading.
#[no_mangle]
pub static mut HK_PREFETCH_ADOPTED: u32 = 0;
#[no_mangle]
pub static mut HK_PREFETCH_HIT_SECTORS: u32 = 0;
/// Gates that staged their scene's group in two windows (`Stage::Split`).
#[no_mangle]
pub static mut HK_SPLIT_GATES: u32 = 0;
/// Prefetches read and then dropped (the Knight went elsewhere).
#[no_mangle]
pub static mut HK_PREFETCH_WASTED: u32 = 0;
fn prefetch_state() -> &'static mut Prefetch {
    unsafe { &mut *(&raw mut PREFETCH) }
}
/// Gameplay's guess at the next scene (a manifest index). Cheap; the read is
/// planned in `Cache::pump` and started by the drive when it is free.
pub fn prefetch_hint(scene: usize) {
    if PREFETCH_ON {
        let p = prefetch_state();
        if p.hint != scene {
            crate::gate_probe::event(1, scene as u32, 0);
        }
        p.hint = scene;
    }
}
/// Study builds can turn the prefetch off (HK_NO_PREFETCH) to measure cold gates.
pub const PREFETCH_ON: bool = option_env!("HK_NO_PREFETCH").is_none();
/// Stored-hash checks of a prefetched group's chunks, done in the present
/// loop's idle time (main.rs `present_spin`) so the gate can skip them: each
/// chunk's bytes are hashed where they landed against the same checksum its
/// decoder would check, and a gate that uses that very prefetch starts those
/// decoders past their stored hash (room_decode.rs `hashed`).
/// `first`: where member `j` starts in the group, in sectors.
struct Verify {
    generation: u32,
    scene: usize,
    j: usize,
    first: usize,
    pos: usize,
    hash: u32,
    mask: psx_math::Mask64,
}
static mut VERIFY: Verify = Verify {
    generation: u32::MAX,
    scene: EMPTY,
    j: 0,
    first: 0,
    pos: 0,
    hash: 0x811c9dc5,
    mask: psx_math::Mask64::EMPTY,
};
impl Verify {
    fn next(&mut self, sectors: usize) {
        self.j += 1;
        self.first += sectors;
        self.pos = 0;
        self.hash = 0x811c9dc5;
    }
}
static mut GATE_VERIFIED: psx_math::Mask64 = psx_math::Mask64::EMPTY;
static mut GATE_SCENE: usize = EMPTY;
#[no_mangle]
pub static mut HK_PREVERIFIED_BYTES: u32 = 0;
#[no_mangle]
pub static mut HK_PREVERIFIED_CHUNKS: u32 = 0;
#[no_mangle]
pub static mut HK_GATE_PREVERIFIED: u32 = 0;
/// Chunk `index` of the scene being admitted was hashed ahead of the gate.
#[inline(never)]
fn preverified(index: usize) -> bool {
    let Some((g, j)) = group_of(index) else {
        return false;
    };
    let yes = g == 1 + unsafe { GATE_SCENE } && unsafe { GATE_VERIFIED }.test(j);
    if yes {
        unsafe {
            HK_GATE_PREVERIFIED = HK_GATE_PREVERIFIED.saturating_add(1);
        }
    }
    yes
}
/// Hash up to `budget` more bytes of the ready prefetch's chunks.
#[inline(never)]
pub fn preverify_step(mut budget: usize) {
    let p = prefetch_state();
    if p.state != Fetch::Ready || p.scene == EMPTY {
        return;
    }
    let v = unsafe { &mut *(&raw mut VERIFY) };
    if v.generation != p.generation || v.scene != p.scene {
        *v = Verify {
            generation: p.generation,
            scene: p.scene,
            j: 0,
            first: 0,
            pos: 0,
            hash: 0x811c9dc5,
            mask: psx_math::Mask64::EMPTY,
        };
    }
    let g = 1 + p.scene;
    let n = staged_len(g);
    while budget > 0 && v.j < n.min(64) {
        let Some((size, checksum, _)) = chunk_spec(member(g, v.j)) else {
            v.j = n;
            break;
        };
        // Hash member j only if the prefetch holds all of it.
        let sectors = size.div_ceil(SECTOR_BYTES);
        if v.first < p.skip || v.first + sectors > p.skip + p.sectors {
            v.next(sectors);
            continue;
        }
        let bytes = unsafe {
            core::slice::from_raw_parts(p.base.add((v.first - p.skip) * SECTOR_BYTES), size)
        };
        let m = (size - v.pos).min(budget);
        for &b in &bytes[v.pos..v.pos + m] {
            v.hash = (v.hash ^ u32::from(b)).wrapping_mul(0x01000193);
        }
        v.pos += m;
        budget -= m;
        unsafe {
            HK_PREVERIFIED_BYTES = HK_PREVERIFIED_BYTES.wrapping_add(m as u32);
        }
        if v.pos == size {
            if v.hash == checksum {
                v.mask.set(v.j);
                unsafe {
                    HK_PREVERIFIED_CHUNKS += 1;
                }
            }
            v.next(sectors);
        }
    }
}
/// Read an approached room's code chunk alone before its group (off: see `Cache::pump`).
const CODE_FIRST: bool = false;
/// The code sectors this admission skips and the group sectors after them a
/// prefetch left at the arena tail, consumed by this admission (zero when
/// there is none). A prefetch still reading is waited for when it is this
/// scene's and cancelled otherwise. `skip_now` is what the gate would skip
/// on its own; a prefetch that skipped code the room no longer has resident
/// is dropped.
fn prefetch_take(scene: usize, total: usize, skip_now: usize, adopt: bool) -> (usize, usize, bool) {
    bank_settle();
    let p = prefetch_state();
    crate::gate_probe::event(
        12,
        (p.state as u32) | (p.skip as u32) << 4 | ((p.scene == scene) as u32) << 14 | ((p.total == total) as u32) << 15,
        skip_now as u32 | (p.sectors as u32) << 8,
    );
    // A whole-group prefetch of this room still reading lands where the gate
    // stages (the arena's end), from the same sector: the gate takes it over.
    if adopt
        && p.state == Fetch::Reading
        && unsafe { DRIVE.prefetch }
        && p.scene == scene
        && p.total == total
        && !p.code_only
        && p.skip + p.sectors == total
        && (p.skip == 0 || skip_now > 0)
    {
        let skip = p.skip;
        unsafe {
            DRIVE.prefetch = false;
            GATE_VERIFIED = psx_math::Mask64::EMPTY;
            GATE_SCENE = scene;
            HK_PREFETCH_ADOPTED += 1;
        }
        p.state = Fetch::Idle;
        p.scene = EMPTY;
        p.want = None;
        p.hint = EMPTY;
        p.generation += 1;
        return (skip, 0, true);
    }
    if p.state == Fetch::Reading {
        if p.scene != scene {
            cd_stream::cancel();
        }
        while unsafe { DRIVE.prefetch } {
            crate::input::checkpoint();
            land_prefetch();
        }
    }
    let mut k = if p.state == Fetch::Ready && p.scene == scene && p.total == total {
        p.sectors
    } else {
        0
    };
    if k > 0 && p.skip > 0 && skip_now == 0 {
        k = 0;
    }
    let skip = if k > 0 { p.skip } else { skip_now };
    // The chunks the idle time already hashed, if this prefetch is the one used.
    let v = unsafe { &*(&raw const VERIFY) };
    unsafe {
        GATE_VERIFIED = if k > 0 && v.generation == p.generation && v.scene == scene {
            v.mask
        } else {
            psx_math::Mask64::EMPTY
        };
        GATE_SCENE = scene;
    }
    if k > 0 {
        unsafe {
            HK_PREFETCH_HITS += 1;
            HK_PREFETCH_HIT_SECTORS += k as u32;
        }
    } else if p.state == Fetch::Ready {
        unsafe {
            HK_PREFETCH_WASTED += 1;
        }
    }
    p.state = Fetch::Idle;
    p.scene = EMPTY;
    p.want = None;
    p.hint = EMPTY;
    p.generation += 1;
    (skip, k, false)
}
/// Drop a prefetch whose bytes the arena front up to `bytes` would overwrite.
fn prefetch_guard(bytes: usize) {
    bank_settle();
    let p = prefetch_state();
    if p.state == Fetch::Reading {
        while unsafe { DRIVE.prefetch } {
            crate::input::checkpoint();
            land_prefetch();
        }
    }
    if p.state == Fetch::Ready && ARENA_TOTAL - p.sectors * SECTOR_BYTES < bytes {
        p.state = Fetch::Idle;
        p.scene = EMPTY;
        p.generation += 1;
        unsafe {
            HK_PREFETCH_WASTED += 1;
        }
    }
}
/// Forget any prefetch (the tail is about to be reused).
fn prefetch_discard() {
    bank_settle();
    let p = prefetch_state();
    p.bank.want = None;
    p.bank.ok = false;
    p.bank.scene = EMPTY;
    if p.state == Fetch::Reading {
        cd_stream::cancel();
        while unsafe { DRIVE.prefetch } {
            crate::input::checkpoint();
            land_prefetch();
        }
    }
    p.state = Fetch::Idle;
    p.scene = EMPTY;
    p.want = None;
    p.hint = EMPTY;
    p.generation += 1;
}
/// The prefetch of a scene with a code chunk, as far as it has landed: the
/// scene, the generation, where its staged prefix (code chunk first) starts
/// and how many of its bytes are in.
pub fn code_prefetch() -> Option<(usize, u32, *const u8, usize)> {
    let p = prefetch_state();
    if p.scene == EMPTY || p.skip > 0 || crate::modules::code_chunk(p.scene).is_none() {
        return None;
    }
    let landed = match p.state {
        Fetch::Ready => p.sectors * SECTOR_BYTES,
        Fetch::Reading if unsafe { DRIVE.prefetch } => cd_stream::received() * SECTOR_BYTES,
        _ => return None,
    };
    Some((p.scene, p.generation, p.base, landed))
}
/// A prefetch read finished: it is usable only if every sector landed.
fn land_prefetch() {
    let d = unsafe { &mut *(&raw mut DRIVE) };
    if !d.prefetch {
        return;
    }
    let p = prefetch_state();
    if p.park_flight || p.bank.flight {
        // A side transfer (a runner audio bank read or a head park) is in flight.
        match cd_stream::status() {
            cd_stream::Status::Done => {
                d.prefetch = false;
                if p.bank.flight {
                    p.bank.ok = cd_stream::received() == p.bank.sectors
                        && p.bank.seq == p.bank.flight_seq;
                } else {
                    p.parked = cd_stream::starts();
                }
                p.bank.flight = false;
                p.park_flight = false;
            }
            cd_stream::Status::Failed(diag) => {
                d.prefetch = false;
                p.bank.flight = false;
                p.park_flight = false;
                unsafe {
                    HK_CD_DIAG = diag;
                }
            }
            cd_stream::Status::Idle | cd_stream::Status::Busy => {}
        }
        return;
    }
    match cd_stream::status() {
        cd_stream::Status::Done => {
            d.prefetch = false;
            crate::gate_probe::event(4, p.scene as u32, cd_stream::received() as u32);
            p.state = if cd_stream::received() == p.sectors {
                Fetch::Ready
            } else {
                Fetch::Idle
            };
            if p.state == Fetch::Idle {
                p.scene = EMPTY;
                p.generation += 1;
            }
        }
        cd_stream::Status::Failed(diag) => {
            d.prefetch = false;
            unsafe {
                HK_CD_DIAG = diag;
            }
            p.state = Fetch::Idle;
            p.scene = EMPTY;
            p.generation += 1;
        }
        cd_stream::Status::Idle | cd_stream::Status::Busy => {}
    }
}
fn start_prefetch() -> bool {
    let p = prefetch_state();
    // The room's runner audio bank first: 10 sectors, and the gate then has it.
    if p.want.is_some() {
        if let Some((destination, sectors, lba)) = p.bank.want.take() {
            crate::gate_probe::event(15, p.bank.scene as u32, sectors as u32);
            // Safety: the arena tail below the group prefix is free of the resident
            // scene and its metadata (Cache::pump checked the slack).
            if unsafe { cd_stream::start(destination, sectors, lba) }.is_ok() {
                p.bank.flight = true;
                p.bank.flight_seq = p.bank.seq;
                unsafe {
                    DRIVE.prefetch = true;
                }
                return true;
            }
            p.bank.scene = EMPTY;
        }
    }
    let Some((scene, total, sectors, lba, destination, skip, code_only)) = p.want.take() else {
        return false;
    };
    if p.state == Fetch::Ready && p.scene != scene {
        unsafe {
            HK_PREFETCH_WASTED += 1;
        }
    }
    p.scene = scene;
    p.total = total;
    p.sectors = sectors;
    p.park_lba = lba + sectors as u32;
    p.parked = u32::MAX;
    p.base = destination.cast::<u8>();
    p.generation += 1;
    p.skip = skip;
    p.code_only = code_only;
    // Safety: the tail from `destination` is free of the resident scene and its
    // metadata (Cache::pump checked the slack) until the next gate consumes it.
    crate::gate_probe::event(3, scene as u32, sectors as u32);
    match unsafe { cd_stream::start(destination, sectors, lba) } {
        Ok(()) => {
            p.state = Fetch::Reading;
            unsafe {
                DRIVE.prefetch = true;
                HK_PREFETCH_READS += 1;
                HK_PREFETCH_SECTORS += sectors as u32;
            }
            true
        }
        Err(diag) => {
            p.state = Fetch::Idle;
            p.scene = EMPTY;
            unsafe {
                HK_CD_DIAG = diag;
            }
            false
        }
    }
}

/// Spike `vram-ahead`: decode and upload the next scene's atlases while the Knight
/// is still in this one. The next scene's group sits compressed in the arena tail
/// (the prefetch). Each atlas chunk is decoded into the free gap between the
/// resident scene and that tail and uploaded to pages and palettes the resident
/// scene does not use, so the gate finds them in place and skips their decode and
/// upload. Only whole-group prefetches whose pages and palettes fit beside the
/// resident scene take part.
/// Off in the shipped disc (feature `vram-ahead` turns it on for the gate study): with the
/// entry hold overlapping the load's end, the load time it saves on a covered gate hides
/// under the hold, and it costs 3.4 KB of code in a build with 1.2 KB of headroom to spare.
pub const AHEAD_ON: bool = cfg!(feature = "vram-ahead");
/// Decoder steps run per call (each at most DECODE_STEP byte operations).
const AHEAD_STEPS: usize = match option_env!("HK_AHEAD_STEPS") {
    Some(s) => parse_usize(s),
    None => 4,
};
/// Highest page the ahead placement may use (page 18 is the quick map's).
const AHEAD_PAGE_LIMIT: usize = 18;
struct Ahead {
    scene: usize,
    ok: bool,
    started: bool,
    next: usize,
    end: usize,
    shift_page: u8,
    shift_pal: u16,
    done: [bool; 40],
    decoder: Option<Decoder>,
    pending_upload: bool,
    lo: usize,
}
static mut AHEAD: Ahead = Ahead {
    scene: EMPTY,
    ok: false,
    started: false,
    next: 0,
    end: 0,
    shift_page: 0,
    shift_pal: 0,
    done: [false; 40],
    decoder: None,
    pending_upload: false,
    lo: 0,
};
#[no_mangle]
pub static mut HK_AHEAD_CHUNKS: u32 = 0;
#[no_mangle]
pub static mut HK_AHEAD_SCENES: u32 = 0;
#[no_mangle]
pub static mut HK_AHEAD_REFUSED: u32 = 0;
#[no_mangle]
pub static mut HK_AHEAD_STEPS_RUN: u32 = 0;
/// Why a plan was refused: not a whole-group prefetch, pages, palettes, gap.
#[no_mangle]
pub static mut HK_AHEAD_WHY: [u32; 4] = [0; 4];
/// Atlas chunks the gates found already uploaded.
#[no_mangle]
pub static mut HK_AHEAD_SKIPPED: u32 = 0;
#[no_mangle]
pub static mut HK_AHEAD_GATES: u32 = 0;
fn ahead() -> &'static mut Ahead {
    unsafe { &mut *(&raw mut AHEAD) }
}
impl Ahead {
    fn reset(&mut self) {
        self.scene = EMPTY;
        self.ok = false;
        self.started = false;
        self.decoder = None;
        self.pending_upload = false;
        self.done = [false; 40];
    }
}
/// Whether atlas `idx` (an index into ATLASES) of `scene` was already decoded and uploaded.
fn ahead_done(scene: usize, idx: usize) -> bool {
    let a = ahead();
    AHEAD_ON && a.started && a.scene == scene && {
        let (start, _) = SCENE_ATLAS_RANGES[scene];
        idx >= start && idx - start < a.done.len() && a.done[idx - start]
    }
}
/// A gate for `wanted` begins: an ahead decode in flight is abandoned; finished chunks stay.
fn ahead_gate_begin(wanted: usize) {
    unsafe {
        HK_AHEAD_GATES += 1;
    }
    let a = ahead();
    a.decoder = None;
    a.pending_upload = false;
    if a.scene != wanted {
        if a.started {
            unsafe {
                SHIFT_PAGE[a.scene] = 0;
                SHIFT_PAL[a.scene] = 0;
            }
        }
        a.reset();
    }
}
impl Cache {
    /// One bounded slice of ahead work; call from idle time in the present loop.
    /// `upload`: the GPU path is free to take a page (the main loop, between frames);
    /// the present loop's idle time only decodes.
    pub fn ahead_step(&mut self, upload: bool) {
        if !AHEAD_ON {
            return;
        }
        let p = prefetch_state();
        let a = ahead();
        let cur = self.resident_scene;
        if self.loaded_scenes != 1 || cur >= SCENE_COUNT || !self.metadata_admitted() {
            return;
        }
        if p.state != Fetch::Ready || p.scene == EMPTY || p.scene == cur {
            if a.scene != EMPTY && !a.started {
                a.reset();
            }
            return;
        }
        let b = p.scene;
        if a.scene != b {
            if a.started && a.scene != cur {
                // The Knight's destination changed: what was uploaded is simply unused.
                unsafe {
                    SHIFT_PAGE[a.scene] = 0;
                    SHIFT_PAL[a.scene] = 0;
                }
            }
            a.reset();
            a.scene = b;
            a.ok = self.ahead_plan(a, p, cur, b);
            if !a.ok {
                unsafe {
                    HK_AHEAD_REFUSED += 1;
                }
            }
        }
        if !a.ok {
            return;
        }
        crate::gate_probe::ctx(1 + upload as u32);
        let gpu_busy = crate::render::dma_pending() || psx_rt::interrupts::gp1_queue_pending();
        if gpu_busy && upload {
            return;
        }
        let (start, _) = SCENE_ATLAS_RANGES[b];
        let base_off = (p.base as usize) - unsafe { (&raw const BUFFERS.words) as usize };
        for _ in 0..if upload { AHEAD_STEPS } else { 1 } {
            if a.next >= a.end {
                return;
            }
            let idx = a.next;
            let at = &ATLASES[idx];
            let first = match self.entry(ATLAS_INDEX + idx) {
                Ok(e) => (e.sector_offset - self.group_bases[1 + b]) as usize,
                Err(_) => {
                    a.ok = false;
                    return;
                }
            };
            let e = self.entry(ATLAS_INDEX + idx).ok().unwrap();
            let chunk = base_off + (first - p.skip) * SECTOR_BYTES;
            let end = chunk + e.byte_size as usize;
            let arena = unsafe {
                core::slice::from_raw_parts_mut(
                    (&raw mut BUFFERS.words).cast::<u8>().add(a.lo),
                    end - a.lo,
                )
            };
            if a.decoder.is_none() && !a.pending_upload {
                a.decoder = Some(
                    Decoder::new_bytes(at.stored_len, at.stored_fnv, at.raw_len, at.raw_fnv)
                        .at(chunk - a.lo),
                );
                a.started = true;
                unsafe {
                    SHIFT_PAGE[b] = a.shift_page;
                    SHIFT_PAL[b] = a.shift_pal;
                }
            }
            if let Some(d) = a.decoder.as_mut() {
                unsafe {
                    HK_AHEAD_STEPS_RUN += 1;
                }
                match d.step(arena, DECODE_STEP) {
                    Ok(Some(len)) if len == at.raw_len => {
                        a.decoder = None;
                        a.pending_upload = true;
                    }
                    Ok(Some(_)) | Err(_) => {
                        a.decoder = None;
                        a.ok = false;
                        return;
                    }
                    Ok(None) => continue,
                }
            }
            if a.pending_upload {
                if !upload || gpu_busy {
                    return;
                }
                crate::gate_probe::ctx(3);
                crate::vram_cache::upload_atlas_at(
                    at,
                    &arena[..at.raw_len],
                    scene_page_base(b) + at.first,
                    scene_palette_base(b) + at.first,
                );
                a.pending_upload = false;
                a.done[idx - start] = true;
                a.next += 1;
                unsafe {
                    HK_AHEAD_CHUNKS += 1;
                    if a.next >= a.end {
                        HK_AHEAD_SCENES += 1;
                    }
                }
                // One chunk's upload is this call's work.
                return;
            }
        }
    }
    /// Whether scene `b`, held whole in the prefetch, can be decoded ahead beside `cur`.
    fn ahead_plan(&self, a: &mut Ahead, p: &Prefetch, cur: usize, b: usize) -> bool {
        let why = |i: usize| unsafe { HK_AHEAD_WHY[i] += 1 };
        if !self.tail_fits(b) || p.skip + p.sectors != p.total || p.total != self.group_sectors(b) {
            why(0);
            return false;
        }
        let (start, end) = SCENE_ATLAS_RANGES[b];
        if end - start > a.done.len() {
            return false;
        }
        let (mut pb, mut cb) = (0usize, 0usize);
        for x in &ATLASES[start..end] {
            if x.kind == 0 {
                pb += x.count;
            } else {
                cb += x.count;
            }
        }
        let (mut pa, mut ca) = (0usize, 0usize);
        let (cs, ce) = SCENE_ATLAS_RANGES[cur];
        for x in &ATLASES[cs..ce] {
            if x.kind == 0 {
                pa += x.count;
            } else {
                ca += x.count;
            }
        }
        let (sa_p, sa_c) = unsafe { (SHIFT_PAGE[cur] as usize, SHIFT_PAL[cur] as usize) };
        let page = if sa_p >= pb {
            0
        } else if sa_p + pa + pb <= AHEAD_PAGE_LIMIT {
            AHEAD_PAGE_LIMIT - pb
        } else {
            why(1);
            return false;
        };
        let pal = if sa_c >= cb {
            0
        } else if sa_c + ca + cb <= hk_cache::residency::SCENE_CLUTS {
            hk_cache::residency::SCENE_CLUTS - cb
        } else {
            why(2);
            return false;
        };
        let resident = ((SCENE_MANIFEST[cur].raw_len + 3) & !3) + self.metadata_len;
        let mut base_off = (p.base as usize) - unsafe { (&raw const BUFFERS.words) as usize };
        // The runner audio bank, when one rides below the prefix, is the tail's floor.
        if p.bank.scene == b && p.bank.sectors > 0 {
            base_off = base_off.min(p.bank.at);
        }
        // The largest chunk decodes into the gap between the resident scene and the tail.
        let raw_max = ATLASES[start..end].iter().map(|x| x.raw_len).max().unwrap_or(0);
        if base_off < resident + raw_max + 64 {
            why(3);
            return false;
        }
        a.next = start;
        a.end = end;
        a.shift_page = page as u8;
        a.shift_pal = pal as u16;
        a.lo = resident;
        true
    }
}
/// The one drive. Room loads, area music refills and XA all go through
/// here, so no two of them ever overlap. A refill runs in the background from
/// the CD interrupt; a room read waits for one in flight to land, and during a
/// load the refill only runs between room reads if the music FIFO is running
/// low. XA holds the drive until the fight that asked for it ends.
struct Drive {
    installed: bool,
    music: bool,
    loading: bool,
    cdda: bool,
    prefetch: bool,
    clip: bool,
    pool: bool,
}
static mut DRIVE: Drive = Drive {
    installed: false,
    music: false,
    loading: false,
    cdda: false,
    prefetch: false,
    clip: false,
    pool: false,
};
/// WORLD.PAK-relative LBA of each package chunk (modules.rs), from the checked directory.
static mut PACKAGE_LBA: [u32; crate::modules::PACKAGE_CHUNKS] = [0; crate::modules::PACKAGE_CHUNKS];
/// The room the gameplay side expects next (prefetch hint, or the prefetch in flight).
pub fn predicted_scene() -> Option<usize> {
    let p = prefetch_state();
    if p.hint != EMPTY {
        Some(p.hint)
    } else if p.state != Fetch::Idle && p.scene != EMPTY {
        Some(p.scene)
    } else {
        None
    }
}
/// Wait for a pool read in flight to land (a gate needs the drive and the bytes).
pub fn pool_wait() {
    while unsafe { DRIVE.pool } {
        crate::input::checkpoint();
        land_pool();
    }
}
static mut POOL_SECTORS: usize = 0;
/// Stop a pool read (a gate does not wait for art, or for another room's code).
pub fn pool_cancel() {
    if !unsafe { DRIVE.pool } {
        return;
    }
    cd_stream::cancel();
    while unsafe { DRIVE.pool } {
        crate::input::checkpoint();
        land_pool();
    }
}
fn land_pool() {
    let d = unsafe { &mut *(&raw mut DRIVE) };
    if !d.pool {
        return;
    }
    match cd_stream::status() {
        cd_stream::Status::Done => {
            d.pool = false;
            crate::gate_probe::event(6, cd_stream::received() as u32, unsafe { POOL_SECTORS } as u32);
            crate::modules::pool_landed(cd_stream::received() == unsafe { POOL_SECTORS });
        }
        cd_stream::Status::Failed(diag) => {
            d.pool = false;
            unsafe {
                HK_CD_DIAG = diag;
            }
            crate::modules::pool_landed(false);
        }
        cd_stream::Status::Idle | cd_stream::Status::Busy => {}
    }
}
fn start_pool(code: bool) -> bool {
    let Some((destination, sectors, chunk)) = crate::modules::pool_want(code) else {
        return false;
    };
    crate::gate_probe::event(5, chunk as u32, sectors as u32);
    // Safety: the pool bytes are reserved for this read until it lands (modules.rs).
    match unsafe {
        cd_stream::start(
            destination,
            sectors,
            WORLD_PACK_DEFAULT_LBA + PACKAGE_LBA[chunk],
        )
    } {
        Ok(()) => {
            unsafe {
                DRIVE.pool = true;
                POOL_SECTORS = sectors;
            }
            true
        }
        Err(diag) => {
            unsafe {
                HK_CD_DIAG = diag;
            }
            crate::modules::pool_landed(false);
            false
        }
    }
}
#[no_mangle]
pub static mut HK_DRIVE_MUSIC_WAIT_TICKS: u32 = 0;
/// Sectors the gate's group read stored, once it is over and the drive has
/// started the room's art (usize::MAX while the group read is the last one).
static mut GROUP_LANDED: usize = usize::MAX;
/// Gates that ended without their room's art (a read or allocation failed).
#[no_mangle]
pub static mut HK_MODULE_GATE_ART_MISSED: u32 = 0;
/// Once per VBlank from `music::service`: land a finished refill, start the
/// next one when the drive is free.
pub fn pump() {
    let d = unsafe { &mut *(&raw mut DRIVE) };
    if !d.installed || d.cdda {
        return;
    }
    if d.music {
        match cd_stream::status() {
            cd_stream::Status::Done => {
                d.music = false;
                crate::music::read_done(true, d.loading);
            }
            cd_stream::Status::Failed(diag) => {
                d.music = false;
                unsafe {
                    HK_CD_DIAG = diag;
                }
                crate::music::read_done(false, d.loading);
            }
            cd_stream::Status::Idle | cd_stream::Status::Busy => {}
        }
        return;
    }
    if d.prefetch {
        land_prefetch();
        return;
    }
    if d.pool {
        land_pool();
        return;
    }
    if d.clip {
        match cd_stream::status() {
            cd_stream::Status::Done => {
                d.clip = false;
                crate::ambience::prefetch_done(true);
            }
            cd_stream::Status::Failed(diag) => {
                d.clip = false;
                unsafe {
                    HK_CD_DIAG = diag;
                }
                crate::ambience::prefetch_done(false);
            }
            cd_stream::Status::Idle | cd_stream::Status::Busy => {}
        }
        return;
    }
    // A room read outside a gate load (bootstrap, a probe) holds the
    // transfer without the load flag: leave it be.
    if d.loading || matches!(cd_stream::status(), cd_stream::Status::Busy) {
        return;
    }
    // Priorities: music that is not comfortably buffered, then the room
    // prefetch, then the next area's ambience clips, then ordinary music
    // top-ups. The room prefetch goes first because it only exists when a gate
    // is predicted within reach and saves the whole group read; the clips are
    // wanted from the moment a scene is entered, are small, and nearly always
    // land long before the Knight reaches another area's gate.
    if !crate::music::fifo_comfortable() {
        if let Some((destination, sectors, lba)) = crate::music::want_read(false) {
            start_music(destination, sectors, lba);
            return;
        }
    }
    // A room's code and art ahead of its gate, then its data: the pool reads
    // are small and need no arena slack, and art read here is a read command
    // the gate does not issue (a gate ends with its room's art).
    if start_pool(true) {
        return;
    }
    if start_pool(false) {
        return;
    }
    if start_prefetch() {
        return;
    }
    if let Some((destination, sectors, lba)) = crate::ambience::want_prefetch() {
        // Safety: the stage buffer is the clip prefetch's until prefetch_done.
        crate::gate_probe::event(8, sectors as u32, 0);
        match unsafe { cd_stream::start(destination, sectors, lba) } {
            Ok(()) => d.clip = true,
            Err(diag) => {
                unsafe {
                    HK_CD_DIAG = diag;
                }
                crate::ambience::prefetch_done(false);
            }
        }
        return;
    }
    if let Some((destination, sectors, lba)) = crate::music::want_read(false) {
        start_music(destination, sectors, lba);
        return;
    }
    maybe_park();
}
fn start_music(destination: *mut u32, sectors: usize, lba: u32) {
    crate::gate_probe::event(7, sectors as u32, 0);
    // Safety: the music FIFO owns these sectors until read_done.
    match unsafe { cd_stream::start(destination, sectors, lba) } {
        Ok(()) => unsafe {
            DRIVE.music = true;
        },
        Err(diag) => {
            unsafe {
                HK_CD_DIAG = diag;
            }
            crate::music::read_done(false, unsafe { DRIVE.loading });
        }
    }
}
/// Before every room read: let a refill in flight land, then make one the
/// music FIFO cannot wait for.
/// Start a pool read wanted now (a gate's own art, `sectors` long) as soon
/// as the drive is free; `pool_wait` lands it.
pub fn pool_read_begin(sectors: usize) {
    crate::gate_probe::set(crate::gate_probe::DRIVE);
    while matches!(cd_stream::status(), cd_stream::Status::Busy) {
        crate::input::checkpoint();
    }
    // A music refill here would cost two seeks for a read of a few sectors;
    // it waits for the gate's end unless music is short.
    if crate::music::outlasts_read(sectors) {
        cut_music();
        drive_idle();
    } else {
        room_drive();
    }
    crate::gate_probe::set(crate::gate_probe::CD);
    crate::gate_probe::read_started();
    start_pool(false);
}
/// The longest gate read that stops a music top-up in flight (`cut_music`).
const CUT_MUSIC_MAX: usize = 64;
/// Stop a music top-up in flight at a gate, keeping the sectors that landed
/// (the caller checked the buffered music outlasts the gate's read): the gate
/// does not wait for the rest of it, nor for its seek back.
fn cut_music() {
    if !unsafe { DRIVE.installed && DRIVE.music } {
        return;
    }
    cd_stream::cancel();
    // Ours from here: `pump` must not land it as a whole read (it does not
    // start another during a load).
    unsafe {
        DRIVE.music = false;
    }
    while matches!(cd_stream::status(), cd_stream::Status::Busy) {
        core::hint::spin_loop();
    }
    let landed = match cd_stream::status() {
        cd_stream::Status::Failed(_) => 0,
        _ => cd_stream::received(),
    };
    crate::music::read_cut(landed);
}
/// Wait for any background read to land, starting none.
fn drive_idle() {
    let d = unsafe { &raw mut DRIVE };
    while unsafe { (*d).installed && ((*d).music || (*d).prefetch || (*d).clip || (*d).pool) } {
        crate::input::checkpoint();
        land_prefetch();
        land_pool();
    }
}
fn room_drive() {
    let d = unsafe { &raw mut DRIVE };
    if !unsafe { (*d).installed } {
        return;
    }
    let begin = psx_rt::interrupts::vblank_count();
    while unsafe { (*d).music || (*d).prefetch || (*d).clip || (*d).pool } {
        crate::input::checkpoint();
        land_prefetch();
        land_pool();
    }
    if unsafe { (*d).loading } {
        if let Some((destination, sectors, lba)) = crate::music::want_read(true) {
            start_music(destination, sectors, lba);
            while unsafe { (*d).music } {
                crate::input::checkpoint();
            }
        }
    }
    unsafe {
        HK_DRIVE_MUSIC_WAIT_TICKS = HK_DRIVE_MUSIC_WAIT_TICKS
            .saturating_add(psx_rt::interrupts::vblank_count().wrapping_sub(begin));
    }
}
/// A scene gate's load holds the drive for room reads; XA stops first.
pub fn begin_load() {
    crate::music::release_cdda();
    crate::ambience::abort_prefetch();
    unsafe {
        DRIVE.loading = true;
    }
}
pub fn end_load() {
    unsafe {
        DRIVE.loading = false;
    }
}
/// XA may have the drive once no refill is in flight and no load holds it.
pub fn cdda_acquire() -> bool {
    let d = unsafe { &mut *(&raw mut DRIVE) };
    // A pool fetch still to start (the Shade's art ahead of a fight) goes first.
    if d.music || d.loading || d.prefetch || d.clip || d.pool || crate::modules::fetch_pending() {
        return false;
    }
    if d.installed {
        cd_stream::suspend();
    }
    d.cdda = true;
    true
}
pub fn cdda_release() {
    let d = unsafe { &mut *(&raw mut DRIVE) };
    if d.cdda && d.installed {
        cd_stream::resume();
    }
    d.cdda = false;
}
