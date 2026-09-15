// Included inside the production break-effects native test module.
// All authored fixtures are generated locally; this file contains no retail data.

mod source_tracks {use super::*;include!(env!("HK_TRACK_FIXTURE"));}
#[test]fn actual_host_tracks_match_rng_prefix_and_every_phase(){
 let mut observed=0;let mut frames=0;
 for (ei,e)in source_tracks::EMITTERS.iter().enumerate(){
  for (kind,facing)in [(0,1),(0,-1),(2,1)] {
   for limit in [128,1,1+ei%13] {
    let mut pool=Pool::new();
    spawn_specs(&mut pool,e.scene as usize,e.owner as usize,kind,facing,core::slice::from_ref(e),source_tracks::STYLES);
    let first=pool.particles[0];let mut reference=Pool::new();
    for slot in &mut reference.particles[limit..]{*slot=first;}reference.active_count=(128-limit)as u32;
    let mut fast=Pool::new();fast.particles=reference.particles;fast.active_count=reference.active_count;
    spawn_specs(&mut reference,e.scene as usize,e.owner as usize,kind,facing,core::slice::from_ref(e),source_tracks::STYLES);
    spawn_specs_with_bases(&mut fast,e.scene as usize,e.owner as usize,kind,facing,core::slice::from_ref(e),source_tracks::STYLES,&source_tracks::EMITTER_TRACK_BASES[ei..ei+1]);
    assert_eq!(format!("{:?}",fast.particles),format!("{:?}",reference.particles));
    assert_eq!((fast.spawned,fast.dropped),(reference.spawned,reference.dropped));
    for (i,p)in fast.particles[..limit.min(source_tracks::STYLES[e.style as usize].count as usize)].iter().flatten().enumerate(){
     assert_ne!(fast.tracks[i],0);let s=&source_tracks::STYLES[e.style as usize];
     for phase in 0..33 {
      let word=cached_frame(fast.tracks[i],phase,source_tracks::PARTICLE_TRACKS).unwrap();
      let sm=&s.samples[phase];let half=mulq(p.size,lerp_reference(sm.size,p.gradient))/2;
      let radius=mulq(half,s.radius_scale);let spin=lerp_reference(sm.spin,p.gradient);
      let alpha=lerp_reference(sm.alpha.map(i32::from),p.gradient);let level=((alpha*3+127)/255).clamp(0,3);
      assert_eq!((word[0]&65535,word[0]>>16,frame_spin(word),(word[1]>>26)&3),(half as u32,radius as u32,spin,level as u32));frames+=1;
     }
     observed+=1;
    }
   }
  }
 }
 assert!(observed>0);assert_eq!(frames,observed*33);println!("particles={observed} phase_frames={frames}");
}
#[test]fn actual_source_cached_trajectories_match_scalar_through_contacts_and_reuse(){
 let mut owners=std::collections::BTreeSet::new();for e in source_tracks::EMITTERS{owners.insert((e.scene as usize,e.owner as usize));}
 let mut cached_reads=0;let mut floor_contacts=0;
 for (scene,owner)in owners {
  let first=source_tracks::EMITTERS.iter().find(|e|e.scene as usize==scene&&e.owner as usize==owner).unwrap();
  let(x,y)=(first.origin[0],first.origin[1]);let coverage=[x-100*ONE,y-100*ONE,x+100*ONE,y+100*ONE];
  let edges=[[x-20*ONE,y-ONE,x+20*ONE,y-ONE],[x+3*ONE,y-4*ONE,x+3*ONE,y+10*ONE],[x-4*ONE,y-ONE,x,y+ONE]];
  let(mut fast,mut reference)=(Pool::new(),Pool::new());
  spawn_specs_with_bases(&mut fast,scene,owner,0,1,source_tracks::EMITTERS,source_tracks::STYLES,source_tracks::EMITTER_TRACK_BASES);
  spawn_specs(&mut reference,scene,owner,0,1,source_tracks::EMITTERS,source_tracks::STYLES);
  for tick in 0..650 {
   let current=if tick%53==0{scene+1}else{scene};let edge=|i|if tick>=140&&i==0{[0;4]}else{edges[i]};
   tick_specs_with_tracks(&mut fast,current,source_tracks::STYLES,coverage,edges.len(),edge,source_tracks::PARTICLE_TRACKS);
   tick_reference(&mut reference,current,source_tracks::STYLES,coverage,edges.len(),edge);
   assert_eq!(format!("{:?}",fast.particles),format!("{:?}",reference.particles),"scene{scene} owner{owner} tick{tick}");
   assert_eq!((fast.active(),fast.spawned,fast.dropped),(reference.active(),reference.spawned,reference.dropped));
   for (i,p)in fast.particles.iter().enumerate(){if let Some(p)=p {
    let s=&source_tracks::STYLES[(p.kind-2)as usize];let words=cached_frame(fast.tracks[i],phase(p),source_tracks::PARTICLE_TRACKS);assert!(words.is_some());cached_reads+=1;
    assert_eq!(tick_shape(p,s,words),tick_shape(p,s,None));assert_eq!(draw_shape(p,s,words),draw_shape(p,s,None));
    if tick<140&&p.position[1]==y-ONE+tick_shape(p,s,words).1 {floor_contacts+=1;}
   }}
   if tick==310 {
    fast.clear_scene(scene);reference.clear_scene(scene);
    // Generic input must overwrite stale tracked slot IDs with the scalar sentinel.
    spawn_specs(&mut fast,scene,owner,2,-1,source_tracks::EMITTERS,source_tracks::STYLES);
    spawn_specs(&mut reference,scene,owner,2,-1,source_tracks::EMITTERS,source_tracks::STYLES);
    assert!(fast.tracks.iter().all(|&t|t==0));
    // Rebind the exact generated prefix for the second half of the trajectory.
    fast.clear_scene(scene);reference.clear_scene(scene);
    spawn_specs_with_bases(&mut fast,scene,owner,2,-1,source_tracks::EMITTERS,source_tracks::STYLES,source_tracks::EMITTER_TRACK_BASES);
    spawn_specs(&mut reference,scene,owner,2,-1,source_tracks::EMITTERS,source_tracks::STYLES);
   }
  }
 }
 assert!(cached_reads>10000);assert!(floor_contacts>0);println!("cached_trajectory_draw_checks={cached_reads} floor_contacts={floor_contacts}");
}
