//! Exact full-local opaque core mapping. Keys include every mapping input;
//! camera translation only clips the cached interval. No texture lease needed.
#[derive(Clone,Copy)]
#[repr(C,align(4))]
struct Entry {spans:[i16;2],size:[u16;2],core:[u8;4],pixels:[i16;4]}
impl Entry {const EMPTY:Self=Self{spans:[0;2],size:[0;2],core:[0;4],pixels:[0;4]};}
pub struct Cache {entries:[Entry;64]}
#[no_mangle]pub static mut HK_CORE_MAP_CACHE_HITS:u32=0;
#[no_mangle]pub static mut HK_CORE_MAP_CACHE_MISSES:u32=0;
const _:()={assert!(core::mem::size_of::<Entry>()==20);assert!(core::mem::align_of::<Entry>()==4);assert!(core::mem::size_of::<Cache>()==1280);};
impl Cache {
    pub const fn new()->Self{Self{entries:[Entry::EMPTY;64]}}
    #[inline(never)]
    pub fn map(&mut self,verts:[(i16,i16);4],width:u16,height:u16,core:[u8;4])->Option<[i16;4]> {
        let [(x0,y0),(x1,y1),(x2,y2),(x3,y3)]=verts;
        if y0!=y1||x0!=x2||x1!=x3||y2!=y3||core[2]==0||core[3]==0{return None;}
        let dx=x1 as i32-x0 as i32;let dy=y2 as i32-y0 as i32;
        if dx==0||dx.abs()>1023||dy==0||dy.abs()>511||width==0||width>256||height==0||height>256{return None;}
        let right=core[0]as u16+core[2]as u16;let bottom=core[1]as u16+core[3]as u16;
        if right>width||bottom>height{return None;}
        let spans=[dx as i16,dy as i16];let size=[width,height];
        let hash=(dx as u32).wrapping_mul(13)^(dy as u32).wrapping_mul(7)^width as u32^((height as u32)<<1)^core[0]as u32^core[1]as u32^core[2]as u32^core[3]as u32;
        let e=&mut self.entries[(hash&63)as usize];
        if e.spans!=spans||e.size!=size||e.core!=core {
            #[cfg(target_arch="mips")]
            unsafe {HK_CORE_MAP_CACHE_MISSES=HK_CORE_MAP_CACHE_MISSES.wrapping_add(1);}
            let(l,r)=interval(dx.abs(),width,dx<0,core[0]as u16,right);
            let(t,b)=interval(dy.abs(),height,dy<0,core[1]as u16,bottom);
            e.spans=spans;e.size=size;e.core=core;e.pixels=[l,t,r,b];
        }else{
            #[cfg(target_arch="mips")]
            unsafe {HK_CORE_MAP_CACHE_HITS=HK_CORE_MAP_CACHE_HITS.wrapping_add(1);}
        }
        let [l,t,r,b]=e.pixels;let left=x0.min(x1)as i32;let top=y0.min(y2)as i32;
        let l=(left+l as i32).max(0);let r=(left+r as i32).min(320);let t=(top+t as i32).max(0);let b=(top+b as i32).min(240);
        (l<r&&t<b).then_some([l as i16,t as i16,r as i16,b as i16])
    }
}
fn interval(span:i32,texels:u16,reverse:bool,low:u16,high:u16)->(i16,i16) {
    if low==0&&high==texels{return(0,span as i16);}
    let step=(texels as i32-1)*4096/span;
    let seed=(if reverse{texels as i32-1}else{0})*4096+2048;
    if step==0 {let sample=seed>>12;return if low as i32<=sample&&sample<(high as i32){(0,span as i16)}else{(0,0)};}
    let(seed,low,high)=if reverse{(-seed,1-((high as i32)<<12),1-((low as i32)<<12))}else{(seed,(low as i32)<<12,(high as i32)<<12)};
    let bound=|threshold:i32|->i16 {if threshold<=seed{0}else if threshold>seed+(span-1)*step{span as i16}else{((threshold-seed-1)as u32/step as u32+1)as i16}};
    (bound(low),bound(high))
}
