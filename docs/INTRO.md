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
program carried in the boot art chunk. On a new game the game copies it into the room-module pool (empty at the
title), writes a mailbox at the top of RAM and *calls* it (`game/src/intro.rs`: `hk_intro_enter` saves the
callee-saved registers on the game's own stack and switches to the program's stack, the top of the pool).
Nothing of the game leaves RAM: its image, data and the main loop's stack stay where they are. The program
takes the hardware state it is about to change (`intro/src/machine.rs`: COP0 status, the interrupt mask, the
DMA priority register, the exception vector, the root counters' modes, the SPU's volumes and control register),
puts the machine where the BIOS leaves it (the SDK chain loader's `quiesce`), plays, puts the state back, and
jumps to `hk_intro_leave`, which returns into the game. The game starts its frame loop over (`gpu::init`, a fresh
frame buffer, black display buffers) and goes on into the new game with the title's choices, the card's slots
and the audio it already loaded: there is no reload of `PSX.EXE` and no second boot. The SPU's voices and RAM are
left alone (the program never resets the SPU), so the sound effects stay loaded.

The movies' buffers (374 KB for the Prologue) are the scene arena, which nothing needs between the title and the
first scene's load; the program's own image and stack are in the pool. The program draws the poem, then streams
`HKINTRO1.STR` and `HKINTRO2.STR` through the SDK's FMV player (320x176 letterboxed on 320x240, 15 fps, 15-bit,
mono 37.8 kHz XA), shows the skip prompt with the game's own font (glyphs and palette are already in VRAM;
`hk-build` lays the strings out from `data/font_layout.rs` into `.hkpsx/intro/prompt.rs`).

The poem (`intro/src/poem.rs`): the five lines the original's `Prologue Text` shows, in the game's 14 px font,
wrapped to 300 px and centred (language sheet `General`, `PROLOGUE_EXCERPT_01` to `_04` and `_AUTHOR`; the
author's curly quotes become ASCII). Each line fades in as in the original's animation (linear ramps from 1.43,
6.33, 9.77 and 13.5 s, the author's eased from 17.1 s), a black cover opens at 1.8 s (done by 5.9 s) and closes
over everything from 23.6 s (black at 28.1 s), and the sequence ends at 28.6 s. The curves are the clip's own
(`host/hk-build/intro.rs` `KEYS`), sampled at 100 ms. One assumption: the cover is in front of the text (its z is
nearer the camera), so the text dims with it. A frame is drawn every VBlank into one of two display buffers and
flipped at the next blank edge. The piano is `HKPOEM.XA`, a sound-only XA file (mono 37.8 kHz at single speed:
one sound sector in eight, filler between, a 2 s silent lead-in so that the first note lands at the original's
3.53 s whatever the drive's start takes), played by the SDK's XA player from 1.0 s on; no fake movie carries it.
A first press during the poem shows the skip prompt, a second one while it shows skips the poem and both movies.

Cook: `hk-cook intro-audio` decodes the three clips with FMOD (the cooker every other clip goes through),
`hk-intro-cook --audio-dir` encodes the movies with `psxavenc`, masters each sound to the install's loudness
(+0.9 dB for the player's path, through a limiter) and interleaves it with the picture, and does the same to the
piano for `HKPOEM.XA`. Nothing the cook writes is in the repository (`.hkpsx/intro/`). A machine without the
install's clips or `psxavenc` builds the shipping disc without the poem and the movies and says so.

Builds: the shipping disc (no guest feature) carries them by default (`--no-intro` leaves them out, `--intro`
insists on them). A test disc (any guest feature, such as `route-input`, whose tapes press New Game by poll count)
never has them. `cargo hk-build intro-cook` cooks them and compiles the player alone.

## Costs

- Disc: the poem's piano 1,344 sectors (3.1 MB, `HKPOEM.XA`) and the two movies 10,186 sectors (23.8 MB), after
  WORLD.PAK and before the XA songs: the disc is 95.8 MB (92.6 before the poem).
- RAM in the game: free below the module pool 13,096 B against 12,648 B on the main this was cut from (the
  program's call and return stub is 156 B and `intro::play` 372; the mailbox copy of the settings, `take` and the
  resume path in `main` are gone). The three `#[inline(never)]` marks on `audio::set_volume`, `music::set_volume`
  and `music::apply` stay: without them LLVM links the same tree 1,004 B larger.
- CD: the movies stream at double speed (150 sectors a second, 10 a frame); the piano plays at single speed (a
  sector in eight is sound) and the drive reads nothing else meanwhile; the game's read of the title stays as it was.
- Hand-off (60 ticks a second, emulator; `tools/journeys/new-game-handoff.toml`): from the last movie frame to the
  Knight at the top of the shaft 335 ticks (5.6 s), which is the new game's scene load. It was 790 ticks (13.2 s)
  when the program chain-loaded the game back: 294 ticks of reload with a black screen and the loader's bar, 156 of
  boot and the same 340 of scene load. The reload and the boot are gone because nothing of the game was ever
  overwritten.
- The scene load cannot be hidden under the movie. The Intro streams at the drive's full 150 sectors a second, so
  nothing else can be read during it; its buffers (374 KB for the Prologue) take the scene arena, which the scene
  needs (322 KB resident for Tutorial_01, which also stages its 18 pages through it); and VRAM has no spare 320x240
  for a third display buffer beside the scene's pages. What the load costs is the data (about 1.7 MB at 150
  sectors a second) and it starts as soon as the last frame is gone.
- The poem draws at 60 frames a second (1,715 frames in its 28.6 s), 230 textured rectangles at most a frame.
- Frame time in play: unchanged; nothing of the program runs during play.

Console risk (not tested, the emulator cannot show it): the program returns into a game whose interrupt wrapper,
DMA priorities and SPU it has used in between; `machine.rs` puts back what the emulator showed it changes and the
CD controller's flags are acknowledged before the game's handler sees them, but silicon has surprised this code
before (CD DMA priority, Pause timing). The vendored chain loader in `intro/vendor` (only `quiesce` is used now)
lacks the editor's CD stop fix until HK's SDK repin.

## Journeys

`tools/journeys/new-game-poem.toml` (the poem at 5, 12, 20 and 27 s, then the Prologue) and `new-game-poem-skip.toml`
(two presses during the poem) and `new-game-handoff.toml` (the 5.6 s) run on the shipping disc as well; `new-game-intro.toml` (shipping disc: poem, movies, a first press shows the prompt, a second skips, the
drop lands) and `new-game-intro-full.toml` (both movies play out) are route-split journeys for `psoxide-gate`;
the others in that folder run on the route-input disc, except `new-game-hold-ignored.toml` which skips the movies.

## Not done

The poem's black cover is assumed to be in front of its text (it dims everything from 23.6 s); the clip's
sorting cannot be played back offline. The haze and light beams behind the original's text are not drawn: the
screen behind the poem is black. The scene load after the last frame is the 5.6 s above; a way to shorten it
is the game's own loader (a smaller first scene or a faster stage), not the movie program.
