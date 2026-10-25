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
pub const LOAD_ADDR: u32 = 0x801E_0000;
/// The cooker's exit status for "no intro on this machine".
const NO_INTRO: i32 = 3;
/// The movies, in play order, as raw-sector files for `mkisopsx --xa-file`.
pub const MOVIE_FILES: [&str; 2] = ["HKINTRO1.STR", "HKINTRO2.STR"];

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

/// The skip prompt laid out in the game's own font, which the game has in VRAM
/// when it starts the movies (the intro draws it with raw GP0 commands, no
/// font file of its own): one `(u, v, x)` row per visible glyph, as
/// game/src/dialogue.rs places them, and the string's width.
pub fn prompt_source(font_layout: &str, read_points: &str) -> Result<String> {
    let advances = const_bytes(read_points, "ADVANCES")?;
    let (u0, w, per_row) = (
        const_int(font_layout, "GLYPH_U0")?,
        const_int(font_layout, "GLYPH_W")?,
        const_int(font_layout, "GLYPH_PER_ROW")?,
    );
    let (v0, pitch) = (const_int(font_layout, "FONT_V0")?, const_int(font_layout, "GLYPH_ROW_PITCH")?);
    let mut rows = String::new();
    let (mut x, mut count) = (0i64, 0);
    for b in SKIP_PROMPT.bytes() {
        let i = i64::from(b) - 32;
        let advance = *advances
            .get(i as usize)
            .ok_or_else(|| format!("no advance for {:?}", b as char))?;
        if b != b' ' {
            let (u, v) = (u0 + i % per_row * w, v0 + i / per_row * pitch);
            rows += &format!("    ({u}, {v}, {x}),\n");
            count += 1;
        }
        x += i64::from(advance);
    }
    Ok(format!(
        "// Generated by hk-psx-build (host/hk-build/intro.rs) from data/font_layout.rs and\n\
         // data/read_points.rs.\n\
         pub const WIDTH: i16 = {x};\n\
         pub const FONT_X: u16 = {};\n\
         pub const FONT_PAGE_Y: u16 = {};\n\
         pub const GLYPH_W: u16 = {w};\n\
         pub const GLYPH_H: u16 = {};\n\
         pub const GLYPHS: [(u8, u8, i16); {count}] = [\n{rows}];\n",
        const_int(font_layout, "FONT_X")?,
        const_int(font_layout, "FONT_PAGE_Y")?,
        const_int(font_layout, "GLYPH_H")?,
    ))
}

/// Compile the intro and put it where the guest build reads it
/// (`data/intro.exe`).
pub fn compile(repository: &Path, psoxide: &Path) -> Result<PathBuf> {
    let intro = repository.join("intro");
    let read = |name: &str| std::fs::read_to_string(repository.join("data").join(name));
    std::fs::write(
        dir(repository).join("prompt.rs"),
        prompt_source(&read("font_layout.rs")?, &read("read_points.rs")?)?,
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
    fn the_movies_are_named_for_mkisopsx_in_play_order() {
        assert_eq!(MOVIE_FILES, ["HKINTRO1.STR", "HKINTRO2.STR"]);
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
        let source = prompt_source(layout, &points).unwrap();
        // 24 characters, 4 of them spaces.
        assert!(source.contains("GLYPHS: [(u8, u8, i16); 20]"), "{source}");
        // `P` is cell 48: row 2, column 10; the second glyph starts after P's advance.
        assert!(source.contains("(134, 134, 0),"), "{source}");
        assert!(source.contains(", 9),"), "{source}");
        assert!(source.contains("WIDTH: i16 = "), "{source}");
        assert!(prompt_source("", &points).is_err());
    }

    #[test]
    fn the_player_links_inside_the_empty_module_pool() {
        // intro/src/main.rs and game/src/intro.rs agree on this range: the pool is
        // 0x801DAF00..0x801F3F00 (host/code_modules.py) and the player's image,
        // header excluded, is under 64 KiB.
        assert!(LOAD_ADDR >= 0x801D_B000 && LOAD_ADDR + 0x10000 <= 0x801F_3F00);
    }
}
