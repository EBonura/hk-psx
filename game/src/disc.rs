//! Startup-only resident scene loader. One aligned arena keeps every admitted
//! scene immutable for the session. Region selection creates O(1) local views;
//! no traversal request, prefetch or pump starts CD reads or decoding.
use hk_format::{Room,Scene};
use crate::room_decode::{Decoder,Error as DecodeError};
#[path="cd_stream.rs"] mod cd_stream;
use psx_pack::cd::{SectorReader,SECTOR_WORDS,WORLD_PACK_DEFAULT_LBA};
use psx_pack::{parse_entry,parse_header,PackEntry,SECTOR_BYTES};
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/scene_manifest.rs"));
pub const SCENE_COUNT:usize=SCENE_MANIFEST.len();
pub const ROOM_CAPACITY:usize=SCENE_ARENA_BYTES;
const MAX_CHUNKS:usize=256;
const EMPTY:usize=usize::MAX;
const SEEK_POLL:u32=4_000_000;
const EMPTY_ENTRY:PackEntry=PackEntry {chunk_id:0,sector_offset:0,sector_count:0,byte_size:0,checksum:0};
#[repr(C,align(4))]
struct Buffer {words:[u32;SCENE_ARENA_BYTES/4]}
// Preserve the linked symbol so build memory accounting measures actual bytes.
static mut BUFFERS:Buffer=Buffer {words:[0;SCENE_ARENA_BYTES/4]};
pub const fn scene_index(region:usize)->usize {REGION_SCENE_LOCAL[region].0}
pub const fn scene_page_base(scene:usize)->usize {SCENE_MANIFEST[scene].page_base}
pub const fn scene_palette_base(scene:usize)->usize {SCENE_MANIFEST[scene].palette_base}
/// Cooked largest opaque-black texel rectangle per scene texture (w==0: none).
pub fn scene_black_cores(scene:usize)->&'static [[u8;4]] {SCENE_BLACK_CORES[scene]}
pub const fn scene_region(scene:usize)->usize {SCENE_REGIONS[scene]}
pub struct Cache {
    entries:[PackEntry;MAX_CHUNKS],header:[u32;2048],reader:SectorReader,
    ready:bool,loaded_scenes:usize,selected:usize,
}
#[no_mangle] pub static mut HK_SCENE_LOADS:u32=0;
#[no_mangle] pub static mut HK_SCENE_READ_ID:u32=0;
#[no_mangle] pub static mut HK_SCENE_DECODE_ID:u32=0;
#[no_mangle] pub static mut HK_REGION_ACTIVATIONS:u32=0;
#[no_mangle] pub static mut HK_ROOM_STORED_HITS:u32=0;
#[no_mangle] pub static mut HK_ROOM_STORED_COMPLETIONS:u32=0;
#[no_mangle] pub static mut HK_ROOM_DECODE_PHASE:u32=11;
#[no_mangle] pub static mut HK_ROOM_READ_REGION:u32=0;
#[no_mangle] pub static mut HK_ROOM_DECODE_REGION:u32=0;
#[no_mangle]
pub static mut HK_CD_SECTORS_READ: u32 = 0;
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
}
fn mark(state: u32, error: u32) {
    unsafe {
        core::ptr::write_volatile(&raw mut HK_ROOM_LOAD_STATE, state);
        core::ptr::write_volatile(&raw mut HK_ROOM_LOAD_ERROR, error);
    }
}
fn sector(reader: &mut SectorReader, dst: &mut [u32]) -> bool {
    assert!(dst.len() == SECTOR_WORDS);
    for word in dst.iter_mut() {
        unsafe { core::ptr::write_volatile(word, 0) };
    }
    if !unsafe { reader.read_sector(dst.try_into().unwrap()) } {
        return false;
    }
    unsafe { HK_CD_SECTORS_READ = HK_CD_SECTORS_READ.saturating_add(1) };
    true
}
impl Cache {
    /// Safety: exactly one Cache owns BUFFERS and the CD controller.
    pub const unsafe fn new()->Self {
        Self {entries:[EMPTY_ENTRY;MAX_CHUNKS],header:[0;2048],reader:SectorReader::new(),
            ready:false,loaded_scenes:0,selected:EMPTY}
    }
    fn init(&mut self)->Result<(),LoadError> {
        if self.ready {return Ok(());}
        let chunks=SCENE_COUNT+crate::ambience::CLIPS.len();
        if SCENE_COUNT==0 || chunks>MAX_CHUNKS || SCENE_ARENA_BYTES%4!=0
            || SCENE_REGIONS.len()!=SCENE_COUNT {return Err(LoadError::RoomMismatch);}
        let mut offset=0usize;
        for (i,d) in SCENE_MANIFEST.iter().enumerate() {
            if d.ram_offset!=offset || d.arena_capacity!=SCENE_ARENA_BYTES.saturating_sub(offset)
                || d.raw_len==0 || d.raw_len>d.arena_capacity || d.stored_len==0
                || d.stored_len.div_ceil(SECTOR_BYTES)>d.arena_capacity/SECTOR_BYTES
                || SCENE_REGIONS[i]>=REGION_SCENE_LOCAL.len()
                || REGION_SCENE_LOCAL[SCENE_REGIONS[i]].0!=i {return Err(LoadError::RoomMismatch);}
            offset=offset.checked_add(d.raw_len).and_then(|n|n.checked_add(3)).ok_or(LoadError::RoomMismatch)?&!3;
        }
        if offset>SCENE_ARENA_BYTES || REGION_SCENE_LOCAL.iter().any(|&(s,_)|s>=SCENE_COUNT) {return Err(LoadError::RoomMismatch);}
        if !unsafe {self.reader.prepare_single_speed()} {return Err(LoadError::Prepare);}
        if !unsafe {self.reader.start_read_seek_first(WORLD_PACK_DEFAULT_LBA,SEEK_POLL)}
            || !sector(&mut self.reader,&mut self.header[..SECTOR_WORDS]) {return Err(LoadError::HeaderRead);}
        let first=unsafe {core::slice::from_raw_parts(self.header.as_ptr().cast::<u8>(),SECTOR_BYTES)};
        let h=parse_header(first).ok_or(LoadError::HeaderInvalid)?;
        let header_sectors=(28+chunks*24).div_ceil(SECTOR_BYTES);
        if h.chunk_count as usize!=chunks || h.header_sectors as usize!=header_sectors
            || h.table_bytes as usize!=chunks*24 || header_sectors>4 {return Err(LoadError::HeaderInvalid);}
        for i in 1..header_sectors {
            if !sector(&mut self.reader,&mut self.header[i*SECTOR_WORDS..(i+1)*SECTOR_WORDS]) {return Err(LoadError::HeaderRead);}
        }
        unsafe {self.reader.stop();}
        let bytes=unsafe {core::slice::from_raw_parts(self.header.as_ptr().cast::<u8>(),header_sectors*SECTOR_BYTES)};
        let mut next=header_sectors;
        for i in 0..chunks {
            let(size,checksum,capacity)=if i<SCENE_COUNT {
                let d=&SCENE_MANIFEST[i];(d.stored_len,d.stored_fnv,d.arena_capacity)
            } else {
                let clip=&crate::ambience::CLIPS[i-SCENE_COUNT];(clip.byte_len,clip.checksum,SCENE_ARENA_BYTES)
            };
            let e=parse_entry(bytes,i).ok_or(LoadError::HeaderInvalid)?;
            if size==0 || size.div_ceil(SECTOR_BYTES)>capacity/SECTOR_BYTES
                || e.chunk_id as usize!=i+1 || e.byte_size as usize!=size || e.checksum!=checksum
                || e.sector_offset as usize!=next || e.sector_count as usize!=size.div_ceil(SECTOR_BYTES) {
                return Err(LoadError::RoomMismatch);
            }
            next=next.checked_add(e.sector_count as usize).ok_or(LoadError::HeaderInvalid)?;
            self.entries[i]=e;
        }
        if next!=h.total_sectors as usize {return Err(LoadError::HeaderInvalid);}
        cd_stream::install();self.ready=true;Ok(())
    }
    /// The destination is a private suffix; terminal Done includes Pause so
    /// no IRQ can continue writing when decoding or SPU upload begins.
    fn read(&mut self,entry:PackEntry,offset:usize)->Result<(),LoadError> {
        if offset%4!=0 || offset>SCENE_ARENA_BYTES
            || entry.sector_count as usize>(SCENE_ARENA_BYTES-offset)/SECTOR_BYTES {return Err(LoadError::RoomMismatch);}
        let destination=unsafe {(&raw mut BUFFERS.words).cast::<u8>().add(offset).cast::<u32>()};
        if let Err(diag)=unsafe {cd_stream::start(destination,entry.sector_count as usize,WORLD_PACK_DEFAULT_LBA+entry.sector_offset)} {
            unsafe {HK_CD_DIAG=diag;}return Err(LoadError::PayloadRead);
        }
        loop {match cd_stream::status() {
            cd_stream::Status::Done=>return Ok(()),
            cd_stream::Status::Failed(diag)=>{unsafe {HK_CD_DIAG=diag;}return Err(LoadError::PayloadRead);}
            cd_stream::Status::Idle|cd_stream::Status::Busy=>core::hint::spin_loop(),
        }}
    }
    /// Bootstrap-only ambient loading reuses free RAM before any scene is
    /// admitted. Later calls are no-ops because all ambient samples live in SPU.
    pub fn prepare_ambience(&mut self)->Result<(),LoadError> {
        if crate::ambience::is_ready() {return Ok(());}
        if self.loaded_scenes!=0 {mark(3,LoadError::Ambience as u32);return Err(LoadError::Ambience);}
        mark(1,0);
        let result=(|| {
            self.init()?;crate::ambience::begin_load();
            for index in 0..crate::ambience::CLIPS.len() {
                let entry=self.entries[SCENE_COUNT+index];self.read(entry,0)?;
                let bytes=unsafe {core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>(),entry.byte_size as usize)};
                if !crate::ambience::upload(index,bytes) {return Err(LoadError::Ambience);}
            }
            if !crate::ambience::finish_load() {return Err(LoadError::Ambience);}
            Ok(())
        })();
        if let Err(error)=result {mark(3,error as u32);}result
    }
    // Keep the large startup decoder outside main's bounded MIPS branch span.
    #[inline(never)]
    fn admit_scenes(&mut self)->Result<(),LoadError> {
        self.init()?;
        while self.loaded_scenes<SCENE_COUNT {
            let index=self.loaded_scenes;let d=SCENE_MANIFEST[index];
            unsafe {HK_SCENE_READ_ID=index as u32+1;}
            self.read(self.entries[index],d.ram_offset)?;
            unsafe {HK_SCENE_READ_ID=0;HK_SCENE_DECODE_ID=index as u32+1;}
            let arena=unsafe {core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>().add(d.ram_offset),d.arena_capacity)};
            let mut decoder=Decoder::new_scene(d.stored_len,d.stored_fnv,d.raw_len,d.raw_fnv);
            loop {
                unsafe {HK_ROOM_DECODE_PHASE=decoder.phase_id();}
                match decoder.step(arena,1024) {
                    Ok(Some(len))=>{if len!=d.raw_len {return Err(LoadError::RoomMismatch);}break;}
                    Ok(None)=>{},
                    Err(error)=>return Err(match error {DecodeError::Checksum=>LoadError::Checksum,
                        DecodeError::Decompress=>LoadError::Decompress,DecodeError::RoomFormat=>LoadError::RoomFormat}),
                }
            }
            let scene=unsafe {Scene::validated_view(&arena[..d.raw_len])};
            if scene.id()!=d.scene_id {return Err(LoadError::RoomMismatch);}
            for (region,&(s,local)) in REGION_SCENE_LOCAL.iter().enumerate() {
                if s==index && scene.chunk_id(local)!=Some(region+1) {return Err(LoadError::RoomMismatch);}
            }
            self.loaded_scenes+=1;
            unsafe {HK_SCENE_LOADS=HK_SCENE_LOADS.saturating_add(1);HK_SCENE_DECODE_ID=0;}
            // Further reads/decompression use only the suffix after this raw
            // scene. Retry after a later failure cannot invalidate this prefix.
        }
        Ok(())
    }
    /// Title/retry admission only. Load both scenes once, then select a view.
    pub fn select(&mut self,region:usize)->Result<(),LoadError> {
        if region>=REGION_SCENE_LOCAL.len() {return Err(LoadError::RoomMismatch);}
        mark(1,0);
        if let Err(error)=self.admit_scenes() {mark(3,error as u32);return Err(error);}
        self.try_select(region).and_then(|ready|if ready {Ok(())}else{Err(LoadError::RoomMismatch)})
    }
    pub fn try_select(&mut self,region:usize)->Result<bool,LoadError> {
        if region>=REGION_SCENE_LOCAL.len() {return Err(LoadError::RoomMismatch);}
        if !self.is_ready(region) {return Ok(false);}
        if self.selected!=region {
            unsafe {HK_REGION_ACTIVATIONS=HK_REGION_ACTIVATIONS.saturating_add(1);
                if self.selected!=EMPTY {HK_ROOM_CACHE_HITS=HK_ROOM_CACHE_HITS.saturating_add(1);}}
            self.selected=region;
        }
        unsafe {HK_ROOM_BYTES=SCENE_MANIFEST[scene_index(region)].raw_len as u32;}
        mark(2,0);Ok(true)
    }
    /// No deferred request is created: all scene bytes are admitted at title.
    pub fn request(&mut self,region:usize)->Result<bool,LoadError> {
        if region>=REGION_SCENE_LOCAL.len() {Err(LoadError::RoomMismatch)}else{Ok(self.is_ready(region))}
    }
    pub fn pump(&mut self)->Result<(),LoadError> {Ok(())}
    pub fn prefetch_regions(&mut self,_regions:&[usize]) {}
    pub fn prefetch(&mut self,_region:usize)->bool {false}
    pub fn protect_upload(&mut self,region:Option<usize>)->bool {region.is_none_or(|r|self.is_ready(r))}
    pub fn pending(&self)->Option<usize> {None}
    pub fn is_ready(&self,region:usize)->bool {
        REGION_SCENE_LOCAL.get(region).is_some_and(|&(scene,_)|scene<self.loaded_scenes)
    }
    pub fn room_for(&self,region:usize)->Option<Room<'_>> {
        if !self.is_ready(region) {return None;}
        let(index,local)=REGION_SCENE_LOCAL[region];
        self.scene(index).room(local)
    }
    fn scene(&self,index:usize)->Scene<'_> {
        assert!(index<self.loaded_scenes);let d=&SCENE_MANIFEST[index];
        let bytes=unsafe {core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>().add(d.ram_offset),d.raw_len)};
        unsafe {Scene::validated_view(bytes)}
    }
    pub fn room(&self)->Room<'_> {self.room_for(self.selected).expect("No admitted scene selected")}
}
