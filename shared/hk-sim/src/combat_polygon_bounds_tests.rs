use super::{cross,polygon_hits_box,segments_intersect};
// Frozen previous algorithm. Differential checks keep its ordering and boundary
// semantics, including unusual reversed boxes and self-intersecting polygons.
fn previous(poly:&[[i32;2]],bounds:[i32;4])->bool {
    if !(3..=16).contains(&poly.len()) {return false;}
    let [x0,y0,x1,y1]=bounds;
    if poly.iter().any(|p|(x0..=x1).contains(&p[0])&&(y0..=y1).contains(&p[1])) {return true;}
    let corners=[[x0,y0],[x1,y0],[x1,y1],[x0,y1]];let mut inside=false;let q=corners[0];
    for i in 0..poly.len() {
        let a=poly[i];let b=poly[(i+1)%poly.len()];
        for j in 0..4 {if segments_intersect(a,b,corners[j],corners[(j+1)%4]) {return true;}}
        if (a[1]>q[1])!=(b[1]>q[1]) {
            let side=cross(a,b,q);
            if (b[1]>a[1]&&side>0)||(b[1]<a[1]&&side<0) {inside=!inside;}
        }
    }
    inside
}
fn check(poly:&[[i32;2]],bounds:[i32;4]) {assert_eq!(polygon_hits_box(poly,bounds),previous(poly,bounds),"{poly:?} / {bounds:?}");}
#[test]
fn exhaustive_small_triangles_and_boxes_preserve_touching_and_reversed_bounds() {
    let points:[[i32;2];9]=core::array::from_fn(|i|[(i%3)as i32-1,(i/3)as i32-1]);
    for &a in &points {for &b in &points {for &c in &points {
        for &d in &points {for &e in &points {check(&[a,b,c],[d[0],d[1],e[0],e[1]]);}}
    }}}
}
#[test]
fn random_full_room_range_includes_concavity_self_intersections_and_far_triggers() {
    let mut seed=0x9df1728bu32;
    let mut next=||{seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;seed};
    let mut hit=0;let mut miss=0;
    for i in 0..100000 {
        let len=3+(next()%14)as usize;
        let ox=(next()%(1024*65536+1))as i32-512*65536;
        let oy=(next()%(1024*65536+1))as i32-512*65536;
        let mut points=[[0;2];16];
        for p in &mut points[..len] {p[0]=ox+(next()%(32*65536+1))as i32-16*65536;p[1]=oy+(next()%(32*65536+1))as i32-16*65536;}
        if i%3==0 {points[len-1]=points[0];}
        let near=i%2==0;let bx=if near {ox}else{-ox};let by=if near {oy}else{-oy};
        let dx=(next()%(32*65536+1))as i32;let dy=(next()%(32*65536+1))as i32;
        let bounds=[bx,by,bx+if i%5==0{-dx}else{dx},by+if i%7==0{-dy}else{dy}];
        check(&points[..len],bounds);
        if previous(&points[..len],bounds) {hit+=1;}else{miss+=1;}
    }
    assert!(hit>10000&&miss>10000);
}
#[test]
fn translated_i32_extremes_and_one_bit_gaps_keep_exact_behavior() {
    let unit=65536;let poly=[[0,0],[4*unit,0],[4*unit,unit],[unit,unit],[unit,4*unit],[0,4*unit]];
    for offset in [i32::MIN,0,i32::MAX-8*unit] {
        let points=poly.map(|p|[p[0]+offset,p[1]+offset]);
        for b in [[0,0,0,0],[unit,unit,3*unit,3*unit],[4*unit,unit,5*unit,2*unit],[4*unit+1,unit,5*unit,2*unit],[0,4*unit,unit,4*unit+1],[0,4*unit+1,unit,5*unit],[5*unit,5*unit,unit,unit]] {
            check(&points,b.map(|v|v+offset));
        }
    }
    for len in [0,1,2,17] {check(&[[0,0];17][..len],[0,0,1,1]);}
}
