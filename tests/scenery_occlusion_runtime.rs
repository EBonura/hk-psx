#[path="../game/src/scenery_occlusion.rs"]mod occlusion;
#[path="../game/src/alpha_scissor_cache.rs"]mod alpha_scissor_cache;
#[path="../game/src/occlusion_scratch.rs"]mod occlusion_scratch;
#[path="../game/src/scenery_bounds.rs"]mod scenery_bounds;
#[path="tile_runs_runtime.rs"]mod tile_runs_runtime;
use occlusion::{Pieces,subtract_many};
fn inside(r:[i16;4],x:i16,y:i16)->bool {x>=r[0]&&x<r[2]&&y>=r[1]&&y<r[3]}
#[test]fn overlapping_holes_do_not_double_count_or_double_draw() {
 let mut source=Pieces::from_slice(&[[0,0,320,240]]);
 let holes=[[0,0,100,240],[50,0,180,240],[180,100,320,240]];
 let saved=subtract_many(&mut source,&holes);let out=&source;assert_eq!(saved,62800);
 for y in 0..240 {for x in 0..320 {let copies=out.as_slice().iter().filter(|r|inside(**r,x,y)).count();assert_eq!(copies,usize::from(!holes.iter().any(|r|inside(*r,x,y))));}}
}
#[test]fn complexity_fallback_is_conservative_and_disjoint() {
 let mut seed=192u32;
 for _ in 0..500 {
  let mut holes=[[0i16;4];8];for r in &mut holes {for v in r.iter_mut(){seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);*v=((seed>>16)%128)as i16;}if r[0]>r[2]{r.swap(0,2);}if r[1]>r[3]{r.swap(1,3);}}
  let mut source=Pieces::from_slice(&[[0,0,128,128]]);let saved=subtract_many(&mut source,&holes);let out=&source;assert!(out.len()<=occlusion::MAX_SUBTRACT_PIECES);
  let mut missing=0;for y in 0..128 {for x in 0..128 {let copies=out.as_slice().iter().filter(|r|inside(**r,x,y)).count();assert!(copies<=1);if copies==0 {assert!(holes.iter().any(|r|inside(*r,x,y)));missing+=1;}}}assert_eq!(saved,missing);
 }
}
#[test]fn failed_large_hole_does_not_block_smaller_legal_saving() {
 let mut source=Pieces::from_slice(&[[0,0,100,100],[150,0,250,100]]);
 let saved=subtract_many(&mut source,&[[25,25,225,75],[0,0,25,100]]);let out=&source;assert!(saved>=2500);assert!(out.len()<=occlusion::MAX_SUBTRACT_PIECES);
}
#[test]fn overflowing_preflight_leaves_original_partition_untouched() {
 let original:Vec<_>=(0..occlusion::MAX_SUBTRACT_PIECES).map(|i|[i as i16*120,0,i as i16*120+100,100]).collect();
 let mut p=Pieces::from_slice(&original);
 assert!(!occlusion::subtract(&mut p,&[25,25,occlusion::MAX_SUBTRACT_PIECES as i16*120-25,75]));
 assert_eq!(p.as_slice(),original.as_slice());
}
#[test]fn covered_entries_retire_before_later_splits_need_capacity() {
 let mut p=Pieces::empty();
 for x in 0..7 {p.push([x*10,0,x*10+5,5]);}
 p.push([90,90,110,110]);
 assert!(occlusion::subtract(&mut p,&[0,0,100,100]));
 assert_eq!(p.len(),2);
 for y in 90..110 {for x in 90..110 {let n=p.as_slice().iter().filter(|r|inside(**r,x,y)).count();assert_eq!(n,usize::from(x>=100||y>=100));}}
}

fn compare_filtered(bounds:[i16;4],source:&[[i16;4]],holes:&[[i16;4]]) {
 let mut expected=Pieces::from_slice(source);let mut actual=Pieces::from_slice(source);
 let saved=subtract_many(&mut expected,holes);
 let mut union=[i16::MAX,i16::MAX,i16::MIN,i16::MIN];
 for h in holes {union=[union[0].min(h[0]),union[1].min(h[1]),union[2].max(h[2]),union[3].max(h[3])];}
 let mut filtered=Pieces::empty();
 if !holes.is_empty() && occlusion::can_save_minimum(&bounds,&union) {
  for h in holes {if occlusion::can_save_minimum(&bounds,h) {filtered.push(*h);}}
 }
 assert_eq!(subtract_many(&mut actual,filtered.as_slice()),saved);
 // Packet count, exact partition ordering, ranking ties and capacity fallback
 // must all remain unchanged, beyond merely producing the same occupied area.
 assert_eq!(actual.as_slice(),expected.as_slice());
}

#[test]fn bounds_filter_preserves_threshold_and_rank_ties() {
 let bbox=[0,0,320,240];
 assert!(!occlusion::can_save_minimum(&bbox,&[0,0,31,33])); //1023
 assert!(occlusion::can_save_minimum(&bbox,&[0,0,32,32])); //1024
 assert!(occlusion::can_save_minimum(&bbox,&[0,0,41,25])); //1025
 let holes=[[0,0,31,33],[0,0,32,32],[64,0,96,32],[0,0,41,25],[-50,0,-1,240],[320,0,400,240],[10,10,11,11]];
 compare_filtered(bbox,&[bbox],&holes);
 compare_filtered(bbox,&[],&holes);
 compare_filtered(bbox,&[bbox],&[]);
 compare_filtered(bbox,&[bbox],&[[-50,0,-1,240],[320,0,400,240]]);
}

#[test]fn bounds_filter_preserves_actual_alpha_scissors_and_flat_core_partitions() {
 // Four disjoint authored texel covers, mapped by the actual renderer helper.
 let cover=[4,0,0,0,0,0,15,63,24,0,15,63,48,0,15,23,48,40,15,23];
 let mut seed=98172u32;
 let mut random=||{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed>>16};
 let mut mapped=0;
 for iteration in 0..4000 {
  let l=(random()%240)as i16-80;let t=(random()%160)as i16-40;
  let r=l+100+(random()%220)as i16;let b=t+100+(random()%180)as i16;
  let (x0,x1)=if iteration&1==0 {(l,r)}else{(r,l)};
  let (y0,y1)=if iteration&2==0 {(t,b)}else{(b,t)};
  let xy=[(x0,y0),(x1,y0),(x0,y1),(x1,y1)];
  let bbox=[l.max(0),t.max(0),r.min(320),b.min(240)];
  let Some(scissors)=alpha_scissor_cache::Entry::EMPTY.map(xy,0,64,64,&cover)else{continue;};
  mapped+=1;
  let mut source=Pieces::from_slice(&scissors.rects[..scissors.count]);
  if iteration&4!=0 {
   if let Some(core)=alpha_scissor_cache::texel_rect_to_screen(xy,64,64,[0,0,16,64]) {occlusion::subtract(&mut source,&core);}
  }
  let mut holes=[[0i16;4];8];
  for h in &mut holes {
   let x=(random()%480)as i16-80;let y=(random()%360)as i16-60;
   *h=[x,y,x+1+(random()%160)as i16,y+1+(random()%120)as i16];
  }
  compare_filtered(bbox,source.as_slice(),&holes);
 }
 assert!(mapped>3000);
}

#[test]fn scratch_phase_overlay_initializes_poisoned_memory_and_preserves_rows() {
 use core::mem::{align_of,size_of,MaybeUninit};
 use occlusion_scratch::{Storage,Occluder,Occluders};
 #[repr(C)]struct Guarded {before:[usize;4],storage:MaybeUninit<Storage>,after:[usize;4]}
 let mut guarded=Guarded{before:[0xabc;4],storage:MaybeUninit::uninit(),after:[0xdef;4]};
 let base=guarded.storage.as_mut_ptr();
 assert_eq!(base as usize%align_of::<Storage>(),0);
 unsafe {
  // Deliberately invalid bool/count bytes: initialization must touch only
  // counters, and later readers may expose only explicitly written entries.
  base.cast::<u8>().write_bytes(0xa5,size_of::<Storage>());
  Storage::initialize_at(base);
  let fields=[
   (Storage::hidden(base)as usize,size_of::<Pieces>()),
   (Storage::scenery(base)as usize,size_of::<Pieces>()),
   (Storage::holes(base)as usize,size_of::<Pieces>()),
   (Storage::occluders(base)as usize,size_of::<Occluders>()),
   (Storage::union(base)as usize,size_of::<[i16;4]>()),
   (Storage::tile_rows(base)as usize,size_of::<[u32;15]>()),
  ];
  for (i,(start,len)) in fields.iter().enumerate() {
   assert!(*start>=base as usize && start+len<=base as usize+size_of::<Storage>());
   for (other,width) in &fields[i+1..] {assert!(start+len<=*other || other+width<=*start);}
  }
  {
   let hidden=&mut *Storage::hidden(base);let scenery=&mut *Storage::scenery(base);
   let holes=&mut *Storage::holes(base);let occluders=&mut *Storage::occluders(base);
   let rows=&mut *Storage::tile_rows(base);assert_eq!(*rows,[0;15]);rows[14]=1<<19;
   assert_eq!((hidden.len(),scenery.len(),holes.len(),occluders.len()),(0,0,0,0));
   hidden.push([0,0,320,240]);scenery.push([1,2,3,4]);holes.push([5,6,7,8]);
   for i in 0..8 {occluders.set(i,Occluder{draw:i as u16,rect:[i as i16;4],front:i%2==0});}
   occluders.set(3,Occluder{draw:19,rect:[10,20,30,40],front:true});
   assert_eq!(occluders.len(),8);assert_eq!(occluders.as_slice()[3].draw,19);
   assert!(occlusion::subtract(hidden,&[0,0,160,240]));
   assert_eq!(scenery.as_slice(),&[[1,2,3,4]]);assert_eq!(holes.as_slice(),&[[5,6,7,8]]);
   assert_eq!(hidden.as_slice(),&[[160,0,320,240]]);
  }
  // Switching phases must end every draw-field reference first. Exercise the
  // whole owner array, then invalidate it without exposing its stale bool bytes.
  Storage::initialize_owners_at(base);
  {
   let owners=&mut *Storage::tile_owners(base);
   let rows=&mut *Storage::tile_rows(base);
   assert_eq!(owners.as_ptr()as usize%align_of::<u16>(),0);
   assert_eq!(owners,&[0u16;300]);
   for (i,owner)in owners.iter_mut().enumerate() {*owner=(i+1)as u16;}
   assert_eq!(rows[14],1<<19);
  }
  Storage::initialize_draw_at(base);
  {
   assert_eq!((*Storage::hidden(base)).len(),0);
   assert_eq!((*Storage::occluders(base)).len(),0);
   let scenery=&mut *Storage::scenery(base);let holes=&mut *Storage::holes(base);
   scenery.push([0,0,320,32]);
   let rows=&mut *Storage::tile_rows(base);rows.fill(0);rows[0]=0xaaaaa;
   assert!(occlusion::tile_runs::visible_runs_rows(scenery.as_slice(),rows,holes));
   assert_eq!(holes.len(),11);assert_eq!(scenery.as_slice(),&[[0,0,320,32]]);
   assert_eq!(rows[0],0xaaaaa);

  }
  // Frame/scene reset invalidates every old prefix without changing or reading
  // the stale backing data. Initialize only after previous borrows have ended.
  Storage::initialize_at(base);
  assert_eq!((*Storage::hidden(base)).len(),0);
  assert_eq!((*Storage::scenery(base)).len(),0);
  assert_eq!((*Storage::holes(base)).len(),0);
  assert_eq!((*Storage::occluders(base)).len(),0);
  assert_eq!(*Storage::union(base),[i16::MAX,i16::MAX,i16::MIN,i16::MIN]);
  assert_eq!(*Storage::tile_rows(base),[0u32;15]);
 }
 assert_eq!(guarded.before,[0xabc;4]);assert_eq!(guarded.after,[0xdef;4]);
}

fn check_shared_bounds(v:[(i32,i32);4])->bool {
 let culled=v.iter().all(|p|p.0<0)||v.iter().all(|p|p.0>320)
  ||v.iter().all(|p|p.1<0)||v.iter().all(|p|p.1>240);
 if culled {return false;}
 let actual=scenery_bounds::clipped(&v);
 let l=v.iter().map(|p|p.0).min().unwrap().max(0);let r=v.iter().map(|p|p.0).max().unwrap().min(320);
 let t=v.iter().map(|p|p.1).min().unwrap().max(0);let b=v.iter().map(|p|p.1).max().unwrap().min(240);
 assert_eq!(actual,[l,t,r,b]);
 assert!((0..=320).contains(&l)&&(0..=320).contains(&r));
 assert!((0..=240).contains(&t)&&(0..=240).contains(&b));
 // Both existing legal paths prove i16 representability before packet and
 // partial-occlusion construction; compare their former independent bbox.
 if v.iter().all(|&(x,y)|i16::try_from(x).is_ok()&&i16::try_from(y).is_ok()) {
  let xy=v.map(|(x,y)|(x as i16,y as i16));
  let old=[xy.iter().map(|p|p.0).min().unwrap().max(0),xy.iter().map(|p|p.1).min().unwrap().max(0),
   xy.iter().map(|p|p.0).max().unwrap().min(320),xy.iter().map(|p|p.1).max().unwrap().min(240)];
  assert_eq!(actual,old.map(i32::from));
 }
 true
}

#[test]fn shared_bounds_preserve_edges_degenerates_and_all_corner_orders() {
 for l in [-1024,-1,0,1,319,320,321,1023] {
  for r in [-1024,-1,0,1,319,320,321,1023] {
   for t in [-1024,-1,0,1,239,240,241,1023] {
    for b in [-1024,-1,0,1,239,240,241,1023] {
     let v=[(l,t),(r,t),(l,b),(r,b)];
     for a in 0..4 {for c in 0..4 {if c==a{continue;}for d in 0..4 {if d==a||d==c{continue;}
      let e=6-a-c-d;check_shared_bounds([v[a],v[c],v[d],v[e]]);
     }}}
    }
   }
  }
 }
 // These strict-cull survivors must remain accepted even though they paint
 // no area. Wholly outside quads remain rejected before clipping is called.
 for v in [[(0,0);4],[(320,240);4],[(0,0),(0,240),(0,0),(0,240)]] {assert!(check_shared_bounds(v));}
 for v in [[(-1,10);4],[(321,10);4],[(10,-1);4],[(10,241);4]] {assert!(!check_shared_bounds(v));}
}

#[test]fn shared_bounds_match_large_repair_and_arbitrary_rotated_coordinates() {
 assert!(check_shared_bounds([(i32::MIN,i32::MAX),(i32::MAX,i32::MIN),(0,0),(320,240)]));
 let mut seed=139u32;
 for _ in 0..20000 {
  let mut v=[(0,0);4];
  for p in &mut v {
   seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);p.0=(seed%2097153)as i32-1048576;
   seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);p.1=(seed%2097153)as i32-1048576;
  }
  check_shared_bounds(v);
 }
}

#[test]
fn in_place_alpha_partition_survives_core_and_capacity_fallback() {
 // Original alpha output is local and independent of both mutable scratch
 // partitions, so exhausting the optional packet allowance can still emit it.
 let cover=[4,0,0,0,0,0,15,63,24,0,15,63,48,0,15,23,48,40,15,23];
 let mut old=alpha_scissor_cache::Entry::EMPTY;
 let mut new=alpha_scissor_cache::Entry::EMPTY;
 let mut secondary=alpha_scissor_cache::Secondary::EMPTY;
 for frame in 0..128 {
  let l=(frame*3%180)as i16-50;let t=(frame*7%140)as i16-35;
  let r=l+240;let b=t+200;
  let (x0,x1)=if frame&1==0 {(l,r)}else{(r,l)};
  let (y0,y1)=if frame&2==0 {(t,b)}else{(b,t)};
  let xy=[(x0,y0),(x1,y0),(x0,y1),(x1,y1)];
  let expected=old.map(xy,0,64,64,&cover).unwrap();
  let mut storage=core::mem::MaybeUninit::uninit();
  let alpha=alpha_scissor_cache::MappedScissors::initialize(&mut storage);
  assert!(new.map_cached_into(xy,0,64,64,&cover,&mut secondary,alpha));
  let mut pieces=Pieces::from_slice(alpha.as_slice());
  let holes=[[0,0,100,240],[180,40,320,160]];
  if let Some(core)=alpha_scissor_cache::texel_rect_to_screen(xy,64,64,[0,0,16,64]) {occlusion::subtract(&mut pieces,&core);}
  subtract_many(&mut pieces,&holes);
  // Forced rejection of this optimized partition must still have the exact
  // original alpha fallback available; no alias may mutate its stored prefix.
  assert_eq!(alpha.as_slice(),&expected.rects[..expected.count]);
  assert_eq!(alpha.saved_pixels(),expected.saved_pixels);
 }
}
