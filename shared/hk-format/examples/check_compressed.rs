fn main() {
    let capacity: usize = std::env::var("HK_ROOM_ARENA_BYTES")
        .map(|v| v.parse().expect("arena byte capacity"))
        .unwrap_or(384 * 1024);
    let max_pages: usize = std::env::var("HK_ROOM_MAX_PAGES")
        .map(|v| v.parse().expect("static page limit"))
        .unwrap_or(5);
    let max_textures: usize = std::env::var("HK_ROOM_MAX_TEXTURES")
        .map(|v| v.parse().expect("texture limit"))
        .unwrap_or(384);
    for name in std::env::args().skip(1) {
        let source = std::fs::read(&name).unwrap();
        assert!(
            source.len().div_ceil(2048) * 2048 <= capacity,
            "sector-rounded input capacity"
        );
        let mut arena = vec![0; capacity];
        arena[..source.len()].copy_from_slice(&source);
        let n = psx_pack::decompress_hlzc_in_place(&mut arena, source.len())
            .expect("in-place decompression capacity");
        let room = hk_format::Room::parse(&arena[..n]).expect("decoded room format");
        assert!(
            room.counts[0] <= max_pages,
            "static VRAM bank page capacity"
        );
        assert!(room.counts[1] <= max_textures, "VRAM bank CLUT capacity");
        println!("{} {} {}", name, n, psx_pack::fnv1a32(&arena[..n]));
    }
}
