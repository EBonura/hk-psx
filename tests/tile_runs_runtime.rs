use crate::occlusion::{Pieces,tile_runs::{visible_runs,CELLS}};
fn canonical_count(input:&[[i16;4]],grid:&[u16;CELLS],rank:u16)->usize {
    // Independent per-pixel oracle: only identical runs in consecutive pixel
    // rows can belong to the same output rectangle. Preserve input boundaries.
    let mut count=0;
    for &[l,t,r,b]in input {
        let mut prior=std::collections::HashSet::new();
        for y in t..b {
            let mut current=std::collections::HashSet::new();let mut x=l;
            while x<r {
                if grid[(y as usize>>4)*20+(x as usize>>4)]>rank {x+=1;continue;}
                let left=x;x+=1;
                while x<r&&grid[(y as usize>>4)*20+(x as usize>>4)]<=rank {x+=1;}
                let span=(left,x);if !prior.contains(&span) {count+=1;}current.insert(span);
            }
            prior=current;
        }
    }
    count
}
fn compare_rows(input:&[[i16;4]],grid:&[u16;CELLS],rank:u16,out:&mut Pieces)->bool {
 let mut rows=[0u32;15];for y in 0..15 {for x in 0..20 {if grid[y*20+x]>rank {rows[y]|=1<<x;}}}
 let mut old=Pieces::empty();let old_ok=visible_runs(input,grid,rank,&mut old);
 let ok=crate::occlusion::tile_runs::visible_runs_rows(input,&rows,out);
 assert_eq!(ok,old_ok);assert_eq!(out.as_slice(),old.as_slice());
 // Independent tile/clip overlap oracle, including arbitrary partial tiles.
 let expected=grid.iter().enumerate().any(|(i,&owner)| {
     let x=(i%20)as i16*16;let y=(i/20)as i16*16;
     owner>rank&&input.iter().any(|&[l,t,r,b]|l<r&&t<b&&l<x+16&&x<r&&t<y+16&&y<b)
 });
 let hidden=crate::occlusion::tile_runs::any_hidden(input,&rows);
 assert_eq!(hidden,expected);
 if !hidden {
     assert!(ok);
     let nonempty:Vec<_>=input.iter().copied().filter(|r|r[0]<r[2]&&r[1]<r[3]).collect();
     assert_eq!(out.as_slice(),nonempty.as_slice());
 }
 ok
}
fn compare(input:&[[i16;4]],grid:&[u16;CELLS],rank:u16)->bool {
    let mut out=Pieces::from_slice(&[[319,239,320,240]]);
    let original=input.to_vec();
    let ok=compare_rows(input,grid,rank,&mut out);
    assert_eq!(input,original);
    let expected_count=canonical_count(input,grid,rank);
    assert_eq!(ok,expected_count<=crate::occlusion::MAX_PIECES,"canonical piececount{expected_count},input{input:?}");
    if !ok {assert_eq!(out.len(),0);return false;}
    assert_eq!(out.len(),expected_count);
    let mut expected=vec![false;320*240];
    for &[l,t,r,b]in input {
        for y in t..b {for x in l..r {assert!(!expected[y as usize*320+x as usize]);expected[y as usize*320+x as usize]=grid[(y as usize>>4)*20+(x as usize>>4)]<=rank;}}
    }
    let mut actual=vec![false;320*240];
    for &[l,t,r,b]in out.as_slice() {
        assert!(0<=l&&l<r&&r<=320&&0<=t&&t<b&&b<=240);
        for y in t..b {for x in l..r {assert!(!actual[y as usize*320+x as usize],"output overlaps");actual[y as usize*320+x as usize]=true;}}
    }
    assert_eq!(actual,expected,"input{input:?},rank{rank}");true
}
#[test]
fn full_empty_rank_and_partial_screen_edges() {
    let mut out=Pieces::empty();
    for rect in [[0,0,320,240],[1,1,319,239],[15,15,17,17],[16,16,32,32],[319,239,320,240],[0,0,1,1]] {
        for owner in [0,1,2,65535] {for rank in [0,1,2,65535] {assert!(compare(&[rect],&[owner;CELLS],rank));}}
        assert!(visible_runs(&[rect],&[7;CELLS],7,&mut out));assert_eq!(out.as_slice(),&[rect]);
        assert!(visible_runs(&[rect],&[8;CELLS],7,&mut out));assert_eq!(out.len(),0);
    }
    assert!(visible_runs(&[],&[0;CELLS],0,&mut out));assert_eq!(out.len(),0);
    assert!(visible_runs(&[[0,1,0,9],[2,5,12,5]],&[0;CELLS],0,&mut out));assert_eq!(out.len(),0);
}
#[test]
fn exhaustive_three_by_three_masks_and_partial_tile_boundaries() {
    for mask in 0..512 {
        let mut grid=[0;CELLS];
        for y in 0..3 {for x in 0..3 {grid[y*20+x]=if mask&(1<<(y*3+x))!=0 {5}else{4};}}
        for clip in [[0,0,48,48],[1,7,47,39],[15,15,33,33],[16,16,48,48]] {compare(&[clip],&grid,4);}
    }
}
#[test]
fn identical_runs_extend_without_crossing_clips_or_gaps() {
    let mut grid=[2;CELLS];for y in 0..15 {grid[y*20+3]=1;grid[y*20+4]=1;}
    let mut out=Pieces::empty();assert!(visible_runs(&[[1,1,319,239]],&grid,1,&mut out));
    assert_eq!(out.as_slice(),&[[48,1,80,239]]);
    grid[5*20+3]=2;grid[5*20+4]=2;
    assert!(visible_runs(&[[1,1,319,239]],&grid,1,&mut out));
    assert_eq!(out.as_slice(),&[[48,1,80,80],[48,96,80,239]]);
    assert!(visible_runs(&[[48,0,80,80],[48,80,80,160]],&[0;CELLS],0,&mut out));
    assert_eq!(out.as_slice(),&[[48,0,80,80],[48,80,80,160]]);
}
#[test]
fn capacity_falls_back_without_touching_original() {
    let mut grid=[2;CELLS];
    // Four separated rows of ten one-tile runs exceed the output limit.
    for y in [0,2,4,6] {for x in (0..20).step_by(2) {grid[y*20+x]=0;}}
    let input=[[0,0,320,112]];assert!(!compare(&input,&grid,1));
    let mut out=Pieces::empty();assert!(visible_runs(&[[0,0,256,112]],&grid,1,&mut out));assert_eq!(out.len(),32);
    assert!(visible_runs(&[[0,0,16,16];0],&grid,1,&mut out));assert_eq!(out.len(),0);
}
#[test]
fn random_disjoint_clips_match_pixel_oracle_or_safe_fallback() {
    let mut seed=0x971368acu32;
    let mut next=|| {seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed};
    let mut success=0;let mut fallback=0;
    for iteration in 0..4000 {
        let mut grid=[0;CELLS];for g in &mut grid {*g=(next()%5)as u16;}
        let rank=(next()%5)as u16;let n=(next()%9)as usize;let mut input=vec![];
        for j in 0..n {
            // Separate40px strips, with arbitrary partial tile/screen edges.
            let l=j as i16*40+(next()%20)as i16;let r=l+1+(next()%((40-(l%40))as u32))as i16;
            let t=(next()%240)as i16;let b=t+1+(next()%((240-t)as u32))as i16;
            input.push([l,t,r,b]);
        }
        // Full viewport cases continue to exercise complexity fallback as the
        // bounded output capacity grows; strip cases stress clipped boundaries.
        if iteration%4==0 {input.clear();input.push([0,0,320,240]);}
        if compare(&input,&grid,rank) {success+=1;}else{fallback+=1;}
    }
    assert!(success>100&&fallback>100);
}

#[test]
fn hidden_preflight_checks_half_open_edges_and_ignores_empty_clips() {
    use crate::occlusion::tile_runs::any_hidden;
    let mut rows=[0u32;15];
    for y in 0..15 {for x in 0..20 {
        rows[y]=1<<x;let l=x as i16*16;let t=y as i16*16;
        assert!(any_hidden(&[[l,t,l+1,t+1]],&rows));
        assert!(any_hidden(&[[l+15,t+15,l+16,t+16]],&rows));
        assert!(!any_hidden(&[[l,t,l,t+16],[l,t,l+16,t]],&rows));
        if l>0 {assert!(!any_hidden(&[[0,t,l,t+16]],&rows));}
        if t>0 {assert!(!any_hidden(&[[l,0,l+16,t]],&rows));}
        if l+16<320 {assert!(!any_hidden(&[[l+16,t,320,t+16]],&rows));}
        if t+16<240 {assert!(!any_hidden(&[[l,t+16,l+16,240]],&rows));}
        rows[y]=0;
    }}
    assert!(!any_hidden(&[],&[u32::MAX;15]));
    // Bits outside the twenty screen columns never describe visible pixels.
    assert!(!any_hidden(&[[0,0,320,240]],&[0xfff0_0000;15]));
    assert!(!any_hidden(&[[320,0,320,240],[0,240,320,240]],&[u32::MAX;15]));
}
