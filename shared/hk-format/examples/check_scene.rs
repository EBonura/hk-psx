fn main(){
    let capacity:usize=std::env::var("HK_SCENE_ARENA_BYTES").ok().map(|s|s.parse().unwrap()).unwrap_or(5*256*1024);
    for name in std::env::args().skip(1){
        let input=std::fs::read(&name).unwrap();let mut bytes=vec![0;capacity];
        assert!(input.len().div_ceil(2048)*2048<=capacity);
        bytes[..input.len()].copy_from_slice(&input);
        let n=psx_pack::decompress_hlzc_in_place(&mut bytes,input.len()).expect("in-place scene decode");
        let scene=hk_format::Scene::parse(&bytes[..n]).expect("resident scene format");
        for i in 0..scene.room_count(){let room=scene.room(i).unwrap();
            for j in 0..room.counts[2]{let _=room.draw(j);}
            for j in 0..room.counts[3]{let _=room.frame(j);}
            for j in 0..room.counts[4]{let _=room.clip(j);}
            for j in 0..room.counts[5]{let _=room.edge(j);}
        }
        println!("{} {} {} {} {} {}",name,n,psx_pack::fnv1a32(&bytes[..n]),scene.room_count(),scene.texture_count(),scene.page_count());
    }
}
