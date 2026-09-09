//! Exact shared scene atlases. Every supported scene is loaded once at entry.
//! The host assigns disjoint page/CLUT ranges; spatial regions share them.
use hk_cache::residency;
use hk_format::Room;
use psx_vram::VramRect;
const FRAME_BYTES:usize=64*1024;
pub struct Cache {ready:[bool;crate::disc::SCENE_COUNT],pending:Option<usize>,page:usize,row:usize,palette:usize}
#[no_mangle] pub static mut HK_VRAM_UPLOAD_CANCELLATIONS:u32=0;
#[no_mangle] pub static mut HK_VRAM_UPLOAD_BYTES:u32=0;
#[no_mangle] pub static mut HK_VRAM_UPLOAD_MAX_FRAME:u32=0;
#[no_mangle] pub static mut HK_VRAM_BANK_HITS:u32=0;
#[no_mangle] pub static mut HK_VRAM_BANK_MISSES:u32=0;
#[no_mangle] pub static mut HK_VRAM_ACTIVE_BANK:u32=u32::MAX;
#[no_mangle] pub static mut HK_VRAM_PENDING_REGION:u32=0;
impl Cache {
    pub const fn new()->Self{Self{ready:[false;crate::disc::SCENE_COUNT],pending:None,page:0,row:0,palette:0}}
    pub fn ready(&self,region:usize)->bool{self.ready[crate::disc::scene_index(region)]}
    pub fn pending(&self)->Option<usize>{self.pending}
    // Allocations are disjoint and persistent. Completing another scene never
    // evicts demand textures or needs an upload cancellation.
    pub fn cancel_for_demand(&mut self,_region:usize)->bool{false}
    pub fn activate(&mut self,region:usize)->usize{
        let scene=crate::disc::scene_index(region);assert!(self.ready[scene]);
        unsafe {HK_VRAM_ACTIVE_BANK=scene as u32;HK_VRAM_BANK_HITS+=1;}
        scene
    }
    pub fn begin(&mut self,region:usize,room:&Room,_keep:&[usize])->bool{
        if self.pending.is_some()||self.ready(region){return false;}
        let scene=crate::disc::scene_index(region);
        assert!(crate::disc::scene_page_base(scene)+room.counts[0]<=residency::BANKS*residency::PAGES);
        assert!(crate::disc::scene_palette_base(scene)+room.palettes().len()/32<=residency::SCENE_CLUTS);
        self.pending=Some(region);self.page=0;self.row=0;self.palette=0;
        unsafe{HK_VRAM_BANK_MISSES+=1;HK_VRAM_PENDING_REGION=region as u32+1;}
        true
    }
    pub fn pump(&mut self,room:&Room)->bool{
        let Some(region)=self.pending else{return true;};
        let scene=crate::disc::scene_index(region);
        crate::input::checkpoint();
        assert!(!crate::render::dma_pending()&&!psx_rt::interrupts::gp1_queue_pending());
        let mut bytes=0;
        while self.page<room.counts[0]&&bytes+128<=FRAME_BYTES{
            let rows=(256-self.row).min((FRAME_BYTES-bytes)/128);
            let page=crate::disc::scene_page_base(scene)+self.page;
            let(x,y)=residency::page_xy(page/residency::PAGES,page%residency::PAGES);
            crate::texture_upload::upload(VramRect::new(x,y+self.row as u16,64,rows as u16),&room.page(self.page)[self.row*128..(self.row+rows)*128]);
            crate::input::checkpoint();bytes+=rows*128;self.row+=rows;
            if self.row==256{self.page+=1;self.row=0;}
        }
        let palettes=room.palettes().len()/32;
        while self.page==room.counts[0]&&self.palette<palettes&&bytes+32<=FRAME_BYTES{
            let palette=crate::disc::scene_palette_base(scene)+self.palette;
            let(x,y)=residency::clut_xy(palette/residency::CLUTS,palette%residency::CLUTS);
            crate::texture_upload::upload(VramRect::new(x,y,16,1),&room.palettes()[self.palette*32..self.palette*32+32]);
            self.palette+=1;bytes+=32;
            if self.palette&31==0{crate::input::checkpoint();}
        }
        unsafe{HK_VRAM_UPLOAD_BYTES=HK_VRAM_UPLOAD_BYTES.saturating_add(bytes as u32);HK_VRAM_UPLOAD_MAX_FRAME=HK_VRAM_UPLOAD_MAX_FRAME.max(bytes as u32);}
        if self.page==room.counts[0]&&self.palette==palettes{
            psx_io::gpu::wait_cmd_ready();psx_io::gpu::write_gp0(psx_hw::gpu::gp0::CLEAR_CACHE);psx_gpu::draw_sync();
            self.ready[scene]=true;self.pending=None;crate::input::checkpoint();
            unsafe{HK_VRAM_PENDING_REGION=0;}
            true
        }else{false}
    }
}
