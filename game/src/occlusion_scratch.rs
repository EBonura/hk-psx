//! CPU-only renderer scratch, in main RAM (see `bind`). No DMA, BIOS or
//! interrupt path may use it. Owner collection and drawing have exclusive lifetimes.
//! Drawing exposes disjoint field pointers, so nested hidden tests never borrow
//! the whole store while scenery pieces are live.
use super::occlusion::{
    tile_runs::{CELLS, HEIGHT},
    Pieces,
};
use core::mem::{size_of, ManuallyDrop, MaybeUninit};

pub const MAX_OCCLUDERS: usize = 8;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occluder {
    pub draw: u16,
    pub rect: [i16; 4],
    pub front: bool,
}

/// Dynamic quads are emitted after BACK and before FRONT. Only one later
/// FRONT rectangle may prove their entire clipped bounding box invisible.
/// This read-only test adds no scissor packets or scratch storage. Retain the
/// original draw for degenerate or signed-11-bit-wrapping coordinates.
#[inline(never)]
pub fn dynamic_hidden(verts: [(i16, i16); 4], occluders: &[Occluder]) -> bool {
    let (mut left, mut top, mut right, mut bottom) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    for (x, y) in verts {
        if (x as i32 + 1024) as u32 > 2047 || (y as i32 + 1024) as u32 > 2047 {
            return false;
        }
        left = left.min(x);
        right = right.max(x);
        top = top.min(y);
        bottom = bottom.max(y);
    }
    left = left.max(0);
    right = right.min(320);
    top = top.max(0);
    bottom = bottom.min(240);
    if left >= right || top >= bottom {
        return false;
    }
    occluders.iter().any(|o| {
        o.front
            && o.rect[0] <= left
            && o.rect[1] <= top
            && o.rect[2] >= right
            && o.rect[3] >= bottom
    })
}

#[repr(C)]
pub struct Occluders {
    entries: [MaybeUninit<Occluder>; MAX_OCCLUDERS],
    count: usize,
}
impl Occluders {
    pub fn clear(&mut self) {
        self.count = 0;
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn as_slice(&self) -> &[Occluder] {
        unsafe { core::slice::from_raw_parts(self.entries.as_ptr().cast(), self.count) }
    }
    /// Append or replace an initialized slot. Uninitialized entries are never
    /// exposed as Occluder: their bool byte need not yet have a valid value.
    pub fn set(&mut self, index: usize, value: Occluder) {
        assert!(index <= self.count && index < MAX_OCCLUDERS);
        self.entries[index].write(value);
        if index == self.count {
            self.count += 1;
        }
    }
}

#[repr(C)]
struct DrawStorage {
    hidden: Pieces,
    scenery: Pieces,
    holes: Pieces,
    occluders: Occluders,
    union: [i16; 4],
}
// Owners are consumed into TILE_EVENTS before any drawing. These two phases
// share bytes, but their typed references must never coexist. Rows survive the
// phase switch and are kept outside the overlay.
#[repr(C)]
union Workspace {
    draw: ManuallyDrop<DrawStorage>,
    owners: [u16; CELLS],
}
#[repr(C)]
pub struct Storage {
    workspace: Workspace,
    tile_rows: [u32; HEIGHT],
}
impl Storage {
    /// The caller owns this complete aligned reservation, with no live field
    /// references. Called before any frame's occlusion access, after boot and
    /// cache setup. Does not depend on scratchpad power-on contents.
    pub unsafe fn initialize_at(base: *mut Self) {
        Self::initialize_draw_at(base);
        Self::tile_rows(base).write_bytes(0, 1);
    }
    /// End the owner phase before calling this. All owner references must have
    /// expired; only the disjoint row array may remain live. Stale owner bytes
    /// are never interpreted as initialized rectangles or Occluder bools.
    pub unsafe fn initialize_draw_at(base: *mut Self) {
        Pieces::initialize_at(Self::hidden(base));
        Pieces::initialize_at(Self::scenery(base));
        Pieces::initialize_at(Self::holes(base));
        (&raw mut (*Self::occluders(base)).count).write(0);
        Self::union(base).write([i16::MAX, i16::MAX, i16::MIN, i16::MIN]);
    }
    /// Begin the owner phase with no live draw-workspace references.
    pub unsafe fn initialize_owners_at(base: *mut Self) {
        Self::tile_owners(base).write_bytes(0, 1);
    }
    unsafe fn draw(base: *mut Self) -> *mut DrawStorage {
        (&raw mut (*base).workspace.draw).cast()
    }
    pub unsafe fn hidden(base: *mut Self) -> *mut Pieces {
        &raw mut (*Self::draw(base)).hidden
    }
    pub unsafe fn scenery(base: *mut Self) -> *mut Pieces {
        &raw mut (*Self::draw(base)).scenery
    }
    pub unsafe fn holes(base: *mut Self) -> *mut Pieces {
        &raw mut (*Self::draw(base)).holes
    }
    pub unsafe fn occluders(base: *mut Self) -> *mut Occluders {
        &raw mut (*Self::draw(base)).occluders
    }
    pub unsafe fn union(base: *mut Self) -> *mut [i16; 4] {
        &raw mut (*Self::draw(base)).union
    }
    pub unsafe fn tile_rows(base: *mut Self) -> *mut [u32; HEIGHT] {
        &raw mut (*base).tile_rows
    }
    pub unsafe fn tile_owners(base: *mut Self) -> *mut [u16; CELLS] {
        &raw mut (*base).workspace.owners
    }
}

const _: () = assert!(size_of::<Occluder>() == 12);
// usize is native-sized in host tests; these assertions pin the guest ABI.
#[cfg(target_arch = "mips")]
const _: () = {
    assert!(size_of::<Pieces>() == 260);
    assert!(size_of::<DrawStorage>() == 888);
    assert!(core::mem::offset_of!(Storage, workspace) == 0);
    assert!(core::mem::offset_of!(Storage, tile_rows) == 888);
    assert!(size_of::<Storage>() == 948);
    assert!(core::mem::align_of::<Storage>() == 4);
};

/// The storage the renderer works in. It lived in the scratchpad until the
/// scenery passes moved their stack there (spstack.rs): their spills are the
/// hotter loads. `main` binds it once, in its own frame, before any frame.
#[cfg(target_arch = "mips")]
static mut BOUND: *mut Storage = core::ptr::null_mut();
/// # Safety
/// `storage` must stay valid and unaliased for the rest of the program.
#[cfg(target_arch = "mips")]
pub unsafe fn bind(storage: *mut Storage) {
    unsafe {
        BOUND = storage;
    }
}
#[cfg(target_arch = "mips")]
#[inline(always)]
pub fn base() -> *mut Storage {
    unsafe { BOUND }
}
