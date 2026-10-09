//! Exercise the actual guest DMA packet writer, including reuse after scissors.
#[path = "../../../game/src/draw_packet.rs"]
mod packet;

#[test]
fn opaque_black_reuse_replaces_command_and_excludes_stale_texture_and_scissor_words() {
    let mut p =
        packet::Packet::scissored([0x2e80_8080, 1, 2, 3, 4, 5, 6, 7, 8], [3, 7, 103, 207], 240);
    let vertices = [0xffec_ffec, 0xffec_0168, 0x0104_ffec, 0x0104_0168];
    p.write_black(vertices);
    assert_eq!(p.word_count(), 5);
    assert_eq!(
        &p.words[..5],
        &[
            0x2800_0000,
            vertices[0],
            vertices[1],
            vertices[2],
            vertices[3]
        ]
    );
    assert_eq!(
        p.words[0] & 0x0200_0000,
        0,
        "opaque F4 must not inherit semi-transparency"
    );
    // Reusing that slot for a textured draw must restore all nine words too.
    let original = [0x2e80_8080, 11, 12, 13, 14, 15, 16, 17, 18];
    p.write_plain(original);
    assert_eq!(p.word_count(), 9);
    assert_eq!(&p.words[..9], &original);
}
