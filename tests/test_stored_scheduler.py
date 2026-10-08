"""Resident traversal API tests plus retained legacy residency policy tests.

The guest no longer schedules room reads during spatial traversal. Compile the
actual resident methods with small admitted-scene metadata and no CD/decoder
service; accidental transport dependencies fail this harness. Guest startup
now admits exclusive scene gates only; joint packing has separate host tests. The standalone
residency module's own lease/priority tests remain useful and run separately.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def method(source, name):
    """Extract a Rust method body without depending on descriptive comments."""
    import re
    match = re.search(r'^    (?:pub )?fn ' + re.escape(name) + r'\(', source, re.MULTILINE)
    if match is None:
        raise AssertionError(f'Missing production resident API: {name}')
    start = match.start()
    opening = source.index('{', match.end())
    depth = 1
    end = opening + 1
    while depth:
        if source[end] == '{':
            depth += 1
        elif source[end] == '}':
            depth -= 1
        end += 1
    return source[start:end]


class StoredSchedulerTests(unittest.TestCase):
    def run_rust(self, code, coverage_dependencies=False, env=None):
        import os
        env = {**os.environ, **(env or {})}
        (ROOT / '.hkpsx').mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='resident-cache-test-', dir=ROOT / '.hkpsx') as temp:
            path = Path(temp)
            (path / 'probe.rs').write_text(code)
            extra=['-L',f'dependency={path}']
            if coverage_dependencies:
                for name,source in [('hk_format',ROOT/'shared/hk-format/src/lib.rs'),('room_decode',ROOT/'game/src/room_decode.rs')]:
                    library=path/f'lib{name}.rlib'
                    built=subprocess.run(['rustc','--edition=2021','--crate-type','rlib','--crate-name',name,
                                          str(source),'-o',str(library),*extra],capture_output=True,text=True,env=env)
                    self.assertEqual(built.returncode,0,built.stdout+built.stderr)
                    extra+=['--extern',f'{name}={library}']
            compiled = subprocess.run(
                ['rustc', '--edition=2021', '--test', str(path / 'probe.rs'), '-o', str(path / 'probe'),*extra],
                capture_output=True, text=True, env=env)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            tested = subprocess.run([str(path / 'probe'), '--test-threads=1'], capture_output=True, text=True)
            self.assertEqual(tested.returncode, 0, tested.stdout + tested.stderr)
            return tested.stdout

    def test_retained_residency_lease_and_priority_policies(self):
        code = '#![allow(dead_code)]\n#[path="' + str(ROOT / 'game/src/room_residency.rs') + '"] mod room_residency;\n'
        output = self.run_rust(code)
        self.assertIn('cancelled_read_cannot_release_completed_decode_or_gpu_pin', output)
        self.assertIn('decode_admission_only_waits_for_an_urgent_unread_demand_with_live_transport', output)

    def test_production_resident_traversal_never_demands_transport_or_decode(self):
        source = (ROOT / 'game/src/disc.rs').read_text()
        methods = '\n'.join(method(source, name) for name in (
            'try_select', 'request', 'pump', 'prefetch_regions', 'prefetch',
            'protect_upload', 'pending', 'is_ready', 'atlases_ready'))
        code = r'''
#![allow(dead_code)]
const EMPTY:usize=usize::MAX;
const SCENE_GATE_LOAD:bool=EXCLUSIVE;
const SCENE_COUNT:usize=2;
const SCENE_ATLAS_RANGES:&[(usize,usize)]=&[(0,20),(20,23)];
const REGION_SCENE_LOCAL:&[(usize,usize)]=&[(0,0),(0,1),(0,2),(1,0),(1,1),(1,2)];
struct SceneDesc{raw_len:usize}
const SCENE_MANIFEST:&[SceneDesc]=&[SceneDesc{raw_len:4096},SceneDesc{raw_len:2048}];
fn scene_index(region:usize)->usize{REGION_SCENE_LOCAL[region].0}
#[derive(Clone,Copy,Debug,PartialEq)]enum LoadError{RoomMismatch}
static mut HK_REGION_ACTIVATIONS:u32=0;
static mut HK_ROOM_CACHE_HITS:u32=0;
static mut HK_ROOM_BYTES:u32=0;
static mut STATUS:(u32,u32)=(0,0);
fn mark(state:u32,error:u32){unsafe{STATUS=(state,error);}}
const ATLASES:&[u8]=&[0;23];
struct CoverageDesc{raw_len:usize}
const COVERAGE_MANIFEST:&[CoverageDesc]=&[CoverageDesc{raw_len:64},CoverageDesc{raw_len:64}];
struct WorldMetaDesc{raw_len:usize}
const WORLD_META_MANIFEST:&[WorldMetaDesc]=&[WorldMetaDesc{raw_len:160},WorldMetaDesc{raw_len:160}];
static mut HK_COVERAGE_SCENE:u32=0;
static mut HK_COVERAGE_BYTES:u32=0;
// The room prefetch `pump` plans: it may only record a wanted read for the
// drive; starting or stopping transport is never its business here.
const SECTOR_BYTES:usize=2048;
const ARENA_TOTAL:usize=16*SECTOR_BYTES;
struct Buffer{words:[u32;ARENA_TOTAL/4]}
static mut BUFFERS:Buffer=Buffer{words:[0;ARENA_TOTAL/4]};
#[derive(Clone,Copy,PartialEq)]enum Fetch{Idle,Reading,Ready}
struct Prefetch{hint:usize,scene:usize,total:usize,sectors:usize,state:Fetch,code_only:bool,want:Option<(usize,usize,usize,u32,*mut u32,usize,bool)>}
static mut PREFETCH:Prefetch=Prefetch{hint:EMPTY,scene:EMPTY,total:0,sectors:0,state:Fetch::Idle,code_only:false,want:None};
fn prefetch_state()->&'static mut Prefetch{unsafe{&mut *(&raw mut PREFETCH)}}
mod cd_stream{pub fn cancel(){panic!("resident traversal touched transport");}}
// Rooms without streamed code: planning a code read is a no-op here.
mod modules{pub fn plan_code(_:usize){} pub fn code_chunk(_:usize)->Option<usize>{None} pub fn chunk_len(_:usize)->usize{0} pub fn gave_up(_:usize)->bool{false}}
const CODE_FIRST:bool=false;
struct Cache{loaded_scenes:usize,loaded_atlases:usize,selected:usize,resident_scene:usize,atlas_scene:usize,coverage_scene:usize,coverage_len:usize,metadata_scene:usize,metadata_len:usize,effect_scene:usize,ready:bool}
impl Cache{
 fn admitted(count:usize)->Self{
  unsafe{HK_REGION_ACTIVATIONS=0;HK_ROOM_CACHE_HITS=0;HK_ROOM_BYTES=0;STATUS=(0,0);}
 Self{loaded_scenes:count,loaded_atlases:23,selected:EMPTY,resident_scene:0,atlas_scene:0,coverage_scene:0,coverage_len:64,metadata_scene:0,metadata_len:160,effect_scene:0,ready:true}
 }
 fn metadata_admitted(&self)->bool{self.metadata_scene!=EMPTY}
 fn group_sectors(&self,_scene:usize)->usize{4}
 // The four-sector stub group stages whole at the tail.
 fn staged_prefix(&self,scene:usize)->usize{self.group_sectors(scene)}
 fn group_lba(&self,scene:usize)->u32{100+scene as u32}
 fn code_skip(&self,_scene:usize)->usize{0}
METHODS
}
#[test]fn exclusive_views_require_matching_complete_scene_atlas_and_proof_owners(){
 if !SCENE_GATE_LOAD{return;}
 let mut c=Cache::admitted(0);
 for owner in [0,1,0,1]{
  c.loaded_scenes=0;c.resident_scene=EMPTY;c.selected=EMPTY;c.atlas_scene=owner;c.metadata_scene=owner;c.metadata_len=160;c.effect_scene=owner;
  let (start,end)=SCENE_ATLAS_RANGES[owner];
  for completed in start..=end{
   c.loaded_atlases=completed;
   assert_eq!(c.atlases_ready(),completed==end);
   for region in 0..6{assert!(!c.is_ready(region));assert_eq!(c.try_select(region),Ok(false));}
  }
  c.loaded_scenes=1;c.resident_scene=owner;
  for proof in [EMPTY,1-owner] {
   c.coverage_scene=proof;c.coverage_len=64;
   for region in 0..6{
    assert!(!c.is_ready(region));assert_eq!(c.request(region),Ok(false));
    assert_eq!(c.try_select(region),Ok(false));assert!(!c.protect_upload(Some(region)));
   }
   assert_eq!(c.selected,EMPTY);
  }
  c.coverage_scene=owner;c.coverage_len=64;
  c.atlas_scene=1-owner;c.loaded_atlases=SCENE_ATLAS_RANGES[1-owner].1;
  assert!(c.atlases_ready());
  for region in 0..6{assert!(!c.is_ready(region),"complete atlas belongs to another scene");}
  c.atlas_scene=owner;c.loaded_atlases=end;
  for length in [0,63,65] {
   c.coverage_len=length;
   for region in 0..6{assert!(!c.is_ready(region));assert_eq!(c.try_select(region),Ok(false));}
  }
  c.coverage_len=64;
  for region in 0..6{
   let expected=scene_index(region)==owner;
   assert_eq!(c.is_ready(region),expected);assert_eq!(c.request(region),Ok(expected));
   assert_eq!(c.try_select(region),Ok(expected));assert_eq!(c.protect_upload(Some(region)),expected);
  }
  let selected=c.selected;
  for _ in 0..100{c.prefetch_regions(&[0,5,usize::MAX]);c.pump().unwrap();}
  assert_eq!(c.resident_scene,owner);assert_eq!(c.selected,selected);assert_eq!(c.pending(),None);
  // A hinted next scene only becomes a wanted read in the free arena tail.
  prefetch_state().hint=1-owner;c.pump().unwrap();
  let want=prefetch_state().want.take().expect("hinted scene planned");
  assert_eq!((want.0,want.1,want.2,want.3),(1-owner,4,4,101-owner as u32));
  assert_eq!(c.resident_scene,owner);assert_eq!(c.selected,selected);assert_eq!(c.pending(),None);
  prefetch_state().hint=owner;c.pump().unwrap();assert!(prefetch_state().want.is_none(),"resident scene is never prefetched");
  prefetch_state().hint=EMPTY;
  for invalid in [6,usize::MAX]{
   assert_eq!(c.request(invalid),Err(LoadError::RoomMismatch));assert!(!c.is_ready(invalid));
  }
 }
}
'''.replace('METHODS', methods)
        for exclusive in ('true',):
            with self.subTest(exclusive=exclusive):self.run_rust(code.replace('EXCLUSIVE',exclusive))

    def test_production_scene_admission_matches_parsed_counts_and_complete_region_mapping(self):
        source=(ROOT/'game/src/disc.rs').read_text()
        code=r"""
#![allow(dead_code)]
const EMPTY:usize=usize::MAX;
const SCENE_GATE_LOAD:bool=EXCLUSIVE;
const SCENE_COUNT:usize=2;
const REGION_SCENE_LOCAL:&[(usize,usize)]=&[(0,0),(0,1),(1,0)];
const SCENE_ATLAS_RANGES:&[(usize,usize)]=&[(0,2),(2,4)];
#[derive(Copy,Clone)]struct SceneDesc{scene_id:usize,ram_offset:usize,arena_capacity:usize,stored_len:usize,stored_fnv:u32,raw_len:usize,raw_fnv:u32}
const SCENE_MANIFEST:&[SceneDesc]=&[
 SceneDesc{scene_id:0,ram_offset:0,arena_capacity:48,stored_len:24,stored_fnv:0,raw_len:24,raw_fnv:0},
 SceneDesc{scene_id:1,ram_offset:if SCENE_GATE_LOAD{0}else{24},arena_capacity:if SCENE_GATE_LOAD{48}else{24},stored_len:24,stored_fnv:0,raw_len:24,raw_fnv:0}];
struct AtlasDesc{kind:u8,count:usize}
const ATLASES:&[AtlasDesc]=&[AtlasDesc{kind:0,count:2},AtlasDesc{kind:1,count:3},AtlasDesc{kind:0,count:1},AtlasDesc{kind:1,count:2}];
struct Buffer{words:[u32;12]}
static mut BUFFERS:Buffer=Buffer{words:[0;12]};
static mut HK_SCENE_READ_ID:u32=0;
static mut HK_SCENE_DECODE_ID:u32=0;
static mut HK_ROOM_DECODE_PHASE:u32=0;
static mut HK_SCENE_LOADS:u32=0;
static mut COVERAGE_ATTEMPTS:usize=0;
static mut COVERAGE_FAIL:usize=EMPTY;
static mut ATLAS_CALLS:usize=0;
static mut GEOMETRY_READS:usize=0;
static mut DRAW_COUNTS:[usize;2]=[3,2];
struct CoverageDesc{draw_pool_count:u32}
const COVERAGE_MANIFEST:&[CoverageDesc]=&[CoverageDesc{draw_pool_count:3},CoverageDesc{draw_pool_count:2}];
struct WorldMetaDesc{raw_len:usize}
const WORLD_META_MANIFEST:&[WorldMetaDesc]=&[WorldMetaDesc{raw_len:160},WorldMetaDesc{raw_len:160}];
// The real decoder/parser have their own native format tests. This boundary
// double supplies validated metadata to the *production* admission method so
// malformed manifest-to-scene bindings cannot be accidentally accepted.
static mut PAYLOADS:[[u32;6];2]=[[0,2,3,2,1,2],[1,1,2,1,3,0]];
mod input{pub fn checkpoint(){}}
mod ambience{pub fn missing(_:usize)->u8{0}}
mod gate_probe{pub const SCENE_CHECK:u8=0;pub fn set(_:u8){}pub fn decoder(_:u32)->u8{0}}
// Rooms without streamed code or art: the module calls are no-ops, and the
// admission never touches the drive for art.
mod modules{pub fn gate_begin(_:usize){} pub fn art_missing(_:usize)->bool{false} pub fn art_begin(_:usize)->bool{false} pub fn admit_art(_:usize)->bool{true}}
mod cd_stream{pub enum Status{Busy,Done} pub fn status()->Status{Status::Done} pub fn received()->usize{0}}
static mut GATE_VERIFIED:u64=0;
static mut GATE_SCENE:usize=EMPTY;
static mut GROUP_LANDED:usize=EMPTY;
static mut HK_MODULE_GATE_ART_MISSED:u32=0;
fn preverified(_:usize)->bool{false}
const SCENE_ARENA_BYTES:usize=48;
const AMBIENCE_STAGE_BYTES:usize=0;
fn prefetch_guard(_:usize){}
// The harness admits through the per-chunk path; the staged-tail path is
// covered by the tour and the routes.
#[derive(Clone,Copy)]enum Stage{Read,Tail{base:usize,pre:usize}}
#[derive(Debug)]enum DecodeError{Checksum,Decompress,RoomFormat}
struct Decoder{length:usize}
impl Decoder{
 fn new_scene(_:usize,_:u32,length:usize,_:u32)->Self{Self{length}}
 fn at(self,_:usize)->Self{self}
 fn hashed(self,_:bool)->Self{self}
 fn phase_id(&self)->u32{0}
 fn step(&mut self,_:&mut[u8],_:usize)->Result<Option<usize>,DecodeError>{Ok(Some(self.length))}
}
struct Scene<'a>{bytes:&'a[u8]}
impl<'a> Scene<'a>{
 unsafe fn validated_view(bytes:&'a[u8])->Self{Self{bytes}}
 fn word(&self,index:usize)->usize{u32::from_le_bytes(self.bytes[index*4..index*4+4].try_into().unwrap())as usize}
 fn id(&self)->usize{self.word(0)}
 fn page_count(&self)->usize{self.word(1)}
 fn palette_count(&self)->usize{self.word(2)}
 fn room_count(&self)->usize{self.word(3)}
 fn draw_pool_count(&self)->usize{unsafe{DRAW_COUNTS[self.id()]}}
 fn chunk_id(&self,index:usize)->Option<usize>{(index<self.room_count()&&index<2).then(||self.word(index+4))}
}
#[derive(Debug,PartialEq)]enum LoadError{RoomMismatch,Checksum,Decompress,RoomFormat,Coverage}
struct Cache{loaded_scenes:usize,resident_scene:usize,selected:usize,entries:[usize;2],coverage_scene:usize,coverage_len:usize,metadata_scene:usize,metadata_len:usize,effect_scene:usize}
impl Cache{
 fn new()->Self{Self{loaded_scenes:0,resident_scene:EMPTY,selected:EMPTY,entries:[0,1],coverage_scene:EMPTY,coverage_len:0,metadata_scene:EMPTY,metadata_len:0,effect_scene:EMPTY}}
 // Production rebuilds each directory entry from the manifests instead of
 // keeping the table; the harness only needs a chunk index to stand in for one.
 fn entry(&self,index:usize)->Result<usize,LoadError>{Ok(self.entries[index])}
 fn prepare_metadata(&mut self,_wanted:usize,_:Stage)->Result<(),LoadError>{Ok(())}
 fn stage_group(&mut self,_wanted:usize)->Result<Stage,LoadError>{Ok(Stage::Read)}
 fn stage(&mut self,index:usize,st:Stage,lo:usize)->Result<(usize,usize),LoadError>{
  assert!(matches!(st,Stage::Read));let e=self.entry(index)?;self.read(e,lo)?;Ok((SCENE_ARENA_BYTES,0))
 }
 // Ambience clips stage through the arena, so only while no scene owns it.
 fn prepare_scene_ambience(&mut self,_wanted:usize)->Result<(),LoadError>{
  assert_eq!(self.loaded_scenes,0,"ambience stages before any scene owns the arena");Ok(())
 }
 // The quick map's art is read back through the arena front, like a clip.
 fn prepare_map(&mut self,_wanted:usize)->Result<(),LoadError>{
  assert_eq!(self.loaded_scenes,0,"the map stages before any scene owns the arena");Ok(())
 }
 // The room's code chunk heads its group; this harness's rooms carry none.
 fn prepare_code(&mut self,_wanted:usize,_:Stage)->Result<(),LoadError>{Ok(())}
 // The scene's one-shot bank heads its group and is uploaded before any decode.
 fn prepare_scene_sfx(&mut self,_wanted:usize,_:Stage)->Result<(),LoadError>{
  assert_eq!(self.loaded_scenes,0,"the scene sound bank stages before any scene owns the arena");Ok(())
 }
 // Production also drops the cached bank view, which the harness does not model.
 fn revoke_metadata(&mut self){self.metadata_scene=EMPTY;self.metadata_len=0;}
 fn prepare_effect_art(&mut self,wanted:usize,_:Stage)->Result<(),LoadError>{
  if SCENE_GATE_LOAD{assert_eq!(self.coverage_scene,wanted,"effect art follows proof admission");}
  self.effect_scene=wanted;Ok(())
 }
 fn prepare_coverage(&mut self,wanted:usize,_:Stage)->Result<(),LoadError>{
  if self.coverage_scene==wanted{return Ok(());}
  self.coverage_scene=EMPTY;self.coverage_len=0;
  unsafe{COVERAGE_ATTEMPTS+=1;if COVERAGE_FAIL==wanted{return Err(LoadError::Checksum);}}
  self.coverage_scene=wanted;self.coverage_len=64;Ok(())
 }
 fn prepare_atlases(&mut self,wanted:usize,_:Stage)->Result<(),LoadError>{
  if SCENE_GATE_LOAD{assert_eq!(self.coverage_scene,wanted,"atlas precedes proof admission");assert_eq!(self.coverage_len,64);}
  unsafe{ATLAS_CALLS+=1;}Ok(())
 }
 fn read(&mut self,entry:usize,offset:usize)->Result<(),LoadError>{
  if SCENE_GATE_LOAD{assert_eq!(self.coverage_scene,entry,"geometry precedes proof admission");}
  unsafe{GEOMETRY_READS+=1;}
  unsafe{let target=(&raw mut BUFFERS.words).cast::<u8>().add(offset).cast::<u32>();
   for i in 0..6{target.add(i).write(PAYLOADS[entry][i]);}}
  Ok(())
 }
METHOD
}
fn reset(){unsafe{PAYLOADS=[[0,2,3,2,1,2],[1,1,2,1,3,0]];HK_SCENE_LOADS=0;
 COVERAGE_ATTEMPTS=0;COVERAGE_FAIL=EMPTY;ATLAS_CALLS=0;GEOMETRY_READS=0;DRAW_COUNTS=[3,2];}}
#[test]fn exact_scene_and_atlas_bindings_admit_and_allow_reentry(){
 reset();let mut cache=Cache::new();
 for wanted in [0,1,0,1]{
  assert_eq!(cache.admit_scenes(wanted),Ok(()));
  assert_eq!(cache.loaded_scenes,if SCENE_GATE_LOAD{1}else{2});
  if SCENE_GATE_LOAD{assert_eq!(cache.resident_scene,wanted);}
 }
 assert_eq!(unsafe{HK_SCENE_LOADS},if SCENE_GATE_LOAD{4}else{2});
}
#[test]fn failed_proof_replacement_revokes_old_scene_and_retry_rereads_proof_first(){
 if !SCENE_GATE_LOAD{return;}
 reset();let mut cache=Cache::new();cache.admit_scenes(0).unwrap();cache.selected=0;
 let prior=unsafe{(ATLAS_CALLS,GEOMETRY_READS)};
 unsafe{COVERAGE_FAIL=1;}
 assert_eq!(cache.admit_scenes(1),Err(LoadError::Checksum));
 assert_eq!((cache.loaded_scenes,cache.resident_scene,cache.selected),(0,EMPTY,EMPTY));
 assert_eq!((cache.coverage_scene,cache.coverage_len),(EMPTY,0));
 assert_eq!(unsafe{(ATLAS_CALLS,GEOMETRY_READS)},prior,"failed proof must prevent atlas and geometry replacement");
 unsafe{COVERAGE_FAIL=EMPTY;}
 cache.admit_scenes(1).unwrap();
 assert_eq!((cache.loaded_scenes,cache.resident_scene,cache.coverage_scene),(1,1,1));
 assert_eq!(unsafe{COVERAGE_ATTEMPTS},3);
 let reads=unsafe{(COVERAGE_ATTEMPTS,GEOMETRY_READS)};
 for _ in 0..20{cache.admit_scenes(1).unwrap();}
 assert_eq!(unsafe{(COVERAGE_ATTEMPTS,GEOMETRY_READS)},reads,"same-scene admission must preserve resident proof and geometry");
}
#[test]fn geometry_draw_pool_must_match_validated_proof_pool(){
 reset();let mut cache=Cache::new();unsafe{DRAW_COUNTS[0]=4;}
 assert_eq!(cache.admit_scenes(0),Err(LoadError::Coverage));
 assert_eq!((cache.loaded_scenes,cache.resident_scene,cache.selected),(0,EMPTY,EMPTY));
 unsafe{DRAW_COUNTS[0]=3;}
 assert_eq!(cache.admit_scenes(0),Ok(()));
}
#[test]fn count_id_and_mapping_mismatches_never_publish_incoming_scene(){
 for (field,bad)in[(0,9),(1,1),(1,3),(2,2),(2,4),(3,1),(3,3),(4,2),(5,1)]{
  reset();unsafe{PAYLOADS[0][field]=bad;}
  let mut cache=Cache::new();
  assert_eq!(cache.admit_scenes(0),Err(LoadError::RoomMismatch),"field {field} value {bad}");
  assert_eq!(cache.loaded_scenes,0);assert_eq!(cache.resident_scene,EMPTY);assert_eq!(cache.selected,EMPTY);
  // Repaired input must be reread and admitted, not masked by stale readiness.
  reset();assert_eq!(cache.admit_scenes(0),Ok(()));
 }
}
#[test]fn failed_geometry_replacement_revokes_outgoing_and_retry_readmits(){
 reset();let mut cache=Cache::new();
 if SCENE_GATE_LOAD{cache.admit_scenes(0).unwrap();cache.selected=0;}
 unsafe{PAYLOADS[1][2]=3;}
 assert_eq!(cache.admit_scenes(1),Err(LoadError::RoomMismatch));
 assert_eq!(cache.loaded_scenes,if SCENE_GATE_LOAD{0}else{1});
 assert_eq!(cache.resident_scene,if SCENE_GATE_LOAD{EMPTY}else{0});
 assert_eq!(cache.selected,EMPTY);
 unsafe{PAYLOADS[1][2]=2;}
 assert_eq!(cache.admit_scenes(1),Ok(()));
 assert_eq!(cache.loaded_scenes,if SCENE_GATE_LOAD{1}else{2});
 assert_eq!(cache.resident_scene,1);
}
""".replace('METHOD',method(source,'admit_scenes'))
        for exclusive in ('true',):
            with self.subTest(exclusive=exclusive):self.run_rust(code.replace('EXCLUSIVE',exclusive))

    def test_production_coverage_load_validates_real_bytes_before_owner_publication(self):
        import struct
        def bundle(owner=0):
            raw=bytearray(b'HKOCSC01'+struct.pack('<6I',owner,0x1122+owner,0x3344+owner,600,2,0))
            raw+=b''.join(struct.pack('<2I',*row)for row in [(80,1),(92,1),(96,600),(1296,0),(1296,0),(1296,0)])
            raw+=struct.pack('<hhHHI',-2,1,1,1,0)+struct.pack('<I',1)+struct.pack('<600H',0,*([65535]*599))
            return bytes(raw)
        def stored(raw):
            # Literal-only LZ4 deliberately spans multiple decoder/copy budgets.
            remaining=len(raw)-15;length=[]
            while remaining>=255:length.append(255);remaining-=255
            return b'HLZC'+struct.pack('<I',len(raw))+bytes([0xf0,*length,remaining])+raw
        def fnv(raw):
            h=0x811c9dc5
            for byte in raw:h=((h^byte)*0x1000193)&0xffffffff
            return h
        cases=[]
        for index in range(12):
            owner=1 if index==1 else 0;raw=bytearray(bundle(owner))
            for case,offset,value in [(4,8,99),(5,12,99),(6,16,99),(7,20,599),(10,84,0),(11,92,3)]:
                if index==case:struct.pack_into('<I',raw,offset,value)
            if index==8:struct.pack_into('<H',raw,1294,1) # last pool reference fails after many validation steps
            payload=bytearray(stored(raw));stored_hash=fnv(payload);raw_hash=fnv(raw)
            if index==2:payload[-1]^=1
            if index==3:raw_hash^=1
            if index==9:struct.pack_into('<I',payload,4,len(raw)+4);stored_hash=fnv(payload)
            cases.append((owner,bytes(raw),bytes(payload),stored_hash,raw_hash))
        descriptors=[];payloads=[];raws=[]
        for i,(owner,raw,payload,sh,rh) in enumerate(cases):
            descriptors.append('CoverageDesc{'+f'scene_id:{owner},scene_raw_fnv:{0x1122+owner},atlas_fnv:{0x3344+owner},draw_pool_count:600,chunk_id:{i+1},raw_len:{len(raw)},stored_len:{len(payload)},raw_fnv:{rh},stored_fnv:{sh}'+'}')
            payloads.append('&['+','.join(map(str,payload))+']')
            raws.append('&['+','.join(map(str,raw))+']')
        source=(ROOT/'game/src/disc.rs').read_text()
        code=r'''
#![allow(dead_code)]
use room_decode::{Decoder,Error as DecodeError};
use hk_format::coverage::{CoverageValidation,CoverageView,Expected as CoverageExpected};
const EMPTY:usize=usize::MAX;
const SCENE_ARENA_BYTES:usize=4096;
const COVERAGE_ARENA_BYTES:usize=2048;
#[derive(Clone,Copy)]struct CoverageDesc{
 scene_id:u32,scene_raw_fnv:u32,atlas_fnv:u32,draw_pool_count:u32,chunk_id:usize,
 raw_len:usize,raw_fnv:u32,stored_len:usize,stored_fnv:u32,
}
const COVERAGE_MANIFEST:&[CoverageDesc]=&[DESCRIPTORS];
const PAYLOADS:&[&[u8]]=&[PAYLOADS_DATA];
const RAW:&[&[u8]]=&[RAW_DATA];
#[repr(C,align(4))]struct Buffer{words:[u32;SCENE_ARENA_BYTES/4]}
#[repr(C,align(4))]struct CoverageBuffer{words:[u32;COVERAGE_ARENA_BYTES/4]}
static mut BUFFERS:Buffer=Buffer{words:[0;SCENE_ARENA_BYTES/4]};
static mut COVERAGE_BUFFER:CoverageBuffer=CoverageBuffer{words:[0xa5a5a5a5;COVERAGE_ARENA_BYTES/4]};
static mut HK_ROOM_DECODE_PHASE:u32=0;
static mut HK_COVERAGE_LOADS:u32=0;
static mut HK_COVERAGE_BYTES:u32=0;
static mut HK_COVERAGE_SCENE:u32=0;
static mut READS:[usize;12]=[0;12];
static mut CHECKPOINTS:usize=0;
static mut FAIL_READ:bool=false;
mod input{pub fn checkpoint(){unsafe{
 super::CHECKPOINTS+=1;
 assert_eq!(super::HK_COVERAGE_SCENE,0,"proof owner published before all validation/copy checkpoints");
}}}
#[derive(Debug,PartialEq)]enum LoadError{Coverage,Checksum,Decompress,PayloadRead}
mod gate_probe{pub const COVERAGE_CHECK:u8=0;pub fn set(_:u8){}pub fn decoder(_:u32)->u8{0}}
// No chunk was hashed ahead of this admission: the decoder checks it all.
fn preverified(_:usize)->bool{false}
#[derive(Clone,Copy)]enum Stage{Read,Tail{base:usize,pre:usize}}
const R:Stage=Stage::Read;
struct Cache{entries:[usize;12],loaded_scenes:usize,coverage_scene:usize,coverage_len:usize}
impl Cache{
 fn new()->Self{Self{entries:core::array::from_fn(|i|i),loaded_scenes:0,coverage_scene:EMPTY,coverage_len:0}}
 fn init(&mut self)->Result<(),LoadError>{Ok(())}
 fn entry(&self,index:usize)->Result<usize,LoadError>{Ok(self.entries[index])}
 fn read(&mut self,entry:usize,offset:usize)->Result<(),LoadError>{
  assert_eq!(offset,0);assert_eq!(self.loaded_scenes,0);
  assert_eq!((self.coverage_scene,self.coverage_len),(EMPTY,0));
  unsafe{
   READS[entry]+=1;
   let target=core::slice::from_raw_parts_mut((&raw mut BUFFERS.words).cast::<u8>(),SCENE_ARENA_BYTES);
   target.fill(0x3c);target[..PAYLOADS[entry].len()].copy_from_slice(PAYLOADS[entry]);
   if FAIL_READ{return Err(LoadError::PayloadRead);}
  }
  Ok(())
 }
 fn stage(&mut self,index:usize,st:Stage,lo:usize)->Result<(usize,usize),LoadError>{
  assert!(matches!(st,Stage::Read));let e=self.entry(index)?;self.read(e,lo)?;Ok((SCENE_ARENA_BYTES,0))
 }
METHOD
}
fn copy()->Vec<u8>{unsafe{core::slice::from_raw_parts((&raw const COVERAGE_BUFFER.words).cast::<u8>(),COVERAGE_ARENA_BYTES).to_vec()}}
fn reset(){unsafe{
 HK_COVERAGE_LOADS=0;HK_COVERAGE_BYTES=0;HK_COVERAGE_SCENE=0;READS=[0;12];CHECKPOINTS=0;FAIL_READ=false;
 core::ptr::write(&raw mut COVERAGE_BUFFER,CoverageBuffer{words:[0xa5a5a5a5;COVERAGE_ARENA_BYTES/4]});
}}
#[test]fn real_decoder_parser_and_chunked_copy_publish_exact_owner_then_cache_it(){
 reset();let mut c=Cache::new();
 for scene in [0,1,0,1]{
  assert_eq!(c.prepare_coverage(scene,R),Ok(()));
  assert_eq!((c.coverage_scene,c.coverage_len),(scene,1296));
  assert_eq!(&copy()[..1296],RAW[scene]);assert!(copy()[1296..].iter().all(|&b|b==0xa5));
  let bytes=copy();let mut aligned=vec![0u32;1296/4];unsafe{core::ptr::copy_nonoverlapping(bytes.as_ptr(),aligned.as_mut_ptr().cast::<u8>(),1296);}
  let view=CoverageView::parse(unsafe{core::slice::from_raw_parts(aligned.as_ptr().cast::<u8>(),1296)},CoverageExpected{
   scene_id:scene as u32,scene_raw_fnv:0x1122+scene as u32,atlas_fnv:0x3344+scene as u32,draw_pool_count:600}).unwrap();
  assert_eq!(view.draw_certificate(0),Some(0));assert_eq!(view.draw_certificate(599),None);
  assert_eq!(unsafe{HK_COVERAGE_SCENE},scene as u32+1);assert_eq!(unsafe{HK_COVERAGE_BYTES},1296);
  let state=unsafe{(READS,CHECKPOINTS,HK_COVERAGE_LOADS)};
  c.loaded_scenes=1;
  for _ in 0..100{assert_eq!(c.prepare_coverage(scene,R),Ok(()));}
  assert_eq!(unsafe{(READS,CHECKPOINTS,HK_COVERAGE_LOADS)},state,"resident spatial views must not reload proof");
  assert_eq!(c.prepare_coverage(1-scene,R),Err(LoadError::Coverage));
  assert_eq!((c.coverage_scene,c.coverage_len),(scene,1296));
  assert_eq!(unsafe{(READS,CHECKPOINTS,HK_COVERAGE_LOADS)},state,"cannot reclaim arena under admitted geometry");
  c.loaded_scenes=0;
 }
 assert_eq!(unsafe{HK_COVERAGE_LOADS},4);assert!(unsafe{CHECKPOINTS}>80);
}
// The guest trusts the cook unless built with HK_VERIFY_COOK: a payload whose
// stored hash matches is not raw-hashed again, so a wrong raw hash (case 3)
// is only caught in the verifying build.
const VERIFY:bool=option_env!("HK_VERIFY_COOK").is_some();
#[test]fn bad_hash_identity_late_reference_and_decompression_never_publish_partial_proof(){
 for scene in 2..12{
  if scene==3&&!VERIFY{
   reset();let mut c=Cache::new();assert_eq!(c.prepare_coverage(3,R),Ok(()),"trusted cook skips the raw hash");continue;
  }
  reset();let mut c=Cache::new();c.prepare_coverage(0,R).unwrap();let before=copy();
  let expected=if scene==2||scene==3{LoadError::Checksum}else if scene==9{LoadError::Decompress}else{LoadError::Coverage};
  assert_eq!(c.prepare_coverage(scene,R),Err(expected),"case {scene}");
  assert_eq!((c.coverage_scene,c.coverage_len),(EMPTY,0));assert_eq!(copy(),before,"failed proof changed admitted-buffer bytes");
  assert_eq!(unsafe{(HK_COVERAGE_LOADS,HK_COVERAGE_BYTES,HK_COVERAGE_SCENE)},(1,0,0));
  c.prepare_coverage(0,R).unwrap();assert_eq!(unsafe{READS[0]},2);assert_eq!(unsafe{HK_COVERAGE_LOADS},2);
 }
}
#[test]fn partial_transport_failure_and_stale_length_require_a_fresh_read(){
 reset();let mut c=Cache::new();c.prepare_coverage(0,R).unwrap();let before=copy();
 unsafe{FAIL_READ=true;}
 assert_eq!(c.prepare_coverage(1,R),Err(LoadError::PayloadRead));assert_eq!(copy(),before);
 assert_eq!((c.coverage_scene,c.coverage_len),(EMPTY,0));
 unsafe{FAIL_READ=false;}c.prepare_coverage(1,R).unwrap();assert_eq!(unsafe{READS[1]},2);
 c.coverage_len-=4;c.prepare_coverage(1,R).unwrap();assert_eq!(unsafe{READS[1]},3);
 assert_eq!((c.coverage_scene,c.coverage_len),(1,1296));
}
'''.replace('DESCRIPTORS',','.join(descriptors)).replace('PAYLOADS_DATA',','.join(payloads)).replace('RAW_DATA',','.join(raws)).replace('METHOD',method(source,'prepare_coverage'))
        for env in ({}, {'HK_VERIFY_COOK': '1'}):
            with self.subTest(**env):
                output=self.run_rust(code,coverage_dependencies=True,env=env)
                self.assertIn('bad_hash_identity_late_reference_and_decompression_never_publish_partial_proof',output)

    def test_production_bootstrap_retry_discards_admission_before_menu_reuse(self):
        source = (ROOT / 'game/src/disc.rs').read_text()
        methods = '\n'.join(method(source, name) for name in (
            'reset_bootstrap', 'is_ready', 'atlases_ready'))
        code = r'''
#![allow(dead_code)]
const EMPTY:usize=usize::MAX;
const SCENE_GATE_LOAD:bool=EXCLUSIVE;
const SCENE_COUNT:usize=2;
const SCENE_ATLAS_RANGES:&[(usize,usize)]=&[(0,20),(20,23)];
const REGION_SCENE_LOCAL:&[(usize,usize)]=&[(0,0),(0,1),(1,0)];
const ATLASES:&[u8]=&[0;23];
struct CoverageDesc{raw_len:usize}
const COVERAGE_MANIFEST:&[CoverageDesc]=&[CoverageDesc{raw_len:64},CoverageDesc{raw_len:64}];
struct WorldMetaDesc{raw_len:usize}
const WORLD_META_MANIFEST:&[WorldMetaDesc]=&[WorldMetaDesc{raw_len:160},WorldMetaDesc{raw_len:160}];
static mut HK_COVERAGE_SCENE:u32=0;
static mut HK_COVERAGE_BYTES:u32=0;
struct Drive{music:bool,clip:bool,pool:bool}
static mut DRIVE:Drive=Drive{music:false,clip:false,pool:false};
// A pool read in flight is dropped with the transport, like the clip.
mod modules{pub fn pool_landed(ok:bool){assert!(!ok);}}
// A background ambience clip read in flight is dropped with the transport.
static mut CLIP_DROPPED:bool=false;
mod ambience{pub fn abort_prefetch(){}pub fn prefetch_done(ok:bool){assert!(!ok);unsafe{super::CLIP_DROPPED=true;}}}
// No room prefetch is in flight in these cases; its own cancel is the drive's.
fn prefetch_discard(){}
mod music{pub fn read_done(_:bool,_:bool){}}
mod cd_stream {
    #[derive(Clone,Copy,Debug,PartialEq)]pub enum Status{Idle,Busy,Done,Failed(u32)}
    static mut SEQUENCE:[Status;4]=[Status::Idle;4];
    static mut POSITION:usize=0;
    static mut CANCELLED:bool=false;
    static mut WRITER_ACTIVE:bool=false;
    pub fn setup(first:Status,terminal:Status) {
        unsafe {
            SEQUENCE=[first,first,terminal,terminal];POSITION=0;
            CANCELLED=false;WRITER_ACTIVE=first==Status::Busy;
        }
    }
    pub fn cancel(){unsafe{assert!(!CANCELLED);CANCELLED=true;}}
    pub fn status()->Status {
        unsafe {
            assert!(CANCELLED,"transport must be cancelled before polling");
            let result=SEQUENCE[POSITION];POSITION+=1;
            if result!=Status::Busy{WRITER_ACTIVE=false;}result
        }
    }
    pub fn assert_quiescent(expected_polls:usize) {
        unsafe {
            assert!(CANCELLED);assert!(!WRITER_ACTIVE);
            assert_eq!(POSITION,expected_polls);
        }
    }
}
struct Cache{loaded_scenes:usize,loaded_atlases:usize,selected:usize,resident_scene:usize,atlas_scene:usize,coverage_scene:usize,coverage_len:usize,metadata_scene:usize,metadata_len:usize,effect_scene:usize,ready:bool,header:[u32;4]}
impl Cache{METHODS
 fn revoke_metadata(&mut self){self.metadata_scene=EMPTY;self.metadata_len=0;}
}
#[test]fn failure_after_any_atlas_or_scene_admission_requires_fresh_uploads(){
    // Failures can follow any atlas chunk, or scene admission after all atlas
    // chunks. Menu restoration overwrites their VRAM, so none may remain ready.
    for scenes in 0..=2 {for atlases in 0..=ATLASES.len() {
        if scenes!=0&&atlases!=ATLASES.len(){continue;}
        for terminal in [cd_stream::Status::Idle,cd_stream::Status::Done,cd_stream::Status::Failed(42)] {
            let mut cache=Cache{loaded_scenes:scenes,loaded_atlases:atlases,
                selected:if scenes==0{EMPTY}else{0},resident_scene:0,atlas_scene:0,coverage_scene:0,coverage_len:64,metadata_scene:0,metadata_len:160,effect_scene:0,ready:true,header:[11,22,33,44]};
            unsafe{HK_COVERAGE_SCENE=1;HK_COVERAGE_BYTES=64;}
            cd_stream::setup(terminal,terminal);cache.reset_bootstrap();
            cd_stream::assert_quiescent(1);
            assert_eq!(cache.loaded_scenes,0);assert_eq!(cache.loaded_atlases,0);
            assert_eq!(cache.selected,EMPTY);assert_eq!(cache.resident_scene,EMPTY);assert_eq!(cache.atlas_scene,EMPTY);assert!(!cache.atlases_ready());
            assert_eq!((cache.coverage_scene,cache.coverage_len),(EMPTY,0));
            assert_eq!(unsafe{(HK_COVERAGE_SCENE,HK_COVERAGE_BYTES)},(0,0));
            for region in 0..REGION_SCENE_LOCAL.len(){assert!(!cache.is_ready(region));}
            // Valid pack metadata can survive; only admission references reset.
            assert!(cache.ready);assert_eq!(cache.header,[11,22,33,44]);
        }
    }}
}
#[test]fn active_transport_is_cancelled_and_drained_before_reset_returns(){
    for terminal in [cd_stream::Status::Done,cd_stream::Status::Failed(9)] {
        let mut cache=Cache{loaded_scenes:1,loaded_atlases:23,selected:0,resident_scene:0,atlas_scene:0,coverage_scene:0,coverage_len:64,metadata_scene:0,metadata_len:160,effect_scene:0,ready:true,header:[1;4]};
        unsafe{DRIVE.clip=true;CLIP_DROPPED=false;}
        cd_stream::setup(cd_stream::Status::Busy,terminal);cache.reset_bootstrap();
        cd_stream::assert_quiescent(3);
        assert!(unsafe{!DRIVE.clip&&CLIP_DROPPED},"a clip read in flight is dropped, not left owning the drive");
        assert_eq!((cache.loaded_scenes,cache.loaded_atlases,cache.selected),(0,0,EMPTY));
    }
}
'''.replace('METHODS', methods)
        for exclusive in ('true',):
            with self.subTest(exclusive=exclusive):self.run_rust(code.replace('EXCLUSIVE',exclusive))
        main = (ROOT / 'game/src/main.rs').read_text()
        retry = main[main.index('while cache.prepare_ambience()'):]
        self.assertLess(retry.index('cache.reset_bootstrap()'), retry.index('menu::restore('))
