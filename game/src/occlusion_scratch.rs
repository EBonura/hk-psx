//! CPU-only renderer scratchpad lease. No DMA, BIOS or interrupt path may use
//! this reservation. Only disjoint field pointers are exposed, so a nested
//! hidden test never borrows the entire store while scenery pieces are live.
use core::mem::{size_of,MaybeUninit};
use super::occlusion::{Pieces,tile_runs::{CELLS,HEIGHT}};

pub const MAX_OCCLUDERS:usize=8;
#[repr(C)]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Occluder {pub draw:u16,pub rect:[i16;4],pub front:bool}

/// Dynamic quads are emitted after BACK and before FRONT. Only one later
/// FRONT rectangle may prove their entire clipped bounding box invisible.
/// This read-only test adds no scissor packets or scratch storage. Retain the
/// original draw for degenerate or signed-11-bit-wrapping coordinates.
#[inline(never)]
pub fn dynamic_hidden(verts:[(i16,i16);4],occluders:&[Occluder])->bool {
    let (mut left,mut top,mut right,mut bottom)=(i16::MAX,i16::MAX,i16::MIN,i16::MIN);
    for (x,y) in verts {
        if (x as i32+1024) as u32>2047 || (y as i32+1024) as u32>2047 {return false;}
        left=left.min(x);right=right.max(x);top=top.min(y);bottom=bottom.max(y);
    }
    left=left.max(0);right=right.min(320);top=top.max(0);bottom=bottom.min(240);
    if left>=right || top>=bottom {return false;}
    occluders.iter().any(|o|o.front && o.rect[0]<=left && o.rect[1]<=top && o.rect[2]>=right && o.rect[3]>=bottom)
}

#[repr(C)]
pub struct Occluders {entries:[MaybeUninit<Occluder>;MAX_OCCLUDERS],count:usize}
impl Occluders {
    pub fn clear(&mut self) {self.count=0;}
    pub fn len(&self)->usize {self.count}
    pub fn as_slice(&self)->&[Occluder] {
        unsafe {core::slice::from_raw_parts(self.entries.as_ptr().cast(),self.count)}
    }
    /// Append or replace an initialized slot. Uninitialized entries are never
    /// exposed as Occluder: their bool byte need not yet have a valid value.
    pub fn set(&mut self,index:usize,value:Occluder) {
        assert!(index<=self.count && index<MAX_OCCLUDERS);
        self.entries[index].write(value);
        if index==self.count {self.count+=1;}
    }
}

#[repr(C)]
pub struct Storage {
    hidden:Pieces,
    scenery:Pieces,
    holes:Pieces,
    occluders:Occluders,
    union:[i16;4],
    tile_owners:[u16;CELLS],
    tile_rows:[u32;HEIGHT],
}
impl Storage {
    /// The caller owns this complete aligned reservation, with no live field
    /// references. Called before any frame's occlusion access, after boot and
    /// cache setup. Does not depend on scratchpad power-on contents.
    pub unsafe fn initialize_at(base:*mut Self) {
        Pieces::initialize_at(Self::hidden(base));
        Pieces::initialize_at(Self::scenery(base));
        Pieces::initialize_at(Self::holes(base));
        (&raw mut (*base).occluders.count).write(0);
        Self::union(base).write([i16::MAX,i16::MAX,i16::MIN,i16::MIN]);
        // Owner0 proves nothing. Initialize this disjoint CPU-only array in
        // place, without a600-byte temporary or any DMA reference.
        Self::tile_owners(base).write_bytes(0,1);
        // Initialize before the collector forms a typed mutable row slice.
        Self::tile_rows(base).write_bytes(0,1);
    }
    pub unsafe fn hidden(base:*mut Self)->*mut Pieces {&raw mut (*base).hidden}
    pub unsafe fn scenery(base:*mut Self)->*mut Pieces {&raw mut (*base).scenery}
    pub unsafe fn holes(base:*mut Self)->*mut Pieces {&raw mut (*base).holes}
    pub unsafe fn occluders(base:*mut Self)->*mut Occluders {&raw mut (*base).occluders}
    pub unsafe fn union(base:*mut Self)->*mut [i16;4] {&raw mut (*base).union}
    pub unsafe fn tile_rows(base:*mut Self)->*mut [u32;HEIGHT] {&raw mut (*base).tile_rows}
    pub unsafe fn tile_owners(base:*mut Self)->*mut [u16;CELLS] {&raw mut (*base).tile_owners}
}

const _:()=assert!(size_of::<Occluder>()==12);
// usize is native-sized in host tests; these assertions pin the guest ABI.
#[cfg(target_arch="mips")]
const _:()={
    assert!(size_of::<Pieces>()==68);
    assert!(core::mem::offset_of!(Storage,tile_owners)==312);
    assert!(core::mem::offset_of!(Storage,tile_owners)%core::mem::align_of::<u16>()==0);
    assert!(core::mem::offset_of!(Storage,tile_rows)==912);
    assert!(core::mem::offset_of!(Storage,tile_rows)%core::mem::align_of::<u32>()==0);
    assert!(size_of::<Storage>()==972);
    assert!(core::mem::align_of::<Storage>()==4);
    assert!(size_of::<Storage>()<=psx_hw::memory::scratchpad::SIZE);
    assert!(psx_hw::memory::scratchpad::BASE as usize%core::mem::align_of::<Storage>()==0);
};

/// KUSEG physical scratchpad address; never derive a KSEG1 alias. The lease
/// contains CPU values only, and must never be submitted to a DMA channel.
#[cfg(target_arch="mips")]
#[inline(always)]
pub fn base()->*mut Storage {psx_hw::memory::scratchpad::BASE as *mut Storage}
