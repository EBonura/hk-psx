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

The program is called three times (the mailbox's mode word), and the first scene loads in between. The first call
plays the poem with its piano and returns when the piano is over (about 19 s in): the piano is an XA stream, which
holds the drive for its whole length, so nothing else can be read before then. The game then loads the first scene
as far as the movies allow while the poem goes on (`disc::Cache::preload`, which runs the admission in its early
form: the sound banks, the scene's ambience clips and sound effects, the coverage, the effect art and every
atlas, none of it into the scene arena for good), and draws the poem's remaining nine seconds itself: the loader's
cooperative checkpoints (`disc.rs checkpoint`, the admission's only) call the program's step
(`hk_poem_tick`, entered through the same `hk_intro_enter` stub, the game's VBlank count in the mode word), which
shows the frame drawn last time, polls the pad and draws the next one into the buffers the first call used. The
poem's state between steps is the first 64 bytes of the hand-off block (`poem.rs Carry`); the program's image stays
in the pool the whole time, so nothing may be installed there until the movies are over (the early admission leaves
the scene's code chunk, which installs into the pool, for the real one). The second call plays the movies and puts
the screen back; a skip at any point goes to a call that only puts the screen back and leaves the admission to
finish what the early one did not (a skip before the piano is over loads the whole scene then, as before).

The movies' three display buffers (`intro/src/buffers.rs`, shared by the program and the game) are the game's own
two frame buffers and a third at x 576, y 256, which lies over static texture pages 13 to 17: the first scene has
pages 0 to 12, so nothing it uploads before the movies is drawn over (`disc::scene_clobbered` is asserted at build
time against the cooked atlas inventory; a cook that gave the first scene a page there would fail the build).
The first scene's CLUT rows (x 0..319, y 480..499) and the animation slots are off all three.

The movies' buffers (412 KB for the Prologue, the larger) are the scene arena, which nothing needs between the title and the
first scene's load; the program's own image and stack are in the pool. The program draws the poem, then streams
`HKINTRO1.STR` and `HKINTRO2.STR` through the SDK's FMV player (the 16:9 picture at 320x180 in a coded 320x192,
15 fps, 15-bit, mono 37.8 kHz XA; the player centres the 192 lines on a 236 line screen, so that the cook's 8 black
lines above the picture put it on lines 30 to 209 of the 240, see below), shows the skip prompt with the game's own font (glyphs and palette are already in VRAM;
`hk-build` lays the strings out from `data/font_layout.rs` into `.hkpsx/intro/prompt.rs`). The prompt's glyph rows are
213 to 226 (its ink 216 to 226), inside the title-safe area (the central 90%, rows 12 to 228) and under the picture,
which ends at row 209; over the poem it sits under the block, which ends at row 204. Each glyph is drawn as wide
as its advance and two texels, not the font's whole 13 texel cell (the `p` cell has a stray texel at its far edge,
a speck after the last letter).

The bars: the 180 line picture starts at line 8 of the coded 192, a whole 8x8 block, so the blocks above it decode
to exactly black (a few specks of 8 remain in lines 6 and 7, from the chroma). Its bottom edge is in the middle of
a block (180 lines are 22.5 blocks, whatever the offset): the four lines under it carry the decoder's ringing, a
texel or two of 8 (up to 16) a frame. While no prompt shows the intro ends the display window under the picture
(GP1 07h, line 210) so that those lines are the signal's black; with the prompt up the window reaches line 228.
The emulator's screenshots are the window: 320x210 (the picture on lines 30 to 209, 0 to 29 black), 320x228 with
the prompt.

The poem (`intro/src/poem.rs`): the five lines the original's `Prologue Text` shows, in the game's 14 px font,
the four verses wrapped to 288 px (a margin of 16 px each side, inside the title-safe area) and centred (language
sheet `General`, `PROLOGUE_EXCERPT_01` to `_04` and `_AUTHOR`; the author's curly quotes become ASCII). The
author's line is set smaller and right aligned in the scene: its text object has font size 3.85 where the verses'
is 5, and alignment 2 (top right) where theirs is 1 (top centre), in the same 20 unit box, white, alpha 1 like the
others. So it is a picture of its own, the line's font cells scaled by 0.77 with an area average
(`host/hk-build/intro.rs` `author_strip`), uploaded into the movies' third display buffer (at (512, 0), free
until the movies start) and drawn as one rectangle with the font's palette, ending at column 300. Each line fades in as in the original's animation (linear ramps from 1.43,
6.33, 9.77 and 13.5 s, the author's eased from 17.1 s), a black cover opens at 1.8 s (done by 5.9 s) and closes
over everything from 23.6 s (black at 28.1 s), and the sequence ends at 28.6 s. The curves are the clip's own
(`host/hk-build/intro.rs` `KEYS`), sampled at 100 ms. One assumption: the cover is in front of the text (its z is
nearer the camera), so the text dims with it. The block is anchored by its bottom (row 204, the text's last line) and grows upwards, as the original's scene
places the first line on the screen's centre and the author's line 3.79 units below it, with a pitch of 0.78 units
between lines (read from the install's `Opening_Sequence` scene; the font's size relative to the camera could not
be read, ours is a 14 px font), so a longer text never reaches the prompt. A frame is drawn every VBlank into one of two display buffers and
flipped at the next blank edge. The piano is `HKPOEM.XA`, a sound-only XA file (mono 37.8 kHz at single speed:
one sound sector in eight, filler between, a 2 s silent lead-in so that the first note lands at the original's
3.53 s whatever the drive's start takes), played by the SDK's XA player from 1.0 s on; no fake movie carries it.
A first press during the poem shows the skip prompt, a second one while it shows skips the poem and both movies.

The picture (host/hk-intro-cook `prepare_picture`): the MDEC's 15-bit output truncates each channel to 5 bits with
no dither of its own, so a smooth gradient came out as flat bands with contour rings around the lamps and
colour patches in dark skies. The cook scales the clip to 320x180 with an area average, corrects its tone, reduces
it to 5 bits per channel against a 16x16 ordered (Bayer) threshold, and pads it to 192 lines (8 above, 4 below)
before `psxavenc` sees it; the pattern survives the encode and the truncation then leaves a fine mesh instead of
bands. The matrix starts somewhere else for every picture (`dither::phase`, the R2 low-discrepancy sequence), so
no pixel keeps its threshold from one picture to the next: over a moving picture a mesh fixed to the screen
reads as a shower door, a mesh that changes at 15 pictures a second reads as grain. It costs nothing in the
arena (the busiest frame is within 2 KB of the fixed matrix's) and the temporal change of a flat patch goes from
about 1 to about 4 steps a picture, where the clip's own is 0.1 to 0.3. Tried and not taken: blue noise (the
busiest frame then needs about 424 KB of the arena's 415), ffmpeg's swscale dithers (they collapse into rows on
flat areas) and a lower amplitude where the picture is flat (the gradients there fall back into contour patches
with ragged edges, a block mean error of 0.4 rising to 1.0 steps at half amplitude, worse than the mesh).

Tone (`host/hk-intro-cook/src/transfer.rs`): the whole chain (5 bit reduction, encoder, MDEC, display) lifts midtones
and highlights (up to +14 steps at luma 150 to 256) and adds green, which came out as a teal cast and a clipped lamp
core. The tables there are measured: (value given to the encoder, value the PS1 shows) for each channel on a ramp
through the real disc, inverted and applied before the dither, calibrated twice (the second pass measures the first
pass's output). On all the review pairs the per-band channel means are within 0.7 steps of the clip's (0 to 256
luma, `review/movies/luma_bands_before_after.txt` in the lane's review folder).

Encoder budget and mux: the encoder gets a stereo sound parameter set so that it budgets the picture at 8.75 sectors
a frame (9 chunks at most; the mono default budgets 9.4, which makes the busiest frame bigger than the arena
holds), and `mux::interleave_paced` then lays each frame into exactly 10 slots (150 sectors a second), its last
sector in the last free slot, sound in every 16th slot from 31 slots in, filler elsewhere. Measured on the
emulator (SPU output against the clips' sound, display flips against the clips' frames, screenshots naming the
frame each flip shows) the sound is then within 2 ms of the picture; it was 49 ms early at 24 slots (the picture
runs a frame behind the clip, which the delay had not counted), steadily over both movies. Frame pacing is then even
and the player's memory is 411,552 B (Prologue) and 409,248 B (Intro), against the 415,000 B limit of the cook and
the arena's 415,052 B (the guest build asserts it; the cook refuses a movie over the limit). A 24-bit decode would
need 495 KB. The picture is one frame behind the clip (the encoder puts the first frame in twice). `ffmpeg` is a
cook dependency beside `psxavenc`; a machine without it builds without the poem and the movies.

Cook: `hk-cook intro-audio` decodes the three clips with FMOD (the cooker every other clip goes through),
`hk-intro-cook --audio-dir` encodes the movies with `psxavenc`, masters each sound to the install's loudness
(+0.9 dB for the player's path, through a limiter) and interleaves it with the picture, and does the same to the
piano for `HKPOEM.XA`. Nothing the cook writes is in the repository (`.hkpsx/intro/`). A machine without the
install's clips or `psxavenc` builds the shipping disc without the poem and the movies and says so.

Builds: the shipping disc (no guest feature) carries them by default (`--no-intro` leaves them out, `--intro`
insists on them). A test disc (any guest feature, such as `route-input`, whose tapes press New Game by poll count)
never has them. `cargo hk-build intro-cook` cooks them and compiles the player alone.

## Costs

- Disc: the poem's piano 1,344 sectors (3.1 MB, `HKPOEM.XA`) and the two movies 10,196 sectors (23.8 MB), after
  WORLD.PAK and before the XA songs: the disc is 95.8 MB (92.6 before the poem).
- RAM in the game: free below the module pool 9,472 B against 9,008 B on the main this was cut from (the
  program's call and return stub is 156 B and `intro::play` 372 bytes; the mailbox copy of the settings, `take` and the
  resume path in `main` are gone). The three `#[inline(never)]` marks on `audio::set_volume`, `music::set_volume`
  and `music::apply` stay: without them LLVM links the same tree 1,004 B larger.
- CD: the movies stream at double speed (150 sectors a second, 10 a frame); the piano plays at single speed (a
  sector in eight is sound) and the drive reads nothing else meanwhile; the game's read of the title stays as it was.
- Hand-off (60 ticks a second, emulator; `tools/journeys/new-game-handoff.toml`, which stops at `HK_GAME_MODE` 2, the
  game's admission of the first scene beginning once the last movie frame is gone, and at the Knight at the top of the
  shaft): 103 ticks (1.7 s). It was 335 (5.6 s) when the whole load came after the movies, 790 (13.2 s) when the program
  chain-loaded the game back (294 ticks of reload with a black screen and the loader's bar, 156 of boot and 340 of
  scene load). What is left after the movies, in order: the scene's code chunk (11 sectors, but a seek from the end
  of the movie file to the pack, about 22 ticks), the scene (88 sectors, 45 ticks at the drive's 150 a second with the
  seek), its decode (15), the world metadata (6) and the room's art chunk (4). The loader's work during the poem (the
  banks, the clips, the group's prefix: 313 sectors) takes about 4.2 s of the 9.2 s that are left after the piano.
- The scene cannot be loaded during the movies. They stream at the drive's full 150 sectors a second, so nothing else
  can be read; their buffers (412 KB for the Prologue) take the scene arena, which the scene needs (342 KB resident); the
  code installs into the pool where the program is; and VRAM has no spare 320x240 for a third buffer that spares
  every page of the scene.
- The poem draws at 60 frames a second (1,715 frames in its 28.6 s), 230 textured rectangles at most a frame. While
  the loader runs the frames come from its checkpoints, which are further apart in places (an SPU bank upload or a
  clip's check is one stretch of 6 to 10 VBlanks), but then every line is fully up and nothing moves (the last fade-in
  ends at 19.6 s, the cover starts closing at 23.6 s); the program prints the longest gap (`INTRO poem during the
  load`, 10 VBlanks in the emulator).
- Frame time in play: unchanged; nothing of the program runs during play.

Console risk (not tested, the emulator cannot show it): the program returns into a game whose interrupt wrapper,
DMA priorities and SPU it has used in between; `machine.rs` puts back what the emulator showed it changes and the
CD controller's flags are acknowledged before the game's handler sees them, but silicon has surprised this code
before (CD DMA priority, Pause timing). The vendored chain loader in `intro/vendor` (only `quiesce` is used now)
lacks the editor's CD stop fix until HK's SDK repin.

## Journeys

`tools/journeys/new-game-poem.toml` (the poem at 5, 12, 20 and 27 s, then the Prologue) and `new-game-poem-skip.toml`
(two presses during the poem) and `new-game-poem-skip-late.toml` (two presses at 22 s, while the first scene loads behind the poem) and
`new-game-handoff.toml` (the 1.7 s) run on the shipping disc as well; `new-game-intro.toml` (shipping disc: poem, movies, a first press shows the prompt, a second skips, the
drop lands) and `new-game-intro-full.toml` (both movies play out) are route-split journeys for `psoxide-gate`;
the others in that folder run on the route-input disc, except `new-game-hold-ignored.toml` which skips the movies.

## Not done

The poem's black cover is assumed to be in front of its text (it dims everything from 23.6 s); the clip's
sorting cannot be played back offline. The haze and light beams behind the original's text are not drawn: the
screen behind the poem is black. What remains after the last frame is 1.7 s. The ways to shorten it: the code, scene and metadata chunks
copied to a file placed right after `HKINTRO2.STR` (the first read then needs no long seek, and the three could be
read in one go), or the scene itself split so that its first view loads first (the cook's scene bank is one chunk).
