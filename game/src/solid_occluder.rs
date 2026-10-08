//! Conservative interior rectangles of certified fully opaque child quads.
//!
//! Outputs only suppress earlier draws; original rendering is unchanged. The
//! actual child uses triangles 0-1-2 and 1-3-2. Require convex boundary 0-1-3-2,
//! then their top-left raster rules meet exactly on the shared diagonal. Four
//! boundary half-planes inset two pixels dominate pixel-center/edge rounding.
//! Callers must certify that every possible child texel is opaque. No UV or
//! color assumption can be inferred from geometry alone.
#[derive(Clone,Copy)]
struct Plane {x:i32,y:i32,c:i32} // x*pixel_x+y*pixel_y >= c
impl Plane {
    fn interval(self,y:i32,l:&mut i32,r:&mut i32)->bool {
        let n=self.c-self.y*y;
        if self.x>0 {
            let q=n/self.x;let rem=n%self.x;
            *l=(*l).max(q+i32::from(rem>0));
        } else if self.x<0 {
            let q=n/self.x;let rem=n%self.x;
            *r=(*r).min(q-i32::from(rem>0));
        } else if n>0 {return false;}
        *l<=*r
    }
}
fn bounded_rectangle(xy:[(i16,i16);4],bounds:[i32;4])->Option<[i16;4]> {
    let p=xy.map(|(x,y)|(x as i32,y as i32));
    if p.iter().any(|&(x,y)|!(-1024..=1023).contains(&x)||!(-1024..=1023).contains(&y)){return None;}
    for (a,b) in [(0,1),(1,2),(2,0),(1,3),(3,2)] {
        if (p[a].0-p[b].0).abs()>1023 || (p[a].1-p[b].1).abs()>511
            {return None;}
    }
    let dx1=p[1].0-p[0].0;let dy1=p[1].1-p[0].1;
    let dx2=p[2].0-p[0].0;let dy2=p[2].1-p[0].1;
    let det=dx1*dy2-dx2*dy1;
    // Optimization-only admission: do not spend setup on tiny first triangles.
    if det.abs()<4096 {return None;}
    let orient=if det>0 {1}else{-1};
    let edge=|a:usize,b:usize| {
        let dx=p[b].0-p[a].0;let dy=p[b].1-p[a].1;
        Plane{x:-dy*orient,y:dx*orient,c:(dx*p[a].1-dy*p[a].0)*orient+2*(dx.abs()+dy.abs())}
    };
    let planes=[edge(0,1),edge(1,3),edge(3,2),edge(2,0)];
    // Convex boundary and actual original diagonal topology required.
    for (a,b) in [(0,1),(1,3),(3,2),(2,0)] {
        let dx=p[b].0-p[a].0;let dy=p[b].1-p[a].1;
        if p.iter().any(|q|(dx*(q.1-p[a].1)-dy*(q.0-p[a].0))*orient<0){return None;}
    }
    let top=p.iter().map(|q|q.1).min().unwrap().max(bounds[1]);
    let bottom=p.iter().map(|q|q.1).max().unwrap().min(bounds[3])-1;
    if top>=bottom {return None;}
    let mut rows=[(0i32,0i32,0i32);9];
    for (k,row) in rows.iter_mut().enumerate() {
        let y=top+((bottom-top)*k as i32)/8;let(mut l,mut r)=(bounds[0],bounds[2]-1);
        for &plane in &planes {if !plane.interval(y,&mut l,&mut r) {l=1;r=0;break;}}
        *row=(y,l,r);
    }
    let mut best_area=2047;let mut best=None;
    for a in 0..8 {for b in a+1..9 {
        let (ya,la,ra)=rows[a];let(yb,lb,rb)=rows[b];
        let l=la.max(lb);let r=ra.min(rb);
        if l>r || ya>=yb {continue;}
        let area=(r-l+1)*(yb-ya+1);
        if area>best_area {best_area=area;best=Some([l as i16,ya as i16,(r+1)as i16,(yb+1)as i16]);}
    }}
    // Sample the long interior strip of a nearly horizontal convex quad.
    // Sorted middle vertex heights bound it; the same four planes remain
    // the only correctness test. Diamond-like cases keep the regular search.
    let mut heights=p.map(|q|q.1);
    for (a,b) in [(0,1),(2,3),(0,2),(1,3),(1,2)] {if heights[a]>heights[b] {heights.swap(a,b);}}
    let ya=(heights[1]+2).max(top);let yb=(heights[2]-2).min(bottom);
    if ya<yb {
        let(mut l,mut r)=(bounds[0],bounds[2]-1);
        for y in [ya,yb] {for &plane in &planes {if !plane.interval(y,&mut l,&mut r) {l=1;r=0;break;}}}
        let area=if l<=r {(r-l+1)*(yb-ya+1)}else{0};
        if area>best_area {best=Some([l as i16,ya as i16,(r+1)as i16,(yb+1)as i16]);}
    }
    best
}

#[derive(Clone,Copy)]
struct Entry {key:[u32;3],rect:[i16;4],state:u8}
impl Entry {const EMPTY:Self=Self{key:[0;3],rect:[0;4],state:0};}
/// Four-way, eight-set cache of full-local-domain convex child rectangles.
/// Camera translation only translates/clips the answer. Every relative child
/// vertex is in the key; opacity certification remains the caller's job.
pub struct Cache {entries:[Entry;32],next:[u8;8],hits:u32,misses:u32}
impl Cache {
    pub const fn new()->Self {Self{entries:[Entry::EMPTY;32],next:[0;8],hits:0,misses:0}}
    pub fn stats(&self)->(u32,u32){(self.hits,self.misses)}
    pub fn map(&mut self,xy:[(i16,i16);4])->Option<[i16;4]> {
        if xy.iter().any(|&(x,y)|!(-1024..=1023).contains(&x)||!(-1024..=1023).contains(&y)){return None;}
            let origin=(xy[0].0 as i32,xy[0].1 as i32);
        let local=xy.map(|(x,y)|(x as i32-origin.0,y as i32-origin.1));
        for (a,b) in [(0,1),(1,2),(2,0),(1,3),(3,2)] {if (local[a].0-local[b].0).abs()>1023 || (local[a].1-local[b].1).abs()>511 {return None;}}
        let word=|p:(i32,i32)|p.0 as i16 as u16 as u32|((p.1 as i16 as u16 as u32)<<16);
        let key=[word(local[1]),word(local[2]),word(local[3])];
        let hash=key[0]^key[1].rotate_left(11)^key[2].rotate_left(22);
        let set=((hash^(hash>>8)^(hash>>16)^(hash>>24))as usize)&7;
        let mut found=None;let mut empty=None;
        for k in 0..4 {let i=set*4+k;let e=&self.entries[i];if e.state==0 {empty=Some(i);continue;}
            if ((key[0]^e.key[0])|(key[1]^e.key[1])|(key[2]^e.key[2]))==0 {found=Some(i);break;}}
        let index=if let Some(i)=found {self.hits=self.hits.wrapping_add(1);i}else {
            self.misses=self.misses.wrapping_add(1);
            let i=empty.unwrap_or_else(||{let k=self.next[set]as usize;self.next[set]=(self.next[set]+1)&3;set*4+k});
            let bounds=[local.iter().map(|p|p.0).min().unwrap(),local.iter().map(|p|p.1).min().unwrap(),local.iter().map(|p|p.0).max().unwrap(),local.iter().map(|p|p.1).max().unwrap()];
            let r=bounded_rectangle(local.map(|(x,y)|(x as i16,y as i16)),bounds);
            // Explicit field writes avoid a returned/copied large scratch struct.
            self.entries[i].key=key;
            if let Some(r)=r {self.entries[i].rect=r;self.entries[i].state=2;}else{self.entries[i].state=1;}
            i
        };
        let e=&self.entries[index];if e.state!=2{return None;}let r=e.rect;
        let out=[(r[0]as i32+origin.0).max(0),(r[1]as i32+origin.1).max(0),(r[2]as i32+origin.0).min(320),(r[3]as i32+origin.1).min(240)];
        if out[0]>=out[2] || out[1]>=out[3] {None}else{Some(out.map(|v|v as i16))}
    }
}
