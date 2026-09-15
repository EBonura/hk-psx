//! Verify the exact shipped sequential resident-scene allocation. No disc IO.
#[path="../../../game/src/room_decode.rs"] mod room_decode;
fn main() {
    let args:Vec<String>=std::env::args().skip(1).collect();
    assert!(args.len()>=3,"capacity then scene pairs, optionally --atlases and upload pairs");
    let capacity:usize=args[0].parse().expect("arena byte capacity");
    assert_eq!(capacity%4,0);
    let split=args.iter().position(|v|v=="--atlases").unwrap_or(args.len());
    assert_eq!((split-1)%2,0,"scene pairs");
    let atlas_args=if split<args.len(){&args[split+1..]}else{&[]};
    assert_eq!(atlas_args.len()%2,0,"atlas pairs");
    let atlases:Vec<_>=atlas_args.chunks_exact(2).map(|p|{
        (p[0].clone(),std::fs::read(&p[0]).unwrap(),std::fs::read(&p[1]).unwrap())
    }).collect();
    let pairs:Vec<_>=args[1..split].chunks_exact(2).map(|p|{
        (p[0].clone(),std::fs::read(&p[0]).unwrap(),std::fs::read(&p[1]).unwrap())
    }).collect();
    let mut expected=vec![0xa5;capacity];let mut offset=0;
    for (path,stored,raw)in &atlases {
        assert!(stored.len().div_ceil(2048)*2048<=capacity,"atlas sector-rounded read: {path}");
        expected[..stored.len()].copy_from_slice(stored);
        let n=psx_pack::decompress_hlzc_in_place(&mut expected,stored.len()).expect("pinned SDK atlas overlap proof");
        assert_eq!(n,raw.len());assert_eq!(&expected[..n],raw);
    }
    for (path,stored,raw) in &pairs {
        assert!(stored.len().div_ceil(2048)*2048<=capacity-offset,"sector-rounded read: {path}");
        let prefix=expected[..offset].to_vec();
        expected[offset..offset+stored.len()].copy_from_slice(stored);
        let n=psx_pack::decompress_hlzc_in_place(&mut expected[offset..],stored.len()).expect("pinned SDK overlap proof");
        assert_eq!(n,raw.len());assert_eq!(&expected[offset..offset+n],raw);
        hk_format::Scene::parse(raw).expect("native scene validation");
        assert_eq!(&expected[..offset],&prefix);offset=(offset+n+3)&!3;
    }
    for budget in [1,31,1024,2048,8192] {
        let mut arena=vec![0xa5;capacity];let mut offset=0;
        // Every completed upload relinquishes its CPU staging bytes. No atlas
        // slice survives into the next chunk or compact resident admission.
        for(path,stored,raw)in &atlases {
            arena[..stored.len()].copy_from_slice(stored);
            let mut decoder=room_decode::Decoder::new_bytes(stored.len(),psx_pack::fnv1a32(stored),raw.len(),psx_pack::fnv1a32(raw));
            let mut calls=0usize;
            loop {
                calls+=1;assert!(calls<32_000_000,"atlas decoder failed to progress: {path}");
                if let Some(n)=decoder.step(&mut arena,budget).expect("incremental atlas decoder") {
                    assert_eq!(n,raw.len());break;
                }
            }
            assert_eq!(&arena[..raw.len()],raw,"atlas bytes: {path},budget{budget}");
        }
        for (path,stored,raw) in &pairs {
            let prefix=arena[..offset].to_vec();
            arena[offset..offset+stored.len()].copy_from_slice(stored);
            let mut decoder=room_decode::Decoder::new_scene(stored.len(),psx_pack::fnv1a32(stored),raw.len(),psx_pack::fnv1a32(raw));
            let mut calls=0usize;
            loop {
                calls+=1;assert!(calls<32_000_000,"decoder failed to progress: {path}");
                if let Some(n)=decoder.step(&mut arena[offset..],budget).expect("incremental scene decoder") {
                    assert_eq!(n,raw.len());break;
                }
            }
            assert_eq!(&arena[offset..offset+raw.len()],raw,"decoded bytes: {path},budget{budget}");
            assert_eq!(&arena[..offset],&prefix,"earlier scene changed: {path}");
            offset=(offset+raw.len()+3)&!3;
        }
        assert_eq!(&arena[..offset],&expected[..offset]);
    }
    let mut offset=0;
    for (path,_,raw) in pairs {
        println!("{} {} {} {} {}",path,raw.len(),psx_pack::fnv1a32(&raw),offset,capacity-offset);
        offset=(offset+raw.len()+3)&!3;
    }
}
