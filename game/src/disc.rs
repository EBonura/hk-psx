//! Exclusive scene-gate loading or joint bootstrap residency. Each admitted
//! scene keeps immutable spatial views; only an exclusive select() at a drained,
//! blacked-out scene transition may reclaim the arena and replace static art.
use hk_format::{Room,Scene,WorldMeta};
use hk_format::coverage::{CoverageValidation,CoverageView,Expected as CoverageExpected};
use crate::room_decode::{Decoder,Error as DecodeError};
#[path="cd_stream.rs"] mod cd_stream;
use psx_pack::cd::{SectorReader,SECTOR_WORDS,WORLD_PACK_DEFAULT_LBA};
use psx_pack::{parse_entry,parse_header,PackEntry,SECTOR_BYTES};
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/scene_manifest.rs"));
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/scene_coverage_manifest.rs"));
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
#[repr(C,align(4))]
struct CoverageBuffer {words:[u32;COVERAGE_ARENA_BYTES/4]}
static mut COVERAGE_BUFFER:CoverageBuffer=CoverageBuffer {words:[0;COVERAGE_ARENA_BYTES/4]};
#[repr(C,align(4))]
struct WorldMetaBuffer {words:[u32;WORLD_META_ARENA_BYTES/4]}
static mut WORLD_META_BUFFER:WorldMetaBuffer=WorldMetaBuffer {words:[0;WORLD_META_ARENA_BYTES/4]};
#[no_mangle]pub static mut HK_COVERAGE_LOADS:u32=0;
#[no_mangle]pub static mut HK_COVERAGE_BYTES:u32=0;
#[no_mangle]pub static mut HK_COVERAGE_SCENE:u32=0;
fn atlas_fingerprint(scene:usize)->u32 {
    let (start,end)=SCENE_ATLAS_RANGES[scene];let mut hash=2166136261u32;
    for a in &ATLASES[start..end] {
        for value in [a.kind as u32,a.first as u32,a.count as u32,a.raw_len as u32,a.raw_fnv] {
            for byte in value.to_le_bytes() {hash=(hash^byte as u32).wrapping_mul(16777619);}
        }
    }
    hash
}
pub const fn scene_index(region:usize)->usize {REGION_SCENE_LOCAL[region].0}
pub const fn scene_page_base(scene:usize)->usize {SCENE_MANIFEST[scene].page_base}
pub const fn scene_palette_base(scene:usize)->usize {SCENE_MANIFEST[scene].palette_base}
/// Cooked largest opaque-black texel rectangle per scene texture (w==0: none).
pub fn scene_black_cores(scene:usize)->&'static [[u8;4]] {SCENE_BLACK_CORES[scene]}
/// Full-opacity occlusion only: colours remain textured, STP-clear overwrites.
pub fn scene_opaque_cores(scene:usize)->&'static [[u8;4]] {SCENE_OPAQUE_CORES[scene]}
pub fn scene_texture_flags(scene:usize,texture:usize)->u8 {
    (SCENE_TEXTURE_FLAGS[scene][texture/4]>>((texture%4)*2))&3
}
pub const fn scene_region(scene:usize)->usize {SCENE_REGIONS[scene]}
pub struct Cache {
    entries:[PackEntry;MAX_CHUNKS],header:[u32;2048],reader:SectorReader,
    ready:bool,loaded_scenes:usize,loaded_atlases:usize,selected:usize,
    resident_scene:usize,atlas_scene:usize,coverage_scene:usize,coverage_len:usize,
    metadata_scene:usize,metadata_len:usize,
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
    Coverage = 10,
    WorldMeta = 11,
}

/// Check a staged HKWMTA01 bank against the scene manifest before gameplay
/// receives any world object view.  The caller owns the backing bytes; this
/// function only returns a borrow after both the wire format and the
/// scene/raw-bank/fingerprint identities match.
pub fn checked_world_metadata<'a>(bytes:&'a [u8],scene:usize)->Result<WorldMeta<'a>,LoadError> {
    let expected=WORLD_META_MANIFEST.get(scene).ok_or(LoadError::WorldMeta)?;
    let bank=WorldMeta::parse(bytes).map_err(|_|LoadError::WorldMeta)?;
    if bank.check_identity(expected.scene_id,expected.raw_fnv,&expected.fingerprint).is_err() {
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
            ready:false,loaded_scenes:0,loaded_atlases:0,selected:EMPTY,resident_scene:EMPTY,atlas_scene:EMPTY,coverage_scene:EMPTY,coverage_len:0,metadata_scene:EMPTY,metadata_len:0}
    }
    fn init(&mut self)->Result<(),LoadError> {
        if self.ready {return Ok(());}
        let focus_index=SCENE_COUNT+crate::ambience::CLIPS.len()+ATLASES.len();
        let metadata_index=focus_index+1+COVERAGE_MANIFEST.len();
        let chunks=metadata_index+WORLD_META_MANIFEST.len();
        if !SCENE_GATE_LOAD || COVERAGE_MANIFEST.len()!=SCENE_COUNT || WORLD_META_MANIFEST.len()!=SCENE_COUNT
            || COVERAGE_ARENA_BYTES%4!=0 || WORLD_META_ARENA_BYTES%4!=0 || WORLD_META_ARENA_BYTES==0
            || COVERAGE_GRID_SHIFT!=2 || COVERAGE_GROUP_POOL_CAPACITY>4096 {return Err(LoadError::Coverage);}
        if SCENE_COUNT==0 || chunks>MAX_CHUNKS || SCENE_ARENA_BYTES%4!=0
            || SCENE_REGIONS.len()!=SCENE_COUNT {return Err(LoadError::RoomMismatch);}
        let mut offset=0usize;
        for (i,d) in SCENE_MANIFEST.iter().enumerate() {
            if SCENE_GATE_LOAD {offset=0;}
            if d.ram_offset!=offset || d.arena_capacity!=SCENE_ARENA_BYTES.saturating_sub(offset)
                || d.raw_len==0 || d.raw_len>d.arena_capacity || d.stored_len==0
                || d.stored_len.div_ceil(SECTOR_BYTES)>d.arena_capacity/SECTOR_BYTES
                || SCENE_REGIONS[i]>=REGION_SCENE_LOCAL.len()
                || REGION_SCENE_LOCAL[SCENE_REGIONS[i]].0!=i {return Err(LoadError::RoomMismatch);}
            offset=offset.checked_add(d.raw_len).and_then(|n|n.checked_add(3)).ok_or(LoadError::RoomMismatch)?&!3;
        }
        if offset>SCENE_ARENA_BYTES || REGION_SCENE_LOCAL.iter().any(|&(s,_)|s>=SCENE_COUNT) {return Err(LoadError::RoomMismatch);}
        // Each scene owns an exact contiguous atlas inventory. Exclusive
        // scene plans overlap physical slots only across different owners.
        if SCENE_ATLAS_RANGES.len()!=SCENE_COUNT {return Err(LoadError::RoomMismatch);}
        let(mut pages,mut palettes,mut previous,mut peak_pages,mut peak_palettes)=(0usize,0usize,0usize,0usize,0usize);
        for (scene,&(start,end)) in SCENE_ATLAS_RANGES.iter().enumerate() {
            if start!=previous || end<=start || end>ATLASES.len() {return Err(LoadError::RoomMismatch);}
            if SCENE_GATE_LOAD {pages=0;palettes=0;}
            let d=SCENE_MANIFEST[scene];
            if d.page_base!=pages || d.palette_base!=palettes {return Err(LoadError::RoomMismatch);}
            for a in &ATLASES[start..end] {
                let (next,unit)=match a.kind {0=>(&mut pages,32768usize),1=>(&mut palettes,32usize),_=>return Err(LoadError::RoomMismatch)};
                if a.scene_index!=scene || a.count==0 || a.first!=*next || a.count.checked_mul(unit)!=Some(a.raw_len)
                    || a.raw_len>SCENE_ARENA_BYTES || a.stored_len==0
                    || a.stored_len.div_ceil(SECTOR_BYTES)>SCENE_ARENA_BYTES/SECTOR_BYTES {
                    return Err(LoadError::RoomMismatch);
                }
                *next=next.checked_add(a.count).ok_or(LoadError::RoomMismatch)?;
            }
            if pages>hk_cache::residency::BANKS*hk_cache::residency::PAGES
                || palettes>hk_cache::residency::SCENE_CLUTS {return Err(LoadError::RoomMismatch);}
            peak_pages=peak_pages.max(pages);peak_palettes=peak_palettes.max(palettes);previous=end;
        }
        if previous!=ATLASES.len() || peak_pages!=SCENE_TOTAL_PAGES || peak_palettes!=SCENE_TOTAL_PALETTES {
            return Err(LoadError::RoomMismatch);
        }
        for (scene,c) in COVERAGE_MANIFEST.iter().enumerate() {
            let d=SCENE_MANIFEST[scene];
            if c.scene_id!=d.scene_id as u32 || c.scene_raw_fnv!=d.raw_fnv
                || c.atlas_fnv!=atlas_fingerprint(scene) || c.chunk_id!=focus_index+2+scene
                || c.draw_pool_count==0 || c.draw_pool_count as usize>COVERAGE_GROUP_POOL_CAPACITY
                || c.raw_len<80 || c.raw_len>COVERAGE_ARENA_BYTES || c.raw_len%4!=0
                || c.raw_len>SCENE_ARENA_BYTES || c.stored_len==0
                || c.stored_len.div_ceil(SECTOR_BYTES)>SCENE_ARENA_BYTES/SECTOR_BYTES {return Err(LoadError::Coverage);}
        }
        for (scene,m) in WORLD_META_MANIFEST.iter().enumerate() {
            let d=SCENE_MANIFEST[scene];
            if m.scene_id!=d.scene_id as u32 || m.raw_len<160 || m.raw_len>WORLD_META_ARENA_BYTES
                || m.raw_len%4!=0 || m.stored_len==0
                || m.stored_len.div_ceil(SECTOR_BYTES)>SCENE_ARENA_BYTES/SECTOR_BYTES
                || m.chunk_id!=metadata_index+scene+1 {return Err(LoadError::WorldMeta);}
        }
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
            } else if i<SCENE_COUNT+crate::ambience::CLIPS.len() {
                let clip=&crate::ambience::CLIPS[i-SCENE_COUNT];(clip.byte_len,clip.checksum,SCENE_ARENA_BYTES)
            } else if i<focus_index {
                let a=&ATLASES[i-SCENE_COUNT-crate::ambience::CLIPS.len()];
                (a.stored_len,a.stored_fnv,SCENE_ARENA_BYTES)
            } else if i==focus_index {(crate::focus_audio::BANK_BYTES,crate::focus_audio::BANK_CHECKSUM,SCENE_ARENA_BYTES)}
            else if i<metadata_index {let c=&COVERAGE_MANIFEST[i-focus_index-1];(c.stored_len,c.stored_fnv,SCENE_ARENA_BYTES)}
            else {let m=&WORLD_META_MANIFEST[i-metadata_index];(m.stored_len,m.stored_fnv,SCENE_ARENA_BYTES)};
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
        loop {crate::input::checkpoint();match cd_stream::status() {
            cd_stream::Status::Done=>return Ok(()),
            cd_stream::Status::Failed(diag)=>{unsafe {HK_CD_DIAG=diag;}return Err(LoadError::PayloadRead);}
            cd_stream::Status::Idle|cd_stream::Status::Busy=>core::hint::spin_loop(),
        }}
    }
    /// Bootstrap-only ambient loading reuses free RAM before any scene is
    /// admitted. Later calls are no-ops: all ambient samples live in SPU or
    /// the dedicated main-RAM stream cache, with no playback-time CD access.
    pub fn prepare_ambience(&mut self)->Result<(),LoadError> {
        if crate::ambience::is_ready() && crate::focus_audio::ready() {return Ok(());}
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
            if !crate::focus_audio::ready() {
                let entry=self.entries[SCENE_COUNT+crate::ambience::CLIPS.len()+ATLASES.len()];
                self.read(entry,0)?;
                let bytes=unsafe {core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>(),entry.byte_size as usize)};
                if !crate::focus_audio::upload(bytes) {return Err(LoadError::Ambience);}
            }
            Ok(())
        })();
        if let Err(error)=result {mark(3,error as u32);}result
    }
    /// Stages proofs only while no geometry view is admitted, then preserves
    /// their checked bytes in the separate scene-owned coverage arena.
    #[inline(never)]
    fn prepare_coverage(&mut self,scene:usize)->Result<(),LoadError> {
        self.init()?;let c=COVERAGE_MANIFEST[scene];
        if self.coverage_scene==scene && self.coverage_len==c.raw_len {return Ok(());}
        if self.loaded_scenes!=0 {return Err(LoadError::Coverage);}
        self.coverage_scene=EMPTY;self.coverage_len=0;
        unsafe {HK_COVERAGE_SCENE=0;HK_COVERAGE_BYTES=0;}
        self.read(self.entries[c.chunk_id-1],0)?;
        let arena=unsafe {core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(),SCENE_ARENA_BYTES)};
        let mut decoder=Decoder::new_bytes(c.stored_len,c.stored_fnv,c.raw_len,c.raw_fnv);
        loop {
            crate::input::checkpoint();
            unsafe {HK_ROOM_DECODE_PHASE=decoder.phase_id();}
            match decoder.step(arena,1024) {
                Ok(Some(len))=>{if len!=c.raw_len{return Err(LoadError::Coverage);}break;},
                Ok(None)=>{},
                Err(error)=>return Err(match error {DecodeError::Checksum=>LoadError::Checksum,
                    DecodeError::Decompress=>LoadError::Decompress,DecodeError::RoomFormat=>LoadError::Coverage}),
            }
        }
        let expected=CoverageExpected {scene_id:c.scene_id,scene_raw_fnv:c.scene_raw_fnv,
            atlas_fnv:c.atlas_fnv,draw_pool_count:c.draw_pool_count};
        let mut check=CoverageValidation::new(&arena[..c.raw_len],expected).map_err(|_|LoadError::Coverage)?;
        loop {crate::input::checkpoint();if check.step(32).map_err(|_|LoadError::Coverage)?{break;}}
        check.finish().map_err(|_|LoadError::Coverage)?;
        let destination=unsafe {core::slice::from_raw_parts_mut((&raw mut COVERAGE_BUFFER.words).cast::<u8>(),c.raw_len)};
        for (source,destination) in arena[..c.raw_len].chunks(1024).zip(destination.chunks_mut(1024)) {
            crate::input::checkpoint();destination.copy_from_slice(source);
        }
        self.coverage_scene=scene;self.coverage_len=c.raw_len;
        unsafe {HK_COVERAGE_LOADS=HK_COVERAGE_LOADS.saturating_add(1);HK_COVERAGE_BYTES=c.raw_len as u32;HK_COVERAGE_SCENE=scene as u32+1;}
        Ok(())
    }
    /// Stage and validate the immutable HKWMTA bank before any scene object
    /// view is published.  Metadata lives in its own buffer because the scene
    /// decoder reuses BUFFERS for every CD read and decompression pass.
    #[inline(never)]
    fn prepare_metadata(&mut self,scene:usize)->Result<(),LoadError> {
        self.init()?;let m=WORLD_META_MANIFEST[scene];
        if self.metadata_scene==scene && self.metadata_len==m.raw_len {return Ok(());}
        if self.loaded_scenes!=0 {return Err(LoadError::WorldMeta);}
        self.metadata_scene=EMPTY;self.metadata_len=0;
        self.read(self.entries[m.chunk_id-1],0)?;
        let arena=unsafe {core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(),SCENE_ARENA_BYTES)};
            let mut decoder=Decoder::new_bytes(m.stored_len,m.stored_fnv,m.raw_len,m.bank_fnv);
        loop {
            crate::input::checkpoint();
            match decoder.step(arena,1024) {
                Ok(Some(len))=>{if len!=m.raw_len{return Err(LoadError::WorldMeta);}break;},
                Ok(None)=>{},
                Err(error)=>return Err(match error {DecodeError::Checksum=>LoadError::Checksum,
                    DecodeError::Decompress=>LoadError::Decompress,DecodeError::RoomFormat=>LoadError::WorldMeta}),
            }
        }
        let destination=unsafe {core::slice::from_raw_parts_mut((&raw mut WORLD_META_BUFFER.words).cast::<u8>(),m.raw_len)};
        for (source,destination) in arena[..m.raw_len].chunks(1024).zip(destination.chunks_mut(1024)) {
            crate::input::checkpoint();destination.copy_from_slice(source);
        }
        let bank=checked_world_metadata(destination,scene)?;
        let mut mapped=0;
        for region in bank.regions() {
            let region=region;
            let global=region.global_id() as usize;
            if global==0 || global>REGION_SCENE_LOCAL.len() || REGION_SCENE_LOCAL[global-1].0!=scene {
                return Err(LoadError::WorldMeta);
            }
            mapped+=1;
        }
        if mapped!=REGION_SCENE_LOCAL.iter().filter(|&&(owner,_)|owner==scene).count() {
            return Err(LoadError::WorldMeta);
        }
        self.metadata_scene=scene;self.metadata_len=m.raw_len;Ok(())
    }
    /// Exclusive atlas admission. No Scene/Room view may exist while this
    /// buffer is scratch; completed chunks remain in VRAM across retries.
    #[inline(never)]
    fn prepare_atlases(&mut self,scene:usize)->Result<(),LoadError> {
        self.init()?;
        if self.atlases_ready() && (!SCENE_GATE_LOAD || self.atlas_scene==scene) {return Ok(());}
        if self.loaded_scenes!=0 {return Err(LoadError::RoomMismatch);}
        let (start,end)=if SCENE_GATE_LOAD {SCENE_ATLAS_RANGES[scene]}else{(0,ATLASES.len())};
        if self.atlas_scene!=scene {self.loaded_atlases=start;self.atlas_scene=scene;}
        while self.loaded_atlases<end {
            let a=&ATLASES[self.loaded_atlases];
            let entry=self.entries[SCENE_COUNT+crate::ambience::CLIPS.len()+self.loaded_atlases];
            self.read(entry,0)?;
            let arena=unsafe {core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(),SCENE_ARENA_BYTES)};
            let mut decoder=Decoder::new_bytes(a.stored_len,a.stored_fnv,a.raw_len,a.raw_fnv);
            loop {
                crate::input::checkpoint();
                unsafe {HK_ROOM_DECODE_PHASE=decoder.phase_id();}
                match decoder.step(arena,1024) {
                    Ok(Some(len))=>{if len!=a.raw_len {return Err(LoadError::RoomMismatch);}break;}
                    Ok(None)=>{},
                    Err(error)=>return Err(match error {DecodeError::Checksum=>LoadError::Checksum,
                        DecodeError::Decompress=>LoadError::Decompress,DecodeError::RoomFormat=>LoadError::RoomFormat}),
                }
            }
            crate::vram_cache::upload_atlas(a,&arena[..a.raw_len]);
            self.loaded_atlases+=1;
        }
        Ok(())
    }
    pub fn atlases_ready(&self)->bool {
        if SCENE_GATE_LOAD {self.atlas_scene<SCENE_COUNT && self.loaded_atlases==SCENE_ATLAS_RANGES[self.atlas_scene].1}
        else {self.loaded_atlases==ATLASES.len()}
    }
    /// Retry UI overwrites atlas VRAM with title art. Revoke all startup
    /// admissions before it runs, so the retry uploads every affected page.
    /// Exclusive &mut self ensures no Room/Scene view survives this reset.
    pub fn reset_bootstrap(&mut self) {
        cd_stream::cancel();
        while matches!(cd_stream::status(),cd_stream::Status::Busy) {core::hint::spin_loop();}
        self.loaded_scenes=0;self.loaded_atlases=0;self.selected=EMPTY;self.resident_scene=EMPTY;self.atlas_scene=EMPTY;
        self.coverage_scene=EMPTY;self.coverage_len=0;self.metadata_scene=EMPTY;self.metadata_len=0;
        unsafe {HK_COVERAGE_SCENE=0;HK_COVERAGE_BYTES=0;}
    }

    // Keep the large startup decoder outside main's bounded MIPS branch span.
    #[inline(never)]
    fn admit_scenes(&mut self,wanted:usize)->Result<(),LoadError> {
        if SCENE_GATE_LOAD && self.resident_scene!=wanted {
            // &mut self excludes every outgoing Room/Scene view. Readiness is
            // revoked before any CD/decode/upload may overwrite those bytes.
            self.loaded_scenes=0;self.resident_scene=EMPTY;self.selected=EMPTY;
        }
        self.prepare_metadata(wanted)?;
        self.prepare_coverage(wanted)?;
        self.prepare_atlases(wanted)?;
        while self.loaded_scenes<if SCENE_GATE_LOAD {1}else{SCENE_COUNT} {
            let index=if SCENE_GATE_LOAD {wanted}else{self.loaded_scenes};let d=SCENE_MANIFEST[index];
            unsafe {HK_SCENE_READ_ID=index as u32+1;}
            self.read(self.entries[index],d.ram_offset)?;
            unsafe {HK_SCENE_READ_ID=0;HK_SCENE_DECODE_ID=index as u32+1;}
            let arena=unsafe {core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>().add(d.ram_offset),d.arena_capacity)};
            let mut decoder=Decoder::new_scene(d.stored_len,d.stored_fnv,d.raw_len,d.raw_fnv);
            loop {
                crate::input::checkpoint();
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
            if scene.draw_pool_count()!=COVERAGE_MANIFEST[index].draw_pool_count as usize {return Err(LoadError::Coverage);}
            let (start,end)=SCENE_ATLAS_RANGES[index];
            let mut counts=[0;2];
            for a in &ATLASES[start..end] {counts[a.kind as usize]+=a.count;}
            if counts!=[scene.page_count(),scene.palette_count()] {return Err(LoadError::RoomMismatch);}
            let mut mapped=0;
            for (region,&(s,local)) in REGION_SCENE_LOCAL.iter().enumerate() {
                if s==index {
                    mapped+=1;
                    if scene.chunk_id(local)!=Some(region+1) {return Err(LoadError::RoomMismatch);}
                }
            }
            if mapped!=scene.room_count() {return Err(LoadError::RoomMismatch);}
            self.resident_scene=index;self.loaded_scenes+=1;
            unsafe {HK_SCENE_LOADS=HK_SCENE_LOADS.saturating_add(1);HK_SCENE_DECODE_ID=0;}
            // Joint mode preserves this immutable prefix during later reads.
            // Exclusive mode admits no second scene until the next gate.
        }
        Ok(())
    }
    /// Exclusive admission at title or a drained scene gate; spatial selection uses try_select().
    pub fn select(&mut self,region:usize)->Result<(),LoadError> {
        if region>=REGION_SCENE_LOCAL.len() {return Err(LoadError::RoomMismatch);}
        mark(1,0);
        if let Err(error)=self.admit_scenes(scene_index(region)) {mark(3,error as u32);return Err(error);}
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
    /// Spatial requests never initiate I/O. A nonresident scene requires select().
    pub fn request(&mut self,region:usize)->Result<bool,LoadError> {
        if region>=REGION_SCENE_LOCAL.len() {Err(LoadError::RoomMismatch)}else{Ok(self.is_ready(region))}
    }
    pub fn pump(&mut self)->Result<(),LoadError> {Ok(())}
    pub fn prefetch_regions(&mut self,_regions:&[usize]) {}
    pub fn prefetch(&mut self,_region:usize)->bool {false}
    pub fn protect_upload(&mut self,region:Option<usize>)->bool {region.is_none_or(|r|self.is_ready(r))}
    pub fn pending(&self)->Option<usize> {None}
    pub fn is_ready(&self,region:usize)->bool {
        REGION_SCENE_LOCAL.get(region).is_some_and(|&(scene,_)|
            if SCENE_GATE_LOAD {self.loaded_scenes==1 && scene==self.resident_scene && self.atlas_scene==scene && self.atlases_ready()
                && self.coverage_scene==scene && self.coverage_len==COVERAGE_MANIFEST[scene].raw_len
                && self.metadata_scene==scene && self.metadata_len==WORLD_META_MANIFEST[scene].raw_len}else{false})
    }
    pub fn room_for(&self,region:usize)->Option<Room<'_>> {
        if !self.is_ready(region) {return None;}
        let(index,local)=REGION_SCENE_LOCAL[region];
        self.scene(index).room(local)
    }
    fn scene(&self,index:usize)->Scene<'_> {
        assert!(if SCENE_GATE_LOAD {self.loaded_scenes==1&&index==self.resident_scene}else{index<self.loaded_scenes});let d=&SCENE_MANIFEST[index];
        let bytes=unsafe {core::slice::from_raw_parts((&raw const BUFFERS.words).cast::<u8>().add(d.ram_offset),d.raw_len)};
        unsafe {Scene::validated_view(bytes)}
    }
    pub fn coverage(&self)->CoverageView<'_> {
        assert!(self.is_ready(self.selected),"No admitted coverage selected");
        let bytes=unsafe {core::slice::from_raw_parts((&raw const COVERAGE_BUFFER.words).cast::<u8>(),self.coverage_len)};
        // Only exact checked bytes are copied here; &self ties the view to the
        // scene owner and prevents select/reset from reclaiming it while used.
        unsafe {CoverageView::validated_view(bytes)}
    }
    /// Return the checked world bank that belongs to the selected scene. The
    /// metadata buffer is immutable until the next exclusive admission, just
    /// like the scene and coverage views returned above.
    pub fn world_metadata(&self)->WorldMeta<'_> {
        assert!(self.metadata_scene<SCENE_COUNT && self.metadata_len==WORLD_META_MANIFEST[self.metadata_scene].raw_len,
            "No admitted world metadata selected");
        let bytes=unsafe {core::slice::from_raw_parts((&raw const WORLD_META_BUFFER.words).cast::<u8>(),self.metadata_len)};
        checked_world_metadata(bytes,self.metadata_scene).expect("admitted world metadata changed")
    }
    pub fn room(&self)->Room<'_> {self.room_for(self.selected).expect("No admitted scene selected")}
}
