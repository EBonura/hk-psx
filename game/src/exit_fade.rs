//! The last part of a gate's exit fade, run while the next scene loads.
//!
//! The source fades the camera out over 0.33 s (CameraFade, FadingOut). The
//! first ticks of that fade are ordinary frames (frame::Exit); then the scene
//! is left and the load starts, and the rest of the fade is done from here,
//! once per VBlank from every load checkpoint: the frame last shown stays
//! untouched in its buffer and is copied, darker, into the other buffer,
//! which is shown instead. Nothing here reads the scene arena, the static
//! pages or the coverage arena, so the load may replace all of them at once;
//! only the two framebuffers (x 0..320, y 0..480) are used.
use psx_hw::gpu::{gp0,gp1,pack_color,pack_vertex,pack_texcoord,pack_xy};
use psx_io::gpu::{wait_cmd_ready,write_gp0};
struct Fade {armed:bool,active:bool,shown:u16,other:u16,from:u32,steps:u32,step:u32,last:u32,mode:u32}
static mut FADE:Fade=Fade {armed:false,active:false,shown:0,other:0,from:0,steps:0,step:0,last:0,mode:0};
#[no_mangle]pub static mut HK_EXIT_FADE_STEPS:u32=0;
fn fade()->&'static mut Fade {unsafe {&mut *(&raw mut FADE)}}
/// Frame side: the next scene load continues a fade already `shade`/255 dark
/// with `steps` more VBlanks to black.
pub fn arm(shade:u8,steps:u32) {
    let f=fade();f.armed=true;f.from=255-shade as u32;f.steps=steps.max(1);
}
pub fn armed()->bool {fade().armed}
/// Load side, with the GPU idle: start from the frame shown in buffer row
/// `shown`, drawing into buffer row `other`.
pub fn start(shown:u16,other:u16) {
    let f=fade();
    if !f.armed {return;}
    f.armed=false;f.active=true;f.shown=shown;f.other=other;f.step=0;
    f.last=psx_rt::interrupts::vblank_count();
    // GPUSTAT 0..10 mirror GP0(E1h); restore them afterwards so dither and
    // draw-to-display stay what the renderer set.
    f.mode=psx_io::gpu::gpustat().bits()&0x7ff;
}
pub fn active()->bool {fade().active}
/// One step per VBlank. Called from every checkpoint; cheap when idle.
pub fn service() {
    let f=fade();
    if !f.active {return;}
    let now=psx_rt::interrupts::vblank_count();
    if now==f.last {return;}
    f.last=now;f.step+=1;
    // Brightness left, 0..=255: linear from where the live frames stopped.
    let level=f.from*(f.steps-f.step.min(f.steps))/f.steps;
    copy_dimmed(f.shown,f.other,(level*128/255) as u8);
    // Written directly, not queued at VBlank: the load's VRAM uploads refuse
    // to start with a GP1 word pending. At worst one frame shows a seam
    // between two fade levels.
    if f.step==1 {psx_io::gpu::write_gp1(gp1::display_start(0,f.other as u32));}
    unsafe {HK_EXIT_FADE_STEPS+=1;}
    if f.step>=f.steps {
        // Black: clear both buffers so no old pixel can come back, restore
        // the draw mode, and hand the screen to the load.
        wait_cmd_ready();write_gp0(gp0::draw_mode(f.mode&15,(f.mode>>4)&1,(f.mode>>5)&3,(f.mode>>7)&3,f.mode&0x200!=0,f.mode&0x400!=0));
        psx_gpu::fill_rect(0,0,320,480,0,0,0);
        // Show the other buffer (black too) so the renderer's first frame,
        // drawn into this one, is never on screen while it is drawn.
        psx_io::gpu::write_gp1(gp1::display_start(0,f.shown as u32));
        crate::gate_probe::note(31,now);
        f.active=false;
    }
}
/// Copy the 320x240 frame at row `src` to row `dst`, texels scaled by
/// `modulate`/128. 15-bit texture pages are 256 texels wide and start on
/// rows 0 or 256, so the copy is cut at x 256 and at row 256.
fn copy_dimmed(src:u16,dst:u16,modulate:u8) {
    wait_cmd_ready();write_gp0(gp0::draw_area_top_left(0,dst as u32));
    wait_cmd_ready();write_gp0(gp0::draw_area_bottom_right(319,dst as u32+239));
    wait_cmd_ready();write_gp0(gp0::draw_offset(0,dst as i32));
    let mut row=0u16;
    while row<240 {
        let y=src+row;
        let page_y=u32::from(y>=256);
        let rows=if page_y==0 {(256-y).min(240-row)} else {240-row};
        for (x,w) in [(0u16,256u16),(256,64)] {
            wait_cmd_ready();write_gp0(gp0::draw_mode(x as u32/64,page_y,0,2,false,true));
            wait_cmd_ready();
            // GP0(64h): textured rectangle, variable size, blended with the colour.
            write_gp0(0x6400_0000|pack_color(modulate,modulate,modulate));
            write_gp0(pack_vertex(x as i16,row as i16));
            write_gp0(pack_texcoord(0,(y&255) as u8,0));
            write_gp0(pack_xy(w,rows));
        }
        row+=rows;
    }
}
