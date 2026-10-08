//! Shared screen-clipped bounds for whole-draw and partial occlusion.
/// Call after rejecting quads wholly x<0/x>320/y<0/y>240. That strict cull
/// retains viewport-edge and zero-area quads, and proves these clipped values
/// fit i16 even when the original coordinates require geometry repair. Keep
/// them in native i32 registers until a packet needs packed coordinates.
#[inline]
pub fn clipped(xy:&[(i32,i32);4])->[i32;4] {
    let l=xy.iter().map(|p|p.0).min().unwrap().max(0);
    let r=xy.iter().map(|p|p.0).max().unwrap().min(320);
    let t=xy.iter().map(|p|p.1).min().unwrap().max(0);
    let b=xy.iter().map(|p|p.1).max().unwrap().min(240);
    [l,t,r,b]
}

/// Four two-bit corner IDs in min-X, min-Y, max-X, max-Y order.
#[inline]
pub fn local_indices(xy:&[i32;8])->u8 {
    let (mut lx,mut ty,mut rx,mut by)=(0usize,0usize,0usize,0usize);
    for corner in 1..4 {
        if xy[corner*2]<xy[lx*2] {lx=corner;}
        if xy[corner*2+1]<xy[ty*2+1] {ty=corner;}
        if xy[corner*2]>xy[rx*2] {rx=corner;}
        if xy[corner*2+1]>xy[by*2+1] {by=corner;}
    }
    (lx|(ty<<2)|(rx<<4)|(by<<6))as u8
}
#[inline]
pub fn selected(xy:&[i32;8],indices:u8)->[i32;4] {
    [xy[(indices&3)as usize*2],xy[((indices>>2)&3)as usize*2+1],
     xy[((indices>>4)&3)as usize*2],xy[(indices>>6)as usize*2+1]]
}
/// The cooker proves every XY minus its camera offset fits i32. Selecting
/// extrema before monotone translation/shift preserves the original strict
/// viewport cull, including edge-touching and degenerate quads.
#[inline]
pub fn project_clipped(xy:&[i32;8],indices:u8,cx:i32,cy:i32)->Option<[i32;4]> {
    let bounds=selected(xy,indices);
    let l=160+((bounds[0]-cx)>>8);
    let r=160+((bounds[2]-cx)>>8);
    let t=120-((bounds[3]-cy)>>8);
    let b=120-((bounds[1]-cy)>>8);
    if r<0 || l>320 || b<0 || t>240 {None}
    else {Some([l.max(0),t.max(0),r.min(320),b.min(240)])}
}
/// The same projected bounds without the viewport cull or clamp.
#[inline]
pub fn project_unclipped(xy:&[i32;8],indices:u8,cx:i32,cy:i32)->[i32;4] {
    let bounds=selected(xy,indices);
    [160+((bounds[0]-cx)>>8),120-((bounds[3]-cy)>>8),160+((bounds[2]-cx)>>8),120-((bounds[1]-cy)>>8)]
}
