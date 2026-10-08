#[path="../game/src/scenery_bounds.rs"] mod candidate;
fn projection(xy:&[i32;8],cx:i32,cy:i32)->[(i32,i32);4] {
    core::array::from_fn(|k|(160+((xy[k*2]-cx)>>8),120-((xy[k*2+1]-cy)>>8)))
}
fn reference(xy:&[i32;8],cx:i32,cy:i32)->Option<[i32;4]> {
    let v=projection(xy,cx,cy);
    if v.iter().all(|p|p.0<0)||v.iter().all(|p|p.0>320)||v.iter().all(|p|p.1<0)||v.iter().all(|p|p.1>240) {None}
    else {Some(candidate::clipped(&v))}
}
fn check(xy:[i32;8],cx:i32,cy:i32) {
    for k in 0..8 { assert!(xy[k].checked_sub(if k%2==0{cx}else{cy}).is_some()); }
    let expected=reference(&xy,cx,cy);
    let bits=candidate::local_indices(&xy);
    let expected_local=[xy[0].min(xy[2]).min(xy[4]).min(xy[6]),xy[1].min(xy[3]).min(xy[5]).min(xy[7]),xy[0].max(xy[2]).max(xy[4]).max(xy[6]),xy[1].max(xy[3]).max(xy[5]).max(xy[7])];
    assert_eq!(candidate::selected(&xy,bits),expected_local);
    let actual=candidate::project_clipped(&xy,bits,cx,cy);
    assert_eq!(actual,expected,"xy={xy:?},offset={cx},{cy}");
    if let Some(bb)=actual {
        // Surviving legal and repair paths receive the original four vertices.
        let v=projection(&xy,cx,cy);
        let independent=core::array::from_fn::<_,4,_>(|k|(
            (160+((xy[2*k] as i64-cx as i64)>>8)) as i32,
            (120-((xy[2*k+1] as i64-cy as i64)>>8)) as i32));
        assert_eq!(v,independent);
        assert_eq!(bb,candidate::clipped(&independent));
        assert!(bb.iter().all(|&x|i16::try_from(x).is_ok()));
    }
}
fn rand(s:&mut u64)->u32 {*s=s.wrapping_mul(6364136223846793005).wrapping_add(1);(*s>>32)as u32}
#[test] fn boundaries_all_extrema_corner_positions_and_degenerate_quads() {
    let vals=[i32::MIN,i32::MIN+1,-2000000000,-81921,-81920,-40961,-40960,-40959,-30721,-30720,-30719,-257,-256,-255,-1,0,1,255,256,257,30719,30720,30721,40959,40960,40961,81920,2000000000,i32::MAX-1,i32::MAX];
    for &lo in &vals {for &hi in &vals {for axis in 0..2 {for corner in 0..4 {
        let mut xy=[0;8]; for k in 0..4 {xy[2*k+axis]=lo;}xy[2*corner+axis]=hi;
        check(xy,0,0);
    }}}}
    for edge in [-1,0,1,239,240,241,319,320,321] {for residue in 0..256 {
        let x=(edge-160)*256+residue; let y=(120-edge)*256+residue;
        check([x,0,x,0,x,0,x,0],0,0);check([0,y,0,y,0,y,0,y],0,0);
    }}
}
#[test] fn two_million_valid_full_i32_coordinates_and_camera_offsets() {
    let mut s=0x387954687923u64;
    for _ in 0..2_000_000 {
        let cx=rand(&mut s)as i32;let cy=rand(&mut s)as i32;
        let mut xy=[0;8];
        for k in 0..8 {
            let offset=if k%2==0{cx}else{cy}as i64;
            let low=(i32::MIN as i64+offset).max(i32::MIN as i64);
            let high=(i32::MAX as i64+offset).min(i32::MAX as i64);
            xy[k]=(low+(rand(&mut s)as i64)%(high-low+1))as i32;
        }
        check(xy,cx,cy);
    }
}
#[test] fn translated_reflected_rotated_and_repair_shapes() {
    let mut s=0x4439u64;
    for _ in 0..200_000 {
        let scale=rand(&mut s)as i32;
        let camera=(rand(&mut s)as i32,rand(&mut s)as i32);
        let cx=(((camera.0>>8)as i64*scale as i64)>>12)as i32;
        let cy=(((camera.1>>8)as i64*scale as i64)>>12)as i32;
        let mut xy=[0;8];let mut valid=true;
        for k in 0..8 {
            let delta=(rand(&mut s)%536870912)as i64-268435456;
            let coord=delta+if k%2==0{cx}else{cy}as i64;
            if let Ok(n)=i32::try_from(coord){xy[k]=n;}else{valid=false;}
        }
        if valid {check(xy,cx,cy);}
    }
}

#[test] fn every_packed_value_is_in_bounds_and_every_extrema_permutation_matches() {
    let xy=[i32::MIN,i32::MAX,0,-1,1,0,i32::MAX,i32::MIN];
    for packed in 0..=255 {let _=candidate::selected(&xy,packed);}
    for a in 0..4 {for b in 0..4 {for c in 0..4 {for d in 0..4 {
        let mut xy=[0;8];xy[2*a]=-1000;xy[2*b+1]=-1200;xy[2*c]=2000;xy[2*d+1]=2200;
        check(xy,0,0);
    }}}}
}
