"""Native integration of the production startup uploader with a recording SDK.

This checks guest upload placement and scratch-buffer lifetime without a disc,
retail assets, or emulated GPU. The stubs consume FIFO bytes immediately and
retain outstanding GPU work until the production uploader calls draw_sync.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BootstrapAtlasTests(unittest.TestCase):
    def test_production_upload_layout_lifetime_and_descriptor_guards(self):
        code = r'''
#![allow(dead_code)]
extern crate self as hk_cache;
extern crate self as psx_vram;
extern crate self as psx_rt;
extern crate self as psx_io;
extern crate self as psx_hw;
extern crate self as psx_gpu;
#[path="RESIDENCY"]pub mod residency;
#[path="UPLOADER"]mod vram_cache;
use std::cell::RefCell;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct VramRect{x:u16,y:u16,w:u16,h:u16}
impl VramRect{pub fn new(x:u16,y:u16,w:u16,h:u16)->Self{Self{x,y,w,h}}}
mod disc {
    pub struct AtlasDesc{pub scene_index:usize,pub kind:u8,pub first:usize,pub count:usize,pub raw_len:usize}
    pub const SCENE_COUNT:usize=2;
    pub const SCENE_GATE_LOAD:bool=EXCLUSIVE;
    pub fn scene_index(region:usize)->usize{[0,0,1,1][region]}
    pub fn scene_page_base(scene:usize)->usize{[0,14][scene]}
}
#[derive(Debug,PartialEq)]enum Event{Upload(VramRect),Checkpoint,Wait,Clear,Sync,Reuse}
struct Recorded {
    pixels:Vec<u16>,writes:Vec<u8>,events:Vec<Event>,pending:bool,dma:bool,queued:bool,
}
impl Recorded {
    fn new()->Self{Self{pixels:vec![0xdead;1024*512],writes:vec![0;1024*512],
        events:Vec::new(),pending:false,dma:false,queued:false}}
}
thread_local!{static LOG:RefCell<Recorded>=RefCell::new(Recorded::new());}
fn reset(){LOG.with(|l|*l.borrow_mut()=Recorded::new());}
mod render{pub fn dma_pending()->bool{super::LOG.with(|l|l.borrow().dma)}}
pub mod interrupts{pub fn gp1_queue_pending()->bool{super::LOG.with(|l|l.borrow().queued)}}
mod input{pub fn checkpoint(){super::LOG.with(|l|l.borrow_mut().events.push(super::Event::Checkpoint));}}
mod texture_upload {
    pub fn upload(r:super::VramRect,bytes:&[u8]) {
        assert_eq!(bytes.len(),r.w as usize*r.h as usize*2);
        assert!(r.x as usize+r.w as usize<=1024&&r.y as usize+r.h as usize<=512);
        super::LOG.with(|l|{
            let mut log=l.borrow_mut();log.events.push(super::Event::Upload(r));log.pending=true;
            for (i,word)in bytes.chunks_exact(2).enumerate(){
                let at=(r.y as usize+i/r.w as usize)*1024+r.x as usize+i%r.w as usize;
                assert_eq!(log.writes[at],0,"unexpected duplicate VRAM write at {at}");
                log.writes[at]+=1;log.pixels[at]=u16::from_le_bytes(word.try_into().unwrap());
            }
        });
    }
}
pub mod gpu {
    pub mod gp0{pub const CLEAR_CACHE:u32=0x01000000;}
    pub fn wait_cmd_ready(){super::LOG.with(|l|l.borrow_mut().events.push(super::Event::Wait));}
    pub fn write_gp0(word:u32){assert_eq!(word,gp0::CLEAR_CACHE);super::LOG.with(|l|l.borrow_mut().events.push(super::Event::Clear));}
}
pub fn draw_sync(){LOG.with(|l|{let mut log=l.borrow_mut();assert_eq!(log.events.last(),Some(&Event::Clear));log.pending=false;log.events.push(Event::Sync);});}
fn value(kind:u8,id:usize,word:usize)->u16 {
    ((id as u32*197+word as u32*31+if kind==0{0x1234}else{0xb357})%65521)as u16
}
fn upload_chunk(kind:u8,first:usize,count:usize,staging:&mut[u8]) {
    LOG.with(|l|{let mut log=l.borrow_mut();assert!(!log.pending,"GPU still using previous chunk");log.events.push(Event::Reuse);});
    staging.fill(0xa5);let stride=if kind==0{32768}else{32};let n=stride*count;
    for i in 0..count{for word in 0..stride/2 {
        let at=i*stride+word*2;staging[at..at+2].copy_from_slice(&value(kind,first+i,word).to_le_bytes());
    }}
    vram_cache::upload_atlas(&disc::AtlasDesc{scene_index:0,kind,first,count,raw_len:n},&staging[..n]);
    LOG.with(|l|{
        let log=l.borrow();assert!(!log.pending);
        assert_eq!(&log.events[log.events.len()-4..],&[Event::Wait,Event::Clear,Event::Sync,Event::Checkpoint]);
    });
    // Reuse immediately after return: recorded pixels must retain their bytes.
    staging.fill(0x5a);
}
#[test]fn all_nineteen_pages_and_1209_palettes_are_exact_disjoint_and_complete(){
    reset();let mut staging=vec![0;65536];let mut cache=vram_cache::Cache::new();
    assert!(!cache.ready(0));assert!(!cache.ready(3));
    // Include single- and multiple-page descriptors and cross a ten-page row.
    for (first,count)in[(0,1),(1,2),(3,2),(5,2),(7,2),(9,2),(11,2),(13,1),(14,2),(16,2),(18,1)] {
        upload_chunk(0,first,count,&mut staging);assert!(!cache.ready(0));
    }
    // Cross every CLUT strip boundary within an upload chunk.
    for (first,count)in[(0,397),(397,23),(420,405),(825,384)] {
        upload_chunk(1,first,count,&mut staging);assert!(!cache.ready(3));
    }
    cache.admit_scene(0,true);for region in 0..4{
        assert_eq!(cache.ready(region),!disc::SCENE_GATE_LOAD||region<2);
        if cache.ready(region){assert_eq!(cache.activate(region),region/2);}
    }
    // Independent fixed physical map; do not call page_xy/clut_xy here.
    let mut expected=vec![0xdead;1024*512];let mut touched=vec![false;1024*512];
    for y in 0..512{for x in 384..1024{
        let page=(y/256)*10+(x-384)/64;let word=(y%256)*64+(x-384)%64;
        // Page 19 is the animation cache's second region now, so the atlas
        // path must leave x960..1024,y256..512 untouched.
        if page>=residency::STATIC_PAGES {continue;}
        let at=y*1024+x;expected[at]=value(0,page,word);touched[at]=true;
    }}
    let mut origins=Vec::new();
    for y in 480..500{for x in (0..320).step_by(16){origins.push((x,y));}}
    for y in 480..496{origins.push((336,y));}
    for y in 256..360{for x in (320..384).step_by(16){origins.push((x,y));}}
    for y in 360..464{for x in (320..384).step_by(16){origins.push((x,y));}}
    for (id,&(x,y))in origins.iter().take(1209).enumerate(){for word in 0..16{
        let at=y*1024+x+word;assert!(!touched[at]);touched[at]=true;expected[at]=value(1,id,word);
    }}
    LOG.with(|l|{
        let log=l.borrow();assert_eq!(log.pixels,expected,"VRAM differs, including untouched reservations");
        for (i,&written)in log.writes.iter().enumerate(){assert_eq!(written,u8::from(touched[i]),"write count at {i}");}
        assert_eq!(log.events.iter().filter(|e|matches!(e,Event::Sync)).count(),15);
        assert_eq!(log.events.iter().filter(|e|matches!(e,Event::Upload(_))).count(),1228);
    });
}
#[test]fn scene_replacement_revokes_previous_owner_and_reuploads_shared_slots(){
    if !disc::SCENE_GATE_LOAD{return;}
    reset();let mut cache=vram_cache::Cache::new();
    for (scene,tag)in[(0,0x1234u16),(1,0xabcd),(0,0x5678),(1,0x9abc)] {
        cache.release();for region in 0..4{assert!(!cache.ready(region));}
        LOG.with(|l|{let mut log=l.borrow_mut();assert!(!log.pending);log.writes.fill(0);});
        let bytes:Vec<_>=(0..16384).flat_map(|_|tag.to_le_bytes()).collect();
        vram_cache::upload_atlas(&disc::AtlasDesc{scene_index:scene,kind:0,first:0,count:1,raw_len:32768},&bytes);
        assert!(!cache.ready(scene*2));
        cache.admit_scene(scene*2,true);
        for region in 0..4{assert_eq!(cache.ready(region),region/2==scene);}
        assert_eq!(cache.activate(scene*2+1),scene);
        LOG.with(|l|{let log=l.borrow();
            for y in 0..256{for x in 384..448{assert_eq!(log.pixels[y*1024+x],tag);}}
            assert_eq!(log.pixels[0],0xdead);assert_eq!(log.pixels[480*1024],0xdead);
            assert!(!log.pending);
        });
    }
}
#[test]fn malformed_descriptors_and_active_gpu_cannot_write_or_publish(){
    use std::panic::{catch_unwind,AssertUnwindSafe};
    for (kind,first,count,raw_len,bytes_len)in[
        (0,0,1,32768,32766), // descriptor/payload mismatch
        (0,0,2,32768,32768), // count/payload mismatch
        (1,0,2,32,32),
        (0,20,1,32768,32768), // past final page
        (1,1248,1,32,32), // past admitted CLUT range
        (2,0,1,32,32), // unsupported kind
    ] {
        reset();let bytes=vec![0;bytes_len];
        assert!(catch_unwind(AssertUnwindSafe(||vram_cache::upload_atlas(&disc::AtlasDesc{scene_index:0,kind,first,count,raw_len},&bytes))).is_err());
        LOG.with(|l|{let log=l.borrow();assert!(log.writes.iter().all(|&n|n==0));assert!(!log.pending);});
    }
    for queued in [false,true] {
        reset();LOG.with(|l|{let mut log=l.borrow_mut();log.dma=!queued;log.queued=queued;});
        assert!(catch_unwind(||vram_cache::upload_atlas(&disc::AtlasDesc{scene_index:0,kind:1,first:0,count:1,raw_len:32},&[0;32])).is_err());
        LOG.with(|l|assert!(l.borrow().events.is_empty()));
    }
    let mut cache=vram_cache::Cache::new();
    assert!(catch_unwind(AssertUnwindSafe(||cache.admit_scene(0,false))).is_err());
    assert!(!cache.ready(0));assert!(catch_unwind(AssertUnwindSafe(||cache.activate(0))).is_err());
}
'''.replace('RESIDENCY', str(ROOT / 'shared/hk-cache/src/residency.rs')).replace(
            'UPLOADER', str(ROOT / 'game/src/vram_cache.rs'))
        (ROOT / '.hkpsx').mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='bootstrap-atlas-test-', dir=ROOT / '.hkpsx') as temp:
            path = Path(temp)
            for exclusive in ('false','true'):
                with self.subTest(exclusive=exclusive):
                    (path / 'probe.rs').write_text(code.replace('EXCLUSIVE',exclusive))
                    built = subprocess.run(['rustc', '--edition=2021', '--test', str(path / 'probe.rs'),
                                            '-o', str(path / 'probe')], capture_output=True, text=True)
                    self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
                    tested = subprocess.run([str(path / 'probe'), '--test-threads=1'], capture_output=True, text=True)
                    self.assertEqual(tested.returncode, 0, tested.stdout + tested.stderr)
                    self.assertIn('all_nineteen_pages_and_1209_palettes_are_exact_disjoint_and_complete', tested.stdout)
