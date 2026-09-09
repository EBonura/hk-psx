#[path="../../../game/src/room_decode.rs"] mod room_decode;
use room_decode::{Decoder,Error};
fn room()->Vec<u8> {
    let mut b=vec![0u8;92];b[..8].copy_from_slice(b"HKROOM02");
    b[12..16].copy_from_slice(&1u32.to_le_bytes());
    b[32..36].copy_from_slice(&4u32.to_le_bytes());
    b[40..42].copy_from_slice(&u16::MAX.to_le_bytes());
    b[46..48].copy_from_slice(&1u16.to_le_bytes());
    b[48..50].copy_from_slice(&1u16.to_le_bytes());b
}
fn framed(raw:&[u8])->Vec<u8> {
    let mut b=b"HLZC".to_vec();b.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    b.extend_from_slice(&[0xf0,raw.len() as u8-15]);b.extend_from_slice(raw);b
}
fn decode(source:&[u8],raw:&[u8],budget:usize,capacity:usize)->Result<Vec<u8>,Error> {
    let mut arena=vec![0;capacity];let copy=source.len().min(capacity);arena[..copy].copy_from_slice(&source[..copy]);
    let mut d=Decoder::new(source.len(),psx_pack::fnv1a32(source),raw.len(),psx_pack::fnv1a32(raw));
    for _ in 0..10000 {
        if let Some(n)=d.step(&mut arena,budget)? {arena.truncate(n);return Ok(arena);}
    }
    panic!("bounded decoder did not terminate")
}
#[test] fn raw_and_compressed_resume_at_every_byte_boundary() {
    let raw=room();
    for source in [&raw, &framed(&raw)] {
        for budget in [1,2,3,7,31,4096] {assert_eq!(decode(source,&raw,budget,256).unwrap(),raw);}
    }
}
#[test] fn zero_budget_never_modifies_storage() {
    let raw=room();let mut arena=raw.clone();let mut d=Decoder::new(raw.len(),0,raw.len(),0);
    for _ in 0..100 {assert_eq!(d.step(&mut arena,0),Ok(None));}
    assert_eq!(arena,raw);
}
#[test] fn corrupt_hash_and_invalid_format_are_never_admitted() {
    let raw=room();let mut arena=raw.clone();let mut d=Decoder::new(raw.len(),0,raw.len(),0);
    assert_eq!(d.step(&mut arena,4096),Err(Error::Checksum));
    let mut bad=raw.clone();bad[0]=0;
    assert_eq!(decode(&bad,&bad,1,256),Err(Error::RoomFormat));
    let mut d=Decoder::new(raw.len(),psx_pack::fnv1a32(&raw),raw.len(),0);
    assert_eq!(d.step(&mut arena,4096),Err(Error::Checksum));
}
#[test] fn truncated_and_zero_distance_matches_fail_without_overrun() {
    let raw=room();
    for block in [vec![0x00,0x00,0x00],vec![0xf0,0xff],vec![0x10,0x11,0x01],vec![0xff,255,255]] {
        let mut source=b"HLZC".to_vec();source.extend_from_slice(&(raw.len() as u32).to_le_bytes());source.extend(block);
        assert_eq!(decode(&source,&raw,1,256),Err(Error::Decompress));
    }
    assert_eq!(decode(&framed(&raw),&raw,1,64),Err(Error::Decompress));
}

#[test] fn short_distance_replication_and_unaligned_hash_slices_resume_exactly() {
    fn extension(out:&mut Vec<u8>, mut n:usize) {
        while n>=255 {out.push(255);n-=255;}out.push(n as u8);
    }
    for distance in [1usize,2,3,4,5,7,8,16,31,64] {
      for padding in 0..4 {
        let mut raw=room();raw.truncate(88);raw.extend((0..padding).map(|v|(v*59) as u8));
        let seed:Vec<u8>=(0..distance).map(|v|(v*37+19) as u8).collect();
        raw.extend_from_slice(&seed);
        let literals=raw.len();let matched=6001;
        for i in 0..matched {raw.push(seed[i%distance]);}
        let stream_len=(raw.len()-88) as u32;raw[32..36].copy_from_slice(&stream_len.to_le_bytes());
        let mut source=b"HLZC".to_vec();source.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        source.push(0xff);extension(&mut source,literals-15);
        source.extend_from_slice(&raw[..literals]);source.extend_from_slice(&(distance as u16).to_le_bytes());
        extension(&mut source,matched-4-15);
        let mut expected=vec![0;16384];expected[..source.len()].copy_from_slice(&source);
        let len=psx_pack::decompress_hlzc_in_place(&mut expected,source.len()).unwrap();
        assert_eq!(&expected[..len],&raw);
        for budget in [1,2,3,7,31,1024,2048,8192] {
            // Budget1 performs >10k steps for this larger fixture.
            let mut arena=vec![0;16384];arena[..source.len()].copy_from_slice(&source);
            let mut decoder=Decoder::new(source.len(),psx_pack::fnv1a32(&source),raw.len(),psx_pack::fnv1a32(&raw));
            let mut complete=false;
            for _ in 0..100_000 {
                if let Some(n)=decoder.step(&mut arena,budget).unwrap() {
                    assert_eq!(&arena[..n],&raw);complete=true;break;
                }
            }
            assert!(complete);
        }
    }
  }
}
