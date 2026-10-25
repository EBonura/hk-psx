// The display buffers the opening poem and movies draw into, shared by the program (intro/src/main.rs) and the
// game (game/src/intro.rs, which includes this file): the game asserts at build time that none of them lies
// over the VRAM its first scene's atlases are uploaded to before the movies (disc.rs `scene_clobbered`).
//
// `x`, `y` in VRAM halfwords, 320 by 240 each. The first two are the game's own frame buffers; the third lies
// at x 576, y 256 over static texture pages 13 to 17, which the first scene does not use (it has pages 0 to
// 12), so nothing that is loaded before the movies has to wait for after them. None of them touches the CLUT
// banks (x 0..319, y 480..499 and x 320..383, y 256..463), the animation slots or the shared strips.
/// The three buffers, in the order the movie player takes them.
pub const MOVIE_BUFFERS: [(u16, u16); 3] = [(0, 0), (0, 240), (576, 256)];
