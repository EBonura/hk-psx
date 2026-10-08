#[path="../../../game/src/tile_coverage.rs"] mod tile_coverage;

#[path="../../../game/src/tile_events.rs"]
mod tile_events;

#[path="../../../game/src/tile_frame_cache.rs"]
mod tile_frame_cache;

#[path="../../../game/src/back_prebuild.rs"]
mod back_prebuild;

#[test]
fn back_prefix_resumes_mixed_source_events_including_invisible_draws() {
    const N:usize=64;
    let fronts:[bool;N]=core::array::from_fn(|i|i%5==1);
    let owners=core::array::from_fn(|tile|{let i=(tile*17)%N;i as u16+1+if fronts[i]{1024}else{0}});
    for stop in 0..=N {
        let mut events=tile_events::Events::<N>::new();let mut rows=[0;15];events.build(&owners,&mut rows);
        let mut emitted=Vec::new();
        for i in 0..stop {if fronts[i]{continue;}events.reach(i,false,&mut rows);if i%3!=0{emitted.push(i);}}
        let mut prefix=back_prebuild::Prefix::new();prefix.save((11,22),240,0,stop,emitted.len(),3,emitted.len()as u32,1088,&rows);
        rows.fill(0);rows.copy_from_slice(&prefix.rows);
        for i in prefix.next..N {if fronts[i]{continue;}events.reach(i,false,&mut rows);if i%3!=0{emitted.push(i);}}
        assert_eq!(emitted,(0..N).filter(|&i|!fronts[i]&&i%3!=0).collect::<Vec<_>>());
        for y in 0..15 {assert_eq!(rows[y],(0..20).fold(0,|bits,x|bits|u32::from(owners[y*20+x]>1024)<<x));}
        for i in 0..N {if fronts[i]{events.reach(i,true,&mut rows);}}
        assert_eq!(rows,[0;15]);
    }
}

#[path="../../../game/src/occluder_snapshot.rs"]
mod occluder_snapshot;
