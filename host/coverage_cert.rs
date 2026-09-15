// SPDX-License-Identifier: GPL-2.0-or-later
//! Host-only opaque tile proof. Input/output stay in the ignored cook cache.
//! Intersect exact PS1 texel opacity across every source rounding phase, then
//! admit a4px/8px-grid bit only when its full19x19/23x23 window is opaque.
mod coverage_raster;
use coverage_raster::{tri_plane_eval,tri_raster_setup,tri_span_x};
use std::{env,fs};
struct Reader { bytes:Vec<u8>, at:usize }
impl Reader {
    fn take<const N:usize>(&mut self)->[u8;N] {let end=self.at.checked_add(N).expect("input offset");let a=self.bytes.get(self.at..end).expect("truncated certificate input").try_into().unwrap();self.at=end;a}
    fn u32(&mut self)->u32 {u32::from_le_bytes(self.take())}
    fn i32(&mut self)->i32 {i32::from_le_bytes(self.take())}
    fn u16(&mut self)->u16 {u16::from_le_bytes(self.take())}
}
fn integral(a:&[u8],w:usize,h:usize)->Vec<u32> {
    let mut p=vec![0;(w+1)*(h+1)];
    for y in 0..h {let mut row=0;for x in 0..w {row+=a[y*w+x]as u32;p[(y+1)*(w+1)+x+1]=p[y*(w+1)+x+1]+row;}}
    p
}
fn sum(p:&[u32],w:usize,l:usize,t:usize,r:usize,b:usize)->u32 {p[b*(w+1)+r]+p[t*(w+1)+l]-p[t*(w+1)+r]-p[b*(w+1)+l]}
fn certify(phases:&[[(i32,i32);4]],tw:usize,th:usize,words:&[u16],shift:u32)->([i16;2],[u16;2],Vec<u32>) {
    assert!(shift==2||shift==3);let step=1i32<<shift;let window=16+step-1;
    assert!(!phases.is_empty()&&phases.len()<=16&&tw>0&&th>0&&tw<=63&&th<=63);
    assert_eq!(words.len(),tw*th);
    let mut bounds=[i32::MAX,i32::MAX,i32::MIN,i32::MIN];
    for v in phases {
        assert_eq!(v[0],(0,0));
        assert!(v.iter().all(|&(x,y)|(-1024..=1023).contains(&x)&&(-1024..=1023).contains(&y)));
        for(a,b)in[(0,1),(1,2),(2,0),(1,3),(3,2)]{assert!((v[a].0-v[b].0).abs()<=1023&&(v[a].1-v[b].1).abs()<=511);}
        for&(x,y)in v {bounds[0]=bounds[0].min(x);bounds[1]=bounds[1].min(y);bounds[2]=bounds[2].max(x+1);bounds[3]=bounds[3].max(y+1);}
    }
    let w=(bounds[2]-bounds[0])as usize;let h=(bounds[3]-bounds[1])as usize;
    assert!(w<=1025&&h<=513);
    let mut opaque=vec![1u8;w*h];
    let uv=[(0,0),(tw as i32-1,0),(0,th as i32-1),(tw as i32-1,th as i32-1)];
    for v in phases {
        let mut here=vec![0u8;w*h];
        for ids in[[0,1,2],[1,3,2]] {
            let Some(t)=tri_raster_setup(ids.map(|i|v[i]),[(0,0,0);3],ids.map(|i|uv[i]),true)else{continue;};
            for(y0,y1,l,dl,r,dr)in t.parts {for y in y0..y1 {let k=(y-y0)as i64;
                for x in tri_span_x(l+k*dl)..tri_span_x(r+k*dr) {
                    assert!(x>=bounds[0]&&x<bounds[2]&&y>=bounds[1]&&y<bounds[3],"raster escaped proven bounds");
                    let u=tri_plane_eval(t.planes[3],x,y)as usize;let v=tri_plane_eval(t.planes[4],x,y)as usize;
                    // Unknown neighboring atlas samples never certify opacity.
                    if u>=tw||v>=th {continue;}
                    let word=words[v*tw+u];
                    if word!=0&&word&0x8000==0 {here[(y-bounds[1])as usize*w+(x-bounds[0])as usize]=1;}
                }
            }}
        }
        for(a,b)in opaque.iter_mut().zip(here){*a&=b;}
    }
    let op=integral(&opaque,w,h);let gx=bounds[0]>>shift;let gy=bounds[1]>>shift;
    let gw=(((bounds[2]+step-1)>>shift)-gx)as usize;let gh=(((bounds[3]+step-1)>>shift)-gy)as usize;
    let mut bits=vec![false;gw*gh];let(mut x0,mut y0,mut x1,mut y1)=(gw,gh,0,0);
    for y in 0..gh {for x in 0..gw {
        let l=(gx+x as i32)*step-bounds[0];let t=(gy+y as i32)*step-bounds[1];let r=l+window;let b=t+window;
        if l>=0&&t>=0&&r<=w as i32&&b<=h as i32&&sum(&op,w,l as usize,t as usize,r as usize,b as usize)==(window*window)as u32 {
            bits[y*gw+x]=true;x0=x0.min(x);y0=y0.min(y);x1=x1.max(x+1);y1=y1.max(y+1);
        }
    }}
    if x0>=x1||y0>=y1{return([0,0],[0,0],Vec::new());}
    let width=x1-x0;let height=y1-y0;let mut packed=vec![0u32;(width*height+31)/32];
    for y in y0..y1 {for x in x0..x1 {if bits[y*gw+x] {let bit=(y-y0)*width+x-x0;packed[bit/32]|=1<<(bit%32);}}}
    ([(gx+x0 as i32)as i16,(gy+y0 as i32)as i16],[width as u16,height as u16],packed)
}
fn main() {
    let a:Vec<_>=env::args().collect();assert!(a.len()==3||a.len()==4,"coverage_cert INPUT OUTPUT [GRID_SHIFT]");let shift=a.get(3).map_or(3,|v|v.parse::<u32>().expect("grid shift"));
    let mut r=Reader{bytes:fs::read(&a[1]).expect("certificate input"),at:0};assert_eq!(&r.take::<8>(),b"HKTCIN01");let count=r.u32();assert!(count<=65535);
    let mut out=b"HKTCOT01".to_vec();out.extend(count.to_le_bytes());
    for expected in 0..count {
        let id=r.u32();assert_eq!(id,expected);let n=r.u32()as usize;assert!((1..=16).contains(&n));let w=r.u16()as usize;let h=r.u16()as usize;assert!(w>0&&h>0&&w<=63&&h<=63);
        let phases=(0..n).map(|_|core::array::from_fn(|_|(r.i32(),r.i32()))).collect::<Vec<_>>();let words=(0..w*h).map(|_|r.u16()).collect::<Vec<_>>();
        let(origin,size,bits)=certify(&phases,w,h,&words,shift);out.extend(id.to_le_bytes());for v in origin {out.extend(v.to_le_bytes());}for v in size {out.extend(v.to_le_bytes());}out.extend((bits.len()as u32).to_le_bytes());for v in bits{out.extend(v.to_le_bytes());}
    }
    assert_eq!(r.at,r.bytes.len(),"trailing certificate input");fs::write(&a[2],out).expect("certificate output");
}
#[cfg(test)]mod tests {
    use super::*;
    fn square()->[[(i32,i32);4];1] {[[(0,0),(48,0),(0,48),(48,48)]]}
    #[test]fn transparent_and_soft_never_certify(){for word in[0,0x8000,0xffff]{assert!(certify(&square(),2,2,&[word;4],3).2.is_empty());}}
    #[test]fn opaque_zero_rgb_sentinel_certifies(){let(o,s,b)=certify(&square(),2,2,&[1;4],3);assert_eq!(o,[0,0]);assert_eq!(s,[4,4]);assert_eq!(b,[0xffff]);}
    #[test]fn fine_grid_certifies_the_full_nineteen_pixel_window(){let(o,s,b)=certify(&square(),2,2,&[1;4],2);assert_eq!(o,[0,0]);assert_eq!(s,[8,8]);assert_eq!(b,[u32::MAX;2]);}
    #[test]fn intersection_rejects_missing_geometry(){let p=[square()[0],[(0,0),(16,0),(0,16),(16,16)]];assert!(certify(&p,2,2,&[1;4],3).2.is_empty());}
}
