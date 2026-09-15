//! Bounded row-run visibility scissors over a conservative16px owner grid.
//! This is a child of scenery_occlusion, using its initialized-prefix Pieces.
use super::{Pieces,MAX_PIECES};
pub const WIDTH:usize=20;
pub const HEIGHT:usize=15;
pub const CELLS:usize=WIDTH*HEIGHT;

/// Keep every source pixel whose tile has owner<=rank. Caller supplies disjoint,
/// half-open screen clips and a grid whose larger ranks prove later opacity.
/// Only clip rectangles change; the original source quad/UVs remain untouched.
/// Returns false with empty output if more than8 row-run rectangles are needed.
/// Input is never mutated, so a false return always permits exact fallback.
/// Empty visibility is successful with zero output rectangles.
#[cfg(test)]
#[inline(never)]
pub fn visible_runs(input:&[[i16;4]],owners:&[u16;CELLS],rank:u16,out:&mut Pieces)->bool {
    debug_assert!(input.len()<=MAX_PIECES);
    out.clear();
    for &[left,top,right,bottom] in input {
        debug_assert!(0<=left&&left<=right&&right<=320&&0<=top&&top<=bottom&&bottom<=240);
        if left==right||top==bottom {continue;}
        let base=out.len();let mut y=top;
        while y<bottom {
            let row=(y as usize>>4)*WIDTH;
            let next_y=(((y as i32>>4)+1)<<4).min(bottom as i32)as i16;
            let mut x=left;let mut run=right;
            while x<right {
                let hidden=owners[row+(x as usize>>4)]>rank;
                let next_x=(((x as i32>>4)+1)<<4).min(right as i32)as i16;
                if hidden {
                    if run<x && !append_run(out,base,run,x,y,next_y) {out.clear();return false;}
                    run=right;
                }else if run==right {run=x;}
                x=next_x;
            }
            if run<right && !append_run(out,base,run,right,y,next_y) {out.clear();return false;}
            y=next_y;
        }
    }
    true
}
/// Rows contain exactly the screen tiles hidden by strictly later opaque draws.
#[inline(never)]
pub fn visible_runs_rows(input:&[[i16;4]],rows:&[u32;HEIGHT],out:&mut Pieces)->bool {
    debug_assert!(input.len()<=MAX_PIECES);
    out.clear();
    for &[left,top,right,bottom] in input {
        debug_assert!(0<=left&&left<=right&&right<=320&&0<=top&&top<=bottom&&bottom<=240);
        if left==right||top==bottom {continue;}
        let base=out.len();let first=left as usize>>4;let end=(right as usize+15)>>4;
        let mask=((1u32<<end)-1)^((1u32<<first)-1);let mut y=top;
        while y<bottom {
            let row=y as usize>>4;let bits=rows[row]&mask;
            let mut next_row=row+1;
            // Equal relevant masks have exactly the same clipped horizontal
            // runs. Emit/extend them once for their whole consecutive height.
            while (next_row<<4)<bottom as usize && rows[next_row]&mask==bits {next_row+=1;}
            let next_y=((next_row<<4).min(bottom as usize))as i16;
            if bits==0 {
                if !append_run(out,base,left,right,y,next_y) {out.clear();return false;}
            } else if bits!=mask {
                let mut visible=(!bits&mask)>>first;let mut tile=first;
                while visible!=0 {
                    while visible&1==0 {visible>>=1;tile+=1;}
                    let start=tile;
                    while visible&1!=0 {visible>>=1;tile+=1;}
                    let l=((start<<4)as i16).max(left);let r=((tile<<4)as i16).min(right);
                    if !append_run(out,base,l,r,y,next_y) {out.clear();return false;}
                }
            }
            y=next_y;
        }
    }
    true
}


#[inline]
fn append_run(out:&mut Pieces,base:usize,left:i16,right:i16,top:i16,bottom:i16)->bool {
    // Every existing rectangle is final except a matching run in the preceding
    // row. No merges cross an input clip, preserving its exact boundary.
    for i in base..out.len() {
        let r=unsafe{out.rects[i].assume_init_mut()};
        if r[0]==left&&r[2]==right&&r[3]==top {r[3]=bottom;return true;}
    }
    if out.len()==MAX_PIECES {return false;}
    out.push([left,top,right,bottom]);true
}
