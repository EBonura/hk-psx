"""Resident traversal API tests plus retained legacy residency policy tests.

The guest no longer schedules room reads during traversal. Compile the actual
resident methods with small admitted-scene metadata and no CD/decoder service;
any accidental dependency on such a service fails this harness. The standalone
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
    match = re.search(r'^    pub fn ' + re.escape(name) + r'\(', source, re.MULTILINE)
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
    def run_rust(self, code):
        (ROOT / '.hkpsx').mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='resident-cache-test-', dir=ROOT / '.hkpsx') as temp:
            path = Path(temp)
            (path / 'probe.rs').write_text(code)
            compiled = subprocess.run(
                ['rustc', '--edition=2021', '--test', str(path / 'probe.rs'), '-o', str(path / 'probe')],
                capture_output=True, text=True)
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
            'protect_upload', 'pending', 'is_ready'))
        code = r'''
#![allow(dead_code)]
const EMPTY:usize=usize::MAX;
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
struct Cache{loaded_scenes:usize,selected:usize}
impl Cache{
 fn admitted(count:usize)->Self{
  unsafe{HK_REGION_ACTIVATIONS=0;HK_ROOM_CACHE_HITS=0;HK_ROOM_BYTES=0;STATUS=(0,0);}
  Self{loaded_scenes:count,selected:EMPTY}
 }
METHODS
}
#[test]fn cold_and_partial_admission_do_not_publish_missing_scene_or_schedule_work(){
 for count in 0..=1{
  let mut c=Cache::admitted(count);
  for region in 0..REGION_SCENE_LOCAL.len(){
   assert_eq!(c.is_ready(region),scene_index(region)<count);
   assert_eq!(c.request(region),Ok(scene_index(region)<count));
  }
  assert_eq!(c.try_select(3),Ok(false));assert_eq!(c.selected,EMPTY);
  for _ in 0..100{c.prefetch_regions(&[3,4,5]);c.pump().unwrap();}
  assert_eq!(c.loaded_scenes,count);assert_eq!(c.pending(),None);
  assert!(!c.protect_upload(Some(3)));assert!(c.protect_upload(None));
 }
}
#[test]fn all_admitted_regions_support_reentry_and_scene_crossing_without_eviction(){
 let mut c=Cache::admitted(2);
 for (step,region) in [0,1,2,5,4,3,0,5,0].into_iter().enumerate(){
  c.prefetch_regions(&[usize::MAX,region,region]);c.pump().unwrap();
  assert_eq!(c.request(region),Ok(true));assert!(c.protect_upload(Some(region)));
  assert_eq!(c.try_select(region),Ok(true));assert_eq!(c.selected,region);
  assert_eq!(unsafe{HK_ROOM_BYTES},SCENE_MANIFEST[scene_index(region)].raw_len as u32);
  assert_eq!(unsafe{HK_REGION_ACTIVATIONS},step as u32+1);
  assert_eq!(unsafe{HK_ROOM_CACHE_HITS},step as u32);
  assert_eq!(unsafe{STATUS},(2,0));assert_eq!(c.pending(),None);
  for other in 0..6{assert!(c.is_ready(other));}
  assert_eq!(c.try_select(region),Ok(true));
  assert_eq!(unsafe{HK_REGION_ACTIVATIONS},step as u32+1);
  assert!(!c.prefetch(region));assert!(c.protect_upload(None));
 }
}
#[test]fn invalid_region_requests_cannot_change_selected_view_or_readiness(){
 let mut c=Cache::admitted(2);c.try_select(2).unwrap();
 for invalid in [6,usize::MAX]{
  assert_eq!(c.request(invalid),Err(LoadError::RoomMismatch));
  assert_eq!(c.try_select(invalid),Err(LoadError::RoomMismatch));
  assert!(!c.is_ready(invalid));assert!(!c.protect_upload(Some(invalid)));
  assert_eq!(c.selected,2);assert_eq!(c.loaded_scenes,2);
 }
 assert_eq!(unsafe{HK_REGION_ACTIVATIONS},1);
}
'''.replace('METHODS', methods)
        self.run_rust(code)

    def test_production_bootstrap_retry_discards_admission_before_menu_reuse(self):
        source = (ROOT / 'game/src/disc.rs').read_text()
        methods = '\n'.join(method(source, name) for name in (
            'reset_bootstrap', 'is_ready', 'atlases_ready'))
        code = r'''
#![allow(dead_code)]
const EMPTY:usize=usize::MAX;
const REGION_SCENE_LOCAL:&[(usize,usize)]=&[(0,0),(0,1),(1,0)];
const ATLASES:&[u8]=&[0;23];
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
struct Cache{loaded_scenes:usize,loaded_atlases:usize,selected:usize,ready:bool,header:[u32;4]}
impl Cache{METHODS}
#[test]fn failure_after_any_atlas_or_scene_admission_requires_fresh_uploads(){
    // Failures can follow any atlas chunk, or scene admission after all atlas
    // chunks. Menu restoration overwrites their VRAM, so none may remain ready.
    for scenes in 0..=2 {for atlases in 0..=ATLASES.len() {
        if scenes!=0&&atlases!=ATLASES.len(){continue;}
        for terminal in [cd_stream::Status::Idle,cd_stream::Status::Done,cd_stream::Status::Failed(42)] {
            let mut cache=Cache{loaded_scenes:scenes,loaded_atlases:atlases,
                selected:if scenes==0{EMPTY}else{0},ready:true,header:[11,22,33,44]};
            cd_stream::setup(terminal,terminal);cache.reset_bootstrap();
            cd_stream::assert_quiescent(1);
            assert_eq!(cache.loaded_scenes,0);assert_eq!(cache.loaded_atlases,0);
            assert_eq!(cache.selected,EMPTY);assert!(!cache.atlases_ready());
            for region in 0..REGION_SCENE_LOCAL.len(){assert!(!cache.is_ready(region));}
            // Valid pack metadata can survive; only admission references reset.
            assert!(cache.ready);assert_eq!(cache.header,[11,22,33,44]);
        }
    }}
}
#[test]fn active_transport_is_cancelled_and_drained_before_reset_returns(){
    for terminal in [cd_stream::Status::Done,cd_stream::Status::Failed(9)] {
        let mut cache=Cache{loaded_scenes:1,loaded_atlases:23,selected:0,ready:true,header:[1;4]};
        cd_stream::setup(cd_stream::Status::Busy,terminal);cache.reset_bootstrap();
        cd_stream::assert_quiescent(3);
        assert_eq!((cache.loaded_scenes,cache.loaded_atlases,cache.selected),(0,0,EMPTY));
    }
}
'''.replace('METHODS', methods)
        self.run_rust(code)
        main = (ROOT / 'game/src/main.rs').read_text()
        retry = main[main.index('while cache.prepare_ambience()'):]
        self.assertLess(retry.index('cache.reset_bootstrap()'), retry.index('menu::restore()'))
