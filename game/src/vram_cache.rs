//! Static atlases are uploaded before compact resident scenes are admitted.
//! The persistent allocation never needs a traversal upload or eviction.
use hk_cache::residency;
use psx_vram::VramRect;
pub struct Cache {ready:bool}
#[no_mangle] pub static mut HK_VRAM_UPLOAD_CANCELLATIONS:u32=0;
#[no_mangle] pub static mut HK_VRAM_UPLOAD_BYTES:u32=0;
#[no_mangle] pub static mut HK_VRAM_UPLOAD_MAX_FRAME:u32=0;
#[no_mangle] pub static mut HK_VRAM_BANK_HITS:u32=0;
#[no_mangle] pub static mut HK_VRAM_BANK_MISSES:u32=0;
#[no_mangle] pub static mut HK_VRAM_ACTIVE_BANK:u32=u32::MAX;
#[no_mangle] pub static mut HK_VRAM_PENDING_REGION:u32=0;
impl Cache {
    pub const fn new()->Self {Self{ready:false}}
    /// The loader has verified and uploaded every atlas descriptor before it
    /// publishes any immutable compact Scene. No source payload is retained.
    pub fn admit_bootstrap(&mut self,uploaded:bool) {
        assert!(uploaded);self.ready=true;
    }
    pub fn ready(&self,region:usize)->bool {
        let _=crate::disc::scene_index(region);self.ready
    }
    pub fn activate(&mut self,region:usize)->usize {
        let scene=crate::disc::scene_index(region);assert!(self.ready);
        unsafe {HK_VRAM_ACTIVE_BANK=scene as u32;HK_VRAM_BANK_HITS+=1;}
        scene
    }
}
/// Called only while the loader exclusively owns startup scratch. FIFO upload
/// plus draw_sync completes before the next CD read can reuse these bytes.
/// The generated descriptor inventory is checked for exact, disjoint page and
/// palette coverage by Cache::init before any upload starts.
pub fn upload_atlas(a:&crate::disc::AtlasDesc,bytes:&[u8]) {
    assert_eq!(bytes.len(),a.raw_len);
    assert!(!crate::render::dma_pending()&&!psx_rt::interrupts::gp1_queue_pending());
    crate::input::checkpoint();
    match a.kind {
        0=>{
            assert_eq!(bytes.len(),a.count*32768);
            assert!(a.first+a.count<=residency::BANKS*residency::PAGES);
            for i in 0..a.count {
                let page=a.first+i;
                let(x,y)=residency::page_xy(page/residency::PAGES,page%residency::PAGES);
                crate::texture_upload::upload(VramRect::new(x,y,64,256),&bytes[i*32768..(i+1)*32768]);
                crate::input::checkpoint();
            }
        },
        1=>{
            assert_eq!(bytes.len(),a.count*32);
            assert!(a.first+a.count<=residency::SCENE_CLUTS);
            for i in 0..a.count {
                let palette=a.first+i;
                let(x,y)=residency::clut_xy(palette/residency::CLUTS,palette%residency::CLUTS);
                crate::texture_upload::upload(VramRect::new(x,y,16,1),&bytes[i*32..(i+1)*32]);
                if i&31==0 {crate::input::checkpoint();}
            }
        },
        _=>panic!("atlas kind"),
    }
    psx_io::gpu::wait_cmd_ready();psx_io::gpu::write_gp0(psx_hw::gpu::gp0::CLEAR_CACHE);psx_gpu::draw_sync();
    unsafe {
        HK_VRAM_UPLOAD_BYTES=HK_VRAM_UPLOAD_BYTES.saturating_add(bytes.len()as u32);
        HK_VRAM_UPLOAD_MAX_FRAME=HK_VRAM_UPLOAD_MAX_FRAME.max(bytes.len()as u32);
        // One admission per scene, rather than one per independent atlas chunk.
        if a.kind==0 {
            for scene in 0..crate::disc::SCENE_COUNT {
                let first=crate::disc::scene_page_base(scene);
                if (a.first..a.first+a.count).contains(&first) {HK_VRAM_BANK_MISSES+=1;}
            }
        }
    }
    crate::input::checkpoint();
}
