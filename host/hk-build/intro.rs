//! The opening cinematics on a new game.
//!
//! The game's image has almost no RAM to spare, so the movie player cannot be
//! part of it. `intro/` is a small program (linked at the room-module pool, see
//! intro/src/main.rs) that streams the Prologue and the Intro (`HKINTRO1.STR`,
//! `HKINTRO2.STR`) through the SDK's MDEC player and then loads the disc's boot
//! executable, the game, back. A build with `--intro` compiles it into
//! `data/intro.exe`, which the guest build (host/build_guest.py `boot_art`)
//! appends to the boot art chunk and the `intro` feature reads back when a new
//! game starts (game/src/intro.rs); the disc gets the two movie files.
//!
//! The movies come from the player's own install (`host/hk-intro-cook`) and
//! live, with everything derived from them, under `.hkpsx/intro/`, never in
//! the repository. A machine without the encoder (or an install without the
//! clips) builds the disc without an intro stage.
//!
//! The replay gates press New Game from boot by poll count and must not meet a
//! movie first, so the ordinary `build` has no intro at all and the intro build
//! is its own action (`build --intro --no-validate`).

use super::*;

/// Where the intro program links: inside the room-module pool, which is empty
/// at the title where the game jumps to it.
pub const LOAD_ADDR: u32 = 0x801D_B000;
/// The cooker's exit status for "no intro on this machine".
const NO_INTRO: i32 = 3;
/// The movies, in play order, and the piano of the poem that comes before them, as raw-sector files for
/// `mkisopsx --xa-file`.
pub const MOVIE_FILES: [&str; 3] = ["HKPOEM.XA", "HKINTRO1.STR", "HKINTRO2.STR"];

pub fn dir(repository: &Path) -> PathBuf {
    repository.join(".hkpsx/intro")
}

/// The cooked movies, if this checkout has them.
pub fn cooked(repository: &Path) -> Option<PathBuf> {
    let dir = dir(repository);
    // `sound.json` is written last, by a cook that put the install's own sound under the movies.
    (MOVIE_FILES.iter().all(|name| dir.join(name).is_file())
        && dir.join("movie.rs").is_file()
        && std::fs::read_to_string(dir.join("sound.json"))
            .is_ok_and(|text| text.contains("install AudioClips")))
    .then_some(dir)
}

/// Encode the install's two opening movies. Without the encoder or the clips
/// the stage is dropped (and any earlier one removed, so the disc cannot
/// carry a movie that no longer matches the install).
pub fn cook(repository: &Path, data_dir: &Path) -> Result<bool> {
    let manifest = repository.join("host/hk-intro-cook/Cargo.toml");
    let mut build = Command::new("cargo");
    build
        .current_dir(repository)
        .args(["build", "--release", "--manifest-path"])
        .arg(&manifest);
    run_command(&mut build, "build the intro movie cooker")?;
    let out = dir(repository);
    // The original plays the movies' sound from separate AudioClips (the clips themselves are silent).
    hk_cook::intro_audio::main(repository, Some(data_dir))?;
    println!("\n==> cook the intro movies");
    let status = Command::new(
        repository.join("host/hk-intro-cook/target/release/hk-intro-cook"),
    )
    .arg("--data-dir")
    .arg(data_dir)
    .arg("--out")
    .arg(&out)
    .arg("--audio-dir")
    .arg(out.join("audio"))
    .status()?;
    match status.code() {
        Some(0) => Ok(true),
        Some(NO_INTRO) => {
            let _ = std::fs::remove_dir_all(&out);
            println!("warning: no intro movies cooked; the disc will boot straight into the game");
            Ok(false)
        }
        _ => Err(format!("cook the intro movies failed with {status}").into()),
    }
}

fn run_command(command: &mut Command, label: &str) -> Result<()> {
    println!("\n==> {label}");
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{label} failed with {status}").into());
    }
    Ok(())
}

/// One of the SDK's post-link checks (`tools/psoxide-hazard`: `hazard-patch`,
/// `hazard-scan`), built from the hydrated tree; append the tool's own arguments.
fn hazard_tool(psoxide: &Path, bin: &str) -> Command {
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet", "--release", "--locked", "--manifest-path"])
        .arg(psoxide.join("tools/psoxide-hazard/Cargo.toml"))
        .args(["--bin", bin, "--"]);
    command
}

/// The skip prompt's words, as the original's language sheet has them
/// (`MainMenu` / `TEXT_SKIP_PROMPT`).
pub const SKIP_PROMPT: &str = "Press any button to skip";

/// The value of `pub const NAME: T = <integer>;` in a generated Rust file.
fn const_int(text: &str, name: &str) -> Result<i64> {
    let head = format!("pub const {name}:");
    let line = text
        .lines()
        .find(|l| l.starts_with(&head))
        .ok_or_else(|| format!("no `{head}` in the generated font layout"))?;
    let value = line.split('=').nth(1).and_then(|v| v.trim().trim_end_matches(';').trim().parse().ok());
    value.ok_or_else(|| format!("`{name}` is not an integer: {line}").into())
}

/// The numbers of `pub const NAME: [u8; N] = [..];` (or `NAME:[u8;N]=[..]`).
fn const_bytes(text: &str, name: &str) -> Result<Vec<u8>> {
    let line = text
        .lines()
        .find(|l| l.starts_with(&format!("pub const {name}:")))
        .ok_or_else(|| format!("no `{name}` array in the generated font data"))?;
    let body = line.rsplit('=').next().unwrap_or("").trim().trim_end_matches(';');
    body.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|n| n.trim().parse::<u8>().map_err(|e| format!("`{name}`: {e}").into()))
        .collect()
}

/// The game's font as the dialogue draws it: where a character's cell sits in the texture page and how far
/// the pen moves after it (game/src/dialogue.rs).
struct Font {
    advances: Vec<u8>,
    u0: i64,
    w: i64,
    per_row: i64,
    v0: i64,
    pitch: i64,
}

impl Font {
    fn read(font_layout: &str, read_points: &str) -> Result<Font> {
        Ok(Font {
            advances: const_bytes(read_points, "ADVANCES")?,
            u0: const_int(font_layout, "GLYPH_U0")?,
            w: const_int(font_layout, "GLYPH_W")?,
            per_row: const_int(font_layout, "GLYPH_PER_ROW")?,
            v0: const_int(font_layout, "FONT_V0")?,
            pitch: const_int(font_layout, "GLYPH_ROW_PITCH")?,
        })
    }

    fn advance(&self, b: u8) -> Result<i64> {
        self.advances
            .get(usize::from(b).wrapping_sub(32))
            .map(|a| i64::from(*a))
            .ok_or_else(|| format!("no advance for {:?}", b as char).into())
    }

    /// The width of `text`.
    fn width(&self, text: &str) -> Result<i64> {
        text.bytes().try_fold(0, |x, b| Ok(x + self.advance(b)?))
    }

    /// One `(u, v, x)` per visible glyph of `text`, the pen starting at `x0`.
    fn glyphs(&self, text: &str, x0: i64) -> Result<Vec<(i64, i64, i64, i64)>> {
        let mut x = x0;
        let mut out = Vec::new();
        for b in text.bytes() {
            let i = i64::from(b) - 32;
            if b != b' ' {
                // The cell is drawn as wide as the glyph's advance and a little over, not its whole 13 texels:
                // the font's cells carry a stray texel at their far edge (the `p`'s, drawn last in a row,
                // is a lone speck after it).
                let drawn = (self.advance(b)? + 2).min(self.w);
                out.push((self.u0 + i % self.per_row * self.w, self.v0 + i / self.per_row * self.pitch, x, drawn));
            }
            x += self.advance(b)?;
        }
        Ok(out)
    }
}

/// The skip prompt laid out in the game's own font, which the game has in VRAM
/// when it starts the movies (the intro draws it with raw GP0 commands, no
/// font file of its own): one `(u, v, x)` row per visible glyph, as
/// game/src/dialogue.rs places them, and the string's width. After it the
/// poem's lines ([`poem_source`]).
pub fn prompt_source(font_layout: &str, read_points: &str, poem_font: &[u8]) -> Result<String> {
    let font = Font::read(font_layout, read_points)?;
    let mut rows = String::new();
    let glyphs = font.glyphs(SKIP_PROMPT, 0)?;
    let mut widths = String::new();
    for (u, v, x, w) in &glyphs {
        rows += &format!("    ({u}, {v}, {x}),\n");
        widths += &format!("{w}, ");
    }
    Ok(format!(
        "// Generated by hk-psx-build (host/hk-build/intro.rs) from data/font_layout.rs and\n\
         // data/read_points.rs.\n\
         pub const WIDTH: i16 = {};\n\
         pub const FONT_X: u16 = {};\n\
         pub const FONT_PAGE_Y: u16 = {};\n\
         pub const GLYPH_W: u16 = {};\n\
         pub const GLYPH_H: u16 = {};\n\
         pub const GLYPHS: [(u8, u8, i16); {}] = [\n{rows}];\n\
         /// How wide each of them is drawn (its advance and two texels, at most the cell).\n\
         pub const GLYPH_WIDTHS: [u16; {}] = [{widths}];\n{}",
        font.width(SKIP_PROMPT)?,
        const_int(font_layout, "FONT_X")?,
        const_int(font_layout, "FONT_PAGE_Y")?,
        font.w,
        const_int(font_layout, "GLYPH_H")?,
        glyphs.len(),
        glyphs.len(),
        poem_source(&font, &PoemFont::read(poem_font)?)?,
    ))
}

/// The excerpt of Monomon's elegy the original shows before its movies (`Opening_Sequence`, language sheet
/// `General`, keys `PROLOGUE_EXCERPT_01` to `_04` and `_AUTHOR`, in their order on the screen). The
/// author's line has typographic quotes in the sheet; the game's font has ASCII only.
pub const POEM: [&str; 5] = [
    "In wilds beyond they speak your name with reverence and regret,",
    "For none could tame our savage souls yet you the challenge met,",
    "Under palest watch, you taught, we changed, base instincts were redeemed,",
    "A world you gave to bug and beast as they had never dreamed.",
    "- From 'Elegy for Hallownest' by Monomon the Teacher",
];
/// The widest a row of the poem may be: 288 of the picture's 320 columns, so the text stays inside the
/// title-safe area (the central 90%, columns 16 to 304) whatever a CRT's overscan hides.
const POEM_WIDTH: i64 = 288;
/// Rows apart within one line of the poem, and between lines: the extra space before the author's line is
/// the original's (its line sits further down than the pitch of the four above it).
const POEM_ROW_PITCH: i64 = 16;
const POEM_LINE_GAP: i64 = 5;
const POEM_AUTHOR_GAP: i64 = 10;
/// Where the block ends. The original anchors it at the screen's middle and runs it downward (the scene's
/// transforms: the first line sits on the full-screen vignette's centre, the others 0.78 units apart, the
/// author's line 3.79 under the first, the whole block 0.8 units left of centre), so that its lower edge is
/// about three quarters of the way down. Its text is about a third of the size of ours (a 5 px em at this
/// resolution), so our block is too tall to start at the middle: it ends where the prompt's 28 line zone
/// (from line 212) starts, less a margin, and so sits in the lower middle as the original's does.
const POEM_BOTTOM: i64 = 204;
/// The author's line is set smaller than the verse (the scene's text sizes: 3.85 against 5) and right aligned
/// in the same 20 unit wide box (`m_textAlignment` 2, top right; the verse's is 1, top centre). It is set from
/// the font's outlines at that size (data/poem-font.hk), not the verse's cells scaled down.
/// Where the author's line ends (the verse's box ends at the title-safe edge, 304).
const AUTHOR_RIGHT: i64 = 300;
/// Where the poem's cells are uploaded: the movies' third display buffer, at (576, 256) (intro/src/buffers.rs
/// `MOVIE_BUFFERS`; a test keeps the two together), free during the poem and left alone by the first scene's load.
const STRIP_VRAM: (u16, u16) = (576, 256);
/// The first line the poem may use (the title-safe area starts at 12).
const POEM_TOP_LIMIT: i64 = 12;

/// `text` broken greedily at spaces into rows no wider than `limit`.
fn wrap_greedy(font: &Font, text: &str, limit: i64) -> Result<Vec<String>> {
    let mut rows: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match rows.last_mut() {
            Some(row) if font.width(&format!("{row} {word}"))? <= limit => {
                row.push(' ');
                row.push_str(word);
            }
            _ => rows.push(word.to_string()),
        }
    }
    Ok(rows)
}

/// `text` in as few rows as fit `limit`, the rows as even as the words allow (the narrowest limit that
/// keeps the row count), so that a line that wraps is not left with one word on its second row.
fn wrap(font: &Font, text: &str, limit: i64) -> Result<Vec<String>> {
    let rows = wrap_greedy(font, text, limit)?;
    let mut best = rows;
    if best.len() > 1 {
        let count = best.len();
        for narrower in (0..limit).rev() {
            let rows = wrap_greedy(font, text, narrower)?;
            if rows.len() != count {
                break;
            }
            best = rows;
        }
    }
    Ok(best)
}

/// The poem's own cells (data/poem-font.hk, hk-cook font_strip.rs `cook_poem`): the verse's three rows taller than
/// the dialogue's so a descender is whole, the author's line at its own smaller em.
struct PoemFont {
    body_w: usize,
    body_h: usize,
    author_w: usize,
    author_h: usize,
    author_advances: Vec<u8>,
    body: Vec<Vec<u8>>,
    author: Vec<Vec<u8>>,
}

impl PoemFont {
    fn read(blob: &[u8]) -> Result<PoemFont> {
        let header = blob.get(..4).ok_or("poem-font.hk is empty")?;
        let (body_w, body_h, author_w, author_h) = (usize::from(header[0]), usize::from(header[1]), usize::from(header[2]), usize::from(header[3]));
        let cells = |from: usize, w: usize, h: usize| -> Result<Vec<Vec<u8>>> {
            (0..95)
                .map(|i| blob.get(from + i * w * h..from + (i + 1) * w * h).map(<[u8]>::to_vec).ok_or_else(|| "poem-font.hk is short".into()))
                .collect()
        };
        let author_advances = blob.get(4..99).ok_or("poem-font.hk is short")?.to_vec();
        let body = cells(99, body_w, body_h)?;
        let author = cells(99 + 95 * body_w * body_h, author_w, author_h)?;
        Ok(PoemFont { body_w, body_h, author_w, author_h, author_advances, body, author })
    }

    fn author_advance(&self, b: u8) -> Result<i64> {
        self.author_advances
            .get(usize::from(b).wrapping_sub(32))
            .map(|a| i64::from(*a))
            .ok_or_else(|| format!("no advance for {:?}", b as char).into())
    }

    fn author_width(&self, text: &str) -> Result<i64> {
        text.bytes().try_fold(0, |x, b| Ok(x + self.author_advance(b)?))
    }
}

/// One glyph of the poem as drawn: its cell in the strip, its place on the screen and the size of the rectangle.
struct PoemGlyph {
    u: usize,
    v: usize,
    x: i64,
    y: i64,
    w: usize,
    h: usize,
}

/// The cells the poem draws, packed into one 4bpp image 256 texels wide (one texture page): the verse's
/// characters first, then the author's. `place` gives a cell's (u, v) for a (author, character).
struct PoemStrip {
    texels: Vec<u8>,
    rows: usize,
    cells: Vec<((bool, u8), (usize, usize))>,
}

impl PoemStrip {
    fn new(font: &PoemFont, verse: &[String], author: &str) -> PoemStrip {
        let mut body: Vec<u8> = verse.iter().flat_map(|r| r.bytes()).filter(|b| *b != b' ').collect();
        body.sort_unstable();
        body.dedup();
        let mut by: Vec<u8> = author.bytes().filter(|b| *b != b' ').collect();
        by.sort_unstable();
        by.dedup();
        let (body_per_row, author_per_row) = (256 / font.body_w, 256 / font.author_w);
        let body_rows = body.len().div_ceil(body_per_row);
        let author_rows = by.len().div_ceil(author_per_row);
        let rows = body_rows * font.body_h + author_rows * font.author_h;
        let mut texels = vec![0u8; 256 * rows];
        let mut cells = Vec::new();
        for (i, &ch) in body.iter().enumerate() {
            let (u, v) = (i % body_per_row * font.body_w, i / body_per_row * font.body_h);
            for y in 0..font.body_h {
                for x in 0..font.body_w {
                    texels[(v + y) * 256 + u + x] = font.body[usize::from(ch) - 32][y * font.body_w + x];
                }
            }
            cells.push(((false, ch), (u, v)));
        }
        let top = body_rows * font.body_h;
        for (i, &ch) in by.iter().enumerate() {
            let (u, v) = (i % author_per_row * font.author_w, top + i / author_per_row * font.author_h);
            for y in 0..font.author_h {
                for x in 0..font.author_w {
                    texels[(v + y) * 256 + u + x] = font.author[usize::from(ch) - 32][y * font.author_w + x];
                }
            }
            cells.push(((true, ch), (u, v)));
        }
        PoemStrip { texels, rows, cells }
    }

    fn place(&self, author: bool, ch: u8) -> Result<(usize, usize)> {
        self.cells
            .iter()
            .find(|(key, _)| *key == (author, ch))
            .map(|(_, at)| *at)
            .ok_or_else(|| format!("no cell for {:?}", ch as char).into())
    }

    /// The image as 32 bit words, 8 texels each, row by row: the VRAM rectangle's contents (64 halfwords wide).
    fn words(&self) -> Vec<u32> {
        self.texels
            .chunks(8)
            .map(|t| t.iter().enumerate().fold(0u32, |w, (i, &v)| w | u32::from(v) << (4 * i)))
            .collect()
    }
}

/// What the original's `Prologue Text` animation does to the poem: when each of the five lines fades in
/// and the black cover (`fader`) over everything fades out and in again. Read from the clip's streamed
/// curves (`AnimationClip` "Prologue Text" in sharedassets5.assets, 30.77 s at 30 fps, cubic segments
/// `((a s + b) s + c) s + d` from each key's time); the lines are `excerpt body 1` to `4` and `excerpt
/// author`, the cover is `fader`. The sequence goes on to the movies at 28.6 s.
const KEYS: [(&str, &[(f64, [f64; 4])]); 6] = [
    ("body 1", &[(1.4333, [0.0, 0.0, 0.227_272_72, 0.0]), (5.8333, [0.0, 0.0, 0.0, 1.0])]),
    ("body 2", &[(6.3333, [0.0, 0.0, 0.447_761_24, 0.0]), (8.5667, [0.0, 0.0, 0.0, 1.0])]),
    ("body 3", &[(9.7667, [0.0, 0.0, 0.416_666_57, 0.0]), (12.1667, [0.0, 0.0, 0.0, 1.0])]),
    ("body 4", &[(13.5, [0.0, 0.0, 0.405_405_5, 0.0]), (15.9667, [0.0, 0.0, 0.0, 1.0])]),
    (
        "author",
        &[
            (17.1, [-0.032, 0.080_000_006, 0.4, 0.0]),
            (19.6, [0.002_450_947_4, -0.044_280_447, 0.2, 1.0]),
            (28.6333, [0.0, 0.0, 0.0, 1.0]),
        ],
    ),
    (
        "fader",
        &[
            (0.0, [-0.037_639_264, 0.067_750_67, 0.0, 1.0]),
            (1.8, [0.021_764_044, -0.118_976_78, -0.121_951_21, 1.0]),
            (5.9, [0.0, 0.0, 0.0, 0.0]),
            (23.6333, [-0.022_442_913, 0.150_367_53, 0.0, 0.0]),
            (28.1, [0.0, 0.0, 0.0, 1.0]),
        ],
    ),
];
/// Before its first key a line is invisible and the cover is opaque.
const BEFORE_FIRST: [f64; 6] = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
/// The poem ends here: the animator's `normalizedFinishTime` (0.9296 of 30.77 s).
pub const POEM_END_MS: i64 = 28_600;
/// The table's step, in milliseconds.
pub const ALPHA_STEP_MS: i64 = 100;

/// The curve `curve` at `t` seconds, 0 to 1.
fn curve_at(curve: usize, t: f64) -> f64 {
    let keys = KEYS[curve].1;
    let value = match keys.iter().rev().find(|(at, _)| *at <= t) {
        Some((at, [a, b, c, d])) => {
            let s = t - at;
            ((a * s + b) * s + c) * s + d
        }
        None => BEFORE_FIRST[curve],
    };
    value.clamp(0.0, 1.0)
}

/// The generated source of the poem: its rows of glyphs in the game's font, centred and grouped by line
/// (each line of the poem fades as one), and the table of how bright each line is, in `ALPHA_STEP_MS`
/// steps, with the cover's last.
fn poem_source(font: &Font, poem: &PoemFont) -> Result<String> {
    let rows: Vec<Vec<String>> = POEM[..4]
        .iter()
        .map(|line| wrap(font, line, POEM_WIDTH))
        .collect::<Result<_>>()?;
    let all_rows: Vec<String> = rows.iter().flatten().cloned().collect();
    let strip = PoemStrip::new(poem, &all_rows, POEM[4]);
    let count: i64 = rows.iter().map(|r| r.len() as i64).sum();
    let height = count * POEM_ROW_PITCH + 3 * POEM_LINE_GAP + POEM_AUTHOR_GAP + poem.author_h as i64;
    if POEM_BOTTOM - height < POEM_TOP_LIMIT {
        return Err(format!("the poem is {height} lines tall, ending at {POEM_BOTTOM} it would start above {POEM_TOP_LIMIT}").into());
    }
    let mut y = POEM_BOTTOM - height;
    let mut drawn: Vec<PoemGlyph> = Vec::new();
    let mut ends = Vec::new();
    for (group, line) in rows.iter().enumerate() {
        for row in line {
            let mut x = (320 - font.width(row)?) / 2;
            for b in row.bytes() {
                if b != b' ' {
                    // As wide as the glyph's advance and a little over: the cells carry a stray texel at their far edge.
                    let (u, v) = strip.place(false, b)?;
                    let w = (font.advance(b)? + 2).min(poem.body_w as i64) as usize;
                    drawn.push(PoemGlyph { u, v, x, y, w, h: poem.body_h });
                }
                x += font.advance(b)?;
            }
            y += POEM_ROW_PITCH;
        }
        ends.push(drawn.len());
        y += if group == 3 { POEM_AUTHOR_GAP } else { POEM_LINE_GAP };
    }
    // The author's line: its own cells, right aligned at AUTHOR_RIGHT.
    let mut x = AUTHOR_RIGHT - poem.author_width(POEM[4])?;
    for b in POEM[4].bytes() {
        if b != b' ' {
            let (u, v) = strip.place(true, b)?;
            // Its stray texel sits one past the advance.
            let w = (poem.author_advance(b)? + 1).min(poem.author_w as i64) as usize;
            drawn.push(PoemGlyph { u, v, x, y, w, h: poem.author_h });
        }
        x += poem.author_advance(b)?;
    }
    ends.push(drawn.len());
    let total = drawn.len();
    let glyphs: String = drawn.iter().map(|g| format!("    ({}, {}, {}, {}),\n", g.u, g.v, g.x, g.y)).collect();
    let sizes: String = drawn.iter().map(|g| format!("{}, ", g.w | g.h << 16)).collect();
    // 4 bits a texel, 256 texels (64 halfwords) a row.
    let words: String = strip.words().iter().map(|w| format!("{w:#010x}, ")).collect();
    let word_count = strip.rows * 32;
    let (vram_x, vram_y) = STRIP_VRAM;
    let vram_rows = strip.rows;
    let steps = POEM_END_MS / ALPHA_STEP_MS + 2;
    let mut table = String::new();
    for curve in 0..KEYS.len() {
        table += &format!("    // {}\n    [", KEYS[curve].0);
        for step in 0..steps {
            let value = curve_at(curve, (step * ALPHA_STEP_MS) as f64 / 1000.0);
            table += &format!("{}, ", (value * 255.0).round() as u8);
        }
        table += "],\n";
    }
    Ok(format!(
        "pub const POEM_GLYPHS: [(u8, u8, i16, i16); {total}] = [\n{glyphs}];\n\
         /// Each glyph's rectangle: its width, and its height in the high halfword.\n\
         pub const POEM_GLYPH_SIZES: [u32; {total}] = [{sizes}];\n\
         /// Index in `POEM_GLYPHS` where each of the five lines ends.\n\
         pub const POEM_LINE_END: [usize; 5] = {ends:?};\n\
         pub const POEM_END_MS: u32 = {POEM_END_MS};\n\
         /// The poem's cells as one picture (the verse's, then the author's): the VRAM rectangle (halfwords) it is\n\
         /// uploaded to and its words (4 bits a texel, palette indices of the font's CLUT).\n\
         pub const POEM_STRIP_VRAM: (u16, u16, u16, u16) = ({vram_x}, {vram_y}, 64, {vram_rows});\n\
         pub const POEM_STRIP_WORDS: [u32; {word_count}] = [{words}];\n\
         pub const POEM_ALPHA_STEP_MS: u32 = {ALPHA_STEP_MS};\n\
         /// Brightness 0 to 255 of the four lines of the poem, the author's line and (last) the black cover, per step.\n\
         pub const POEM_ALPHA: [[u8; {steps}]; 6] = [\n{table}];\n"
    ))
}

/// Compile the intro and put it where the guest build reads it
/// (`data/intro.exe`).
pub fn compile(repository: &Path, psoxide: &Path) -> Result<PathBuf> {
    let intro = repository.join("intro");
    let read = |name: &str| std::fs::read_to_string(repository.join("data").join(name));
    std::fs::write(
        dir(repository).join("prompt.rs"),
        prompt_source(
            &read("font_layout.rs")?,
            &read("read_points.rs")?,
            &std::fs::read(repository.join("data/poem-font.hk"))?,
        )?,
    )?;
    let map = dir(repository).join("hk-intro.map");
    let mut command = Command::new("cargo");
    command
        .current_dir(&intro)
        .args(["build", "--release"])
        .env("PSOXIDE", psoxide)
        .env("HK_INTRO_LOAD_ADDR", format!("{LOAD_ADDR:#x}"))
        .env("HKPSX_LINK_MAP", &map);
    run_command(&mut command, "compile the hk-psx intro for PlayStation")?;
    let exe = intro.join("target/mipsel-sony-psx/release/hk-intro.exe");
    if !exe.is_file() {
        return Err(format!("the intro build did not produce {}", exe.display()).into());
    }
    // LLVM's delay-slot filler leaves a load in a slot here and there; the
    // SDK's patcher reroutes those branches through psx-rt's trampolines, and
    // the scanner then proves the image clean (as it does for the game).
    run_command(
        hazard_tool(psoxide, "hazard-patch")
            .arg(&exe)
            .arg("--map")
            .arg(&map),
        "patch load-delay hazards in hk-intro.exe",
    )?;
    run_command(
        hazard_tool(psoxide, "hazard-scan")
            .arg(&exe)
            .arg("--map")
            .arg(&map),
        "prove hk-intro.exe free of load-delay hazards",
    )?;
    let staged = repository.join("data/intro.exe");
    std::fs::copy(&exe, &staged)?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_are_named_for_mkisopsx_in_play_order() {
        assert_eq!(MOVIE_FILES, ["HKPOEM.XA", "HKINTRO1.STR", "HKINTRO2.STR"]);
    }

    #[test]
    fn the_prompt_is_laid_out_from_the_games_own_font_cells() {
        let layout = "pub const FONT_X: u16 = 320;\npub const FONT_PAGE_Y: u16 = 256;\n\
                      pub const FONT_V0: usize = 104;\npub const GLYPH_W: usize = 13;\n\
                      pub const GLYPH_H: usize = 14;\npub const GLYPH_PER_ROW: usize = 19;\n\
                      pub const GLYPH_ROW_PITCH: usize = 15;\npub const GLYPH_U0: usize = 4;\n";
        let mut advances = vec![5u8; 95];
        advances[0] = 3; // the space
        advances[(b'P' - 32) as usize] = 9;
        let points = format!(
            "pub const ADVANCES:[u8;95]=[{}];\n",
            advances.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
        );
        let poem = poem_blob();
        let source = prompt_source(layout, &points, &poem).unwrap();
        // 24 characters, 4 of them spaces.
        assert!(source.contains("pub const GLYPHS: [(u8, u8, i16); 20]"), "{source}");
        // `P` is cell 48: row 2, column 10; the second glyph starts after P's advance.
        assert!(source.contains("(134, 134, 0),"), "{source}");
        assert!(source.contains(", 9),"), "{source}");
        assert!(source.contains("WIDTH: i16 = "), "{source}");
        assert!(prompt_source("", &points, &poem).is_err());
        // The poem: every line is there, rows wrap under the picture's width and each line ends after the one before it.
        assert!(source.contains("POEM_LINE_END: [usize; 5]"), "{source}");
        assert!(source.contains("POEM_ALPHA: [[u8; 288]; 6]"), "{source}");
        // The poem's cells are one picture, uploaded in the movies' third buffer; the author's line is glyphs of its own.
        assert!(source.contains("POEM_STRIP_VRAM: (u16, u16, u16, u16) = (576, 256, 64, "), "{source}");
        assert!(source.contains("POEM_GLYPH_SIZES: [u32; "), "{source}");
    }

    /// A poem-font.hk of the real shape (13x17 verse cells, 11x14 author cells, 5 texel advances) whose cell
    /// `i` is filled with the palette index `i % 16`.
    fn poem_blob() -> Vec<u8> {
        let mut blob = vec![13, 17, 11, 14];
        blob.extend([5u8; 95]);
        for i in 0..95 {
            blob.extend(vec![(i % 16) as u8; 13 * 17]);
        }
        for i in 0..95 {
            blob.extend(vec![(i % 16) as u8; 11 * 14]);
        }
        blob
    }

    #[test]
    fn the_poems_cells_are_in_the_movies_third_buffer() {
        let buffers = include_str!("../../intro/src/buffers.rs");
        let (x, y) = STRIP_VRAM;
        assert!(buffers.contains(&format!("({x}, {y})];")), "{buffers}");
    }

    #[test]
    fn the_strip_holds_each_cell_once_and_the_descender_rows_with_it() {
        let font = PoemFont::read(&poem_blob()).unwrap();
        let strip = PoemStrip::new(&font, &["gag".to_string()], "ag");
        // Verse cells g and a, author cells a and g: four cells, a verse row of 17 and an author row of 14.
        assert_eq!(strip.cells.len(), 4);
        assert_eq!(strip.rows, 17 + 14);
        let (u, v) = strip.place(false, b'g').unwrap();
        // g is character 71: the verse's cell is filled with 71 % 16 down to its last (17th) row.
        assert_eq!(strip.texels[(v + 16) * 256 + u], 71 % 16);
        assert_eq!(strip.words().len(), strip.rows * 32);
        assert!(strip.place(false, b'z').is_err());
    }

    #[test]
    fn a_wrapped_line_breaks_near_its_middle_not_after_the_last_word_that_fits() {
        let font = Font { advances: vec![5; 95], u0: 0, w: 13, per_row: 19, v0: 0, pitch: 15 };
        // 5 px a character, a space included: 12 characters a row fit 60 px.
        let rows = wrap(&font, "one two three four five six", 100).unwrap();
        assert_eq!(rows, ["one two three", "four five six"]);
        assert_eq!(wrap(&font, "short", 100).unwrap(), ["short"]);
    }

    #[test]
    fn the_cooked_curves_hold_the_lines_back_until_their_keys_and_close_on_the_cover() {
        // Body 1 starts at 1.4333 s, its ramp reaches 1 at 5.8333 s; the cover opens by 5.9 s and shuts by 28.1 s.
        assert_eq!(curve_at(0, 1.0), 0.0);
        assert!((curve_at(0, 3.6333) - 0.5).abs() < 0.01);
        assert_eq!(curve_at(0, 6.0), 1.0);
        assert_eq!(curve_at(5, 1.0), 1.0);
        assert!(curve_at(5, 3.5) < 0.6);
        assert_eq!(curve_at(5, 10.0), 0.0);
        assert!((curve_at(5, 28.2) - 1.0).abs() < 1e-9);
        // The author's line is fully up by 19.6 s and stays.
        assert!((curve_at(4, 19.6) - 1.0).abs() < 0.01);
        assert_eq!(curve_at(4, 25.0), 1.0);
    }

    #[test]
    fn the_player_links_inside_the_empty_module_pool() {
        // intro/src/main.rs and game/src/intro.rs agree on this range: the pool is
        // 0x801DAF00..0x801F3F00 (host/code_modules.py) and the player's image,
        // header excluded, is under 64 KiB.
        assert!(LOAD_ADDR >= 0x801D_B000 && LOAD_ADDR + 0x10000 <= 0x801F_3F00);
    }
}
