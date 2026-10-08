//! Host-certified opaque 16px tiles from immutable source geometry.
//!
//! A bit certifies a(16+step-1) square across every fractional projection
//! phase. Flooring a16px screen tile's local origin to the cooked grid adds less
//! than one grid step,
//! so the entire tile remains inside that square. Only later source ranks
//! may suppress a draw. No source primitive or texture sampling is changed.
#[cfg(not(test))]
pub use hk_format::coverage::CoverageCert as TileCert;
#[cfg(test)]
#[derive(Clone,Copy)]
#[repr(C)]
pub struct TileCert {pub gx:i16,pub gy:i16,pub width:u16,pub height:u16,pub offset:u32}
#[cfg(not(test))]const TILE_CERT_SHIFT:u32=crate::disc::COVERAGE_GRID_SHIFT;
#[cfg(test)]const TILE_CERT_SHIFT:u32=3;
pub const NO_CERT:u16=u16::MAX;
#[inline]
pub fn rank(draw:usize,front:bool)->u16 {(draw+1+if front{1024}else{0})as u16}

/// Reject certificates with no screen tile origin inside their cropped bounds.
/// Uses only vertex0; callers can defer the remaining projection/legal checks.
#[inline]
pub fn intersects(certs:&[TileCert],cert_id:u16,origin:(i32,i32))->bool {
    intersects_mask::<TILE_CERT_SHIFT>(&certs[cert_id as usize],origin)
}
#[inline(always)]
fn intersects_mask<const SHIFT:u32>(c:&TileCert,origin:(i32,i32))->bool {
    let step=1<<SHIFT;
    let left=((origin.0+c.gx as i32*step+15)>>4).max(0);
    let top=((origin.1+c.gy as i32*step+15)>>4).max(0);
    let right=((origin.0+(c.gx as i32+c.width as i32)*step+15)>>4).min(20);
    let bottom=((origin.1+(c.gy as i32+c.height as i32)*step+15)>>4).min(15);
    left<right && top<bottom
}

/// Caller visits source ranks in decreasing order and supplies a zeroed grid.
/// Actual primitive coordinates must pass the signed11bit and extent guards.
/// Returns (newly owned tiles, certificate bit lookups), bounded by300 each.
#[inline(never)]
pub fn claim(certs:&[TileCert],bits:&[u32],cert_id:u16,origin:(i32,i32),owner:u16,grid:&mut[u16;300])->(u32,u32) {
    claim_mask::<TILE_CERT_SHIFT>(&certs[cert_id as usize],bits,origin,owner,grid)
}
#[inline(always)]
fn claim_mask<const SHIFT:u32>(c:&TileCert,bits:&[u32],origin:(i32,i32),owner:u16,grid:&mut[u16;300])->(u32,u32) {
    let step:i32=1<<SHIFT;let bit_step:u32=1<<(4-SHIFT);
    let left=((origin.0+c.gx as i32*step+15)>>4).max(0);
    let top=((origin.1+c.gy as i32*step+15)>>4).max(0);
    let right=((origin.0+(c.gx as i32+c.width as i32)*step+15)>>4).min(20);
    let bottom=((origin.1+(c.gy as i32+c.height as i32)*step+15)>>4).min(15);
    if left>=right || top>=bottom {return(0,0);}
    let x=((left*16-origin.0)>>SHIFT)-c.gx as i32;
    let y=((top*16-origin.1)>>SHIFT)-c.gy as i32;
    let mut row_bit=c.offset+y as u32*c.width as u32+x as u32;
    let(mut added,mut lookups)=(0,0);
    for row in top..bottom {
        let mut bit=row_bit;
        for column in left..right {
            let slot=&mut grid[row as usize*20+column as usize];
            if *slot==0 {
                lookups+=1;
                if bits[bit as usize>>5]&(1u32<<(bit&31))!=0 {*slot=owner;added+=1;}
            }
            bit+=bit_step;
        }
        row_bit+=c.width as u32*bit_step;
    }
    (added,lookups)
}

#[cfg(test)]mod tests {
    use super::*;
    fn phases<const SHIFT:u32>() {
        let c=TileCert{gx:-9,gy:-5,width:13,height:9,offset:32};
        let mut bits=[0u32;5];
        for y in 0..9 {for x in 0..13 {if (x*7+y*11)%5!=0 {let bit=32+y*13+x;bits[bit>>5]|=1<<(bit&31);}}}
        for oy in (-400..400).step_by(17) {for ox in (-700..700).step_by(13) {
            let mut grid=core::array::from_fn(|i|if i%11==0{4000}else{0});let mut expected=grid;
            let(mut added,mut reads)=(0,0);
            for row in 0..15 {for column in 0..20 {
                let x=((column as i32*16-ox)>>SHIFT)-c.gx as i32;
                let y=((row as i32*16-oy)>>SHIFT)-c.gy as i32;
                let i=row*20+column;
                if 0<=x&&x<c.width as i32&&0<=y&&y<c.height as i32&&expected[i]==0 {
                    reads+=1;let bit=c.offset+y as u32*c.width as u32+x as u32;
                    if bits[bit as usize>>5]&(1<<(bit&31))!=0 {expected[i]=7;added+=1;}
                }
            }}
            let any=(0..15).any(|row|(0..20).any(|column|{let x=((column*16-ox)>>SHIFT)-c.gx as i32;let y=((row*16-oy)>>SHIFT)-c.gy as i32;0<=x&&x<c.width as i32&&0<=y&&y<c.height as i32}));
            assert_eq!(intersects_mask::<SHIFT>(&c,(ox,oy)),any);
            assert_eq!(claim_mask::<SHIFT>(&c,&bits,(ox,oy),7,&mut grid),(added,reads));
            assert_eq!(grid,expected,"shift{SHIFT} origin{ox},{oy}");
        }}
    }
    #[test]fn both_lattices_match_direct_floor_lookup_and_preserve_latest_owner(){phases::<2>();phases::<3>();}
    #[test]fn ranks_separate_back_actors_and_front_for_full_format_capacity(){
        assert!(rank(1023,false)<1025);assert_eq!(rank(0,true),1025);
        assert!(rank(0,true)>rank(1023,false));assert_eq!(rank(1023,true),2048);
    }
}
