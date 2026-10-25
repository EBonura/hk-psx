# The opening cinematics

What the original plays on a new game, what the port plays, and what it costs.

## The original

`Opening_Sequence` (scene 5) is a `ChainSequence` of three parts: an `AnimatorSequence` ("Prologue Text":
the five-line excerpt from Monomon's elegy, lines fading in one after another over about 31 s, with the
piano clip `short_piano_for_opening_text` on the scene's Music source), then a `CinematicSequence` for each
of two movies, `Prologue` (906 frames at 30 fps) and `Intro` (953 frames at 25 fps), then the new game
(`GameManager.OnWillActivateFirstLevel` sets the entry gate to `top1`, which is the drop in King's Pass).

The two MP4s carry digital silence. Their sound is two separate AudioClips in `sharedassets5.assets`,
`Prologue_Cutscene_30fps AUDIO` (30.07 s) and `Intro_cinematic_v01 AUDIO` (38.0 s), started with the
video. The Intro's is quiet by design (-35.8 LUFS integrated; the Prologue's is -23.5).

The sequence is skippable through `InputHandler`'s `SKIP_PROMPT` mode: the first press of any button shows
`CinematicSkipPopup` ("Press any button to skip", language sheet `MainMenu` key `TEXT_SKIP_PROMPT`; fade in
0.15 s, hold 2 s, fade out 1 s), a second press while it shows skips the whole sequence.

## The port

The player is not part of the game (the game's image has no room for an MDEC player). `intro/` is a small
program carried in the boot art chunk. On a new game the title writes a mailbox at the top of RAM (profile and
the settings struct, `game/src/intro.rs`), copies the program into the room-module pool (empty at the title)
and jumps to it. The program streams `HKINTRO1.STR` and `HKINTRO2.STR` through the SDK's FMV player (320x176
letterboxed on 320x240, 15 fps, 15-bit, mono 37.8 kHz XA), shows the skip prompt in the letterbox bar with the
game's own font (glyphs and palette are already in VRAM; `hk-build` lays the string out from
`data/font_layout.rs` into `.hkpsx/intro/prompt.rs`), then has the SDK chain loader read the disc's `PSX.EXE`
(the game) back. The game boots, finds the mailbox and goes straight into the new game.

Cook: `hk-cook intro-audio` decodes the three clips with FMOD (the cooker every other clip goes through),
`hk-intro-cook --audio-dir` encodes the movies with `psxavenc`, masters each sound to the install's loudness
(+0.9 dB for the player's path, through a limiter) and interleaves it with the picture. Nothing the cook writes
is in the repository (`.hkpsx/intro/`). A machine without the install's clips or `psxavenc` builds the shipping
disc without the movies and says so.

Builds: the shipping disc (no guest feature) carries the movies by default (`--no-intro` leaves them out,
`--intro` insists on them). A test disc (any guest feature, such as `route-input`, whose tapes press New Game by
poll count) never has them. `cargo hk-build intro-cook` cooks the movies and compiles the program alone.

## Costs

- Disc: two movie files, 10,186 sectors (23.8 MB), after WORLD.PAK and before the XA songs.
- RAM in the game: free below the module pool 12,648 B against 12,628 B on the main it was cut from (the
  feature's resident code, 450 B, was found back in two `#[inline(never)]` marks: LLVM inlined
  `audio::set_volume`, `music::set_volume` and `music::apply` into `main` or not as the code around them moved).
- CD: the movies stream at double speed (150 sectors a second, 10 a frame); the game's read of the title
  stays as it was.
- Hand-off (60 ticks a second, emulator): the last frame, then the chain load of the game (about 4.9 s, black
  with the loader's thin progress bar), the game's own boot (about 2.6 s), the new game's scene load (about
  5.7 s, as a new game without the movies) and the drop.
- Frame time in play: unchanged; nothing of the player runs during play.

## Journeys

`tools/journeys/new-game-intro.toml` (shipping disc: movies, a first press shows the prompt, a second skips, the
drop lands) and `new-game-intro-full.toml` (both movies play out) are route-split journeys for `psoxide-gate`;
the others in that folder run on the route-input disc, except `new-game-hold-ignored.toml` which skips the movies.

## Not done

The poem and its piano (the first 31 s of the original's sequence) are not in the port: the piano would need a
16x16 black "movie" to carry it (about 5.5 MB) and the lines would be drawn by the program from the game's font.
