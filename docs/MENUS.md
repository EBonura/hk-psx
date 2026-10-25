# How the menus are built

The menus of this port (title, options, cheats, save slots, pause, charm board, dialogue boxes, area cards, the ten title styles)
copy the original's behaviour from the original's data, not from footage, and run on the console at one video period a frame with no
RAM cost for their art. This page is the recipe, for reuse in another game's menus: where the numbers come from, which parts are
generic, what each costs, and what the limits are. docs/MENU_STYLES.md is the title styles in detail; docs/SAVE.md the cards.

## The rules the menus keep

- **Real time.** Every duration is a count of video periods (`psx_rt::interrupts::vblank_count()`), never a count of frames drawn, so
  a slow frame stretches nothing. Animations are integer ramps `0..128` of the age of a page (`menu.rs levels`, `ramp`); the screen's
  brightness gain is a `u8` passed down to every draw, so a fade is one multiplication per piece.
- **Data from the original.** Durations, orders, easings and counts are read from the install: Unity's animation clips and animator
  controllers, PlayMaker state machines, serialized components and the disassembled coroutines of `UIManager`. Nothing is timed by eye.
- **No RAM for art.** Pictures, flipbooks, particle tracks and fonts are disc data uploaded to video memory (the boot art chunk, read
  before the title), or streamed by the animation cache; only code and a few constants are linked.
- **One video period a frame.** The emulator's GPU model gives a menu frame about 400,000 GPU cycles; the picture and text use about
  190,000. Costs are modelled in the cooker and trimmed there, never adaptively in the game.
- **Input is never held back** while a page animates (the original's is). The recorded input tapes press on their own timing and
  stay valid.
- **Integer only, no allocation, stateless.** Particles and flipbooks are functions of the vblank count and an index.

## The original's timings (all from game data)

| Element | What animates | Duration | Source |
| --- | --- | --- | --- |
| Screen, title and content groups | alpha 0 to 1 and back, linear | 0.19 s (`MENU_FADE_SPEED` 5 a second, serialized on the `UIManager`; the constructor's 3.2 is overridden) | `FadeInCanvasGroup`, `FadeOutCanvasGroup` |
| A page appearing | title at 0, top flourish 0.1 s, content and bottom flourish 0.2 s, input 0.3 s | about 0.5 s | `UIManager.ShowMenu` |
| A page leaving | title 0, top flourish 0.1 s, content 0.2 s, bottom flourish 0.3 s, then the screen group | about 0.5 s | `HideMenu`, `HideCurrentMenu` |
| Flourishes | flipbook `Warning_Fleur0000..0008`, reversed to hide | 9 frames at 30 a second | animator "Menu Animate In Out" |
| Cursor leaves | `main_menu_pointer_anim0000..0010` grows a mark into the leaf | 11 frames at 120 a second | animator "Menu Fleur" |
| Button press | `Button_flash0002..0007` over the button | 0.18 s | animator "Menu Flash Effect" |
| Save slots | slot flourish, four slots 0.165 s apart, then the controls 0.695 s after the last | 0.65 s show, 0.43 s hide | `GoToProfileMenu` |
| Pause | dimmer to alpha 0.8, crest, list, bottom flourish in `ShowMenu`'s order | 0.16 s dimmer | `GoToPauseMenu`, `UIClosePauseMenu` |
| Dialogue box | backboard alpha 0 to 0.6 (scale 0.75 to 1), typewriter | 0.5 s open, 0.3 s close; 65 characters a second | FSM "Box Open", `DialogueBox.revealSpeed` |
| Area card, first visit | 0.6 s wait, 4 s fade in, full to 5.35 s, 1.5 s fade out; NPCs inert until 7.35 s | | FSM "Area Title Control" |
| Area card, revisit | 1.5 s in, hold to 6 s, 1.5 s out | | same FSM |
| Charm board | pane fade 0.2 s, notch glow one-second cycle, icon flight 0.25 s ease-out, refusal jitter 0.5 s | | FSM "Inventory Control", "UI Charms" |
| Title style change | fade through black, `fadeTime` 0.25 s | | `MenuStyles.SetStyle` |

How to read one: serialized fields with the PPtrs resolved to object paths, an animation clip's streamed curves (a cubic per key,
`title_clips.rs` decodes them), an animator controller's states and transitions, a PlayMaker FSM's actions with their raw
parameters, and the CIL of the C# coroutines. The lab that did this (`font-lab`, built on `hk-unity`) lives with the lane's notes;
`hk-unity` (host/hk-unity) is the reader to start from, and `title_clips.rs`, `title_particles.rs` and `title_style.rs` are worked
examples of reading a clip, a particle system and a scene.

## The generic parts

- **Page sequence** (`menu.rs`: `levels`, `hide_frames`, `input_at`, the `View` with its `Levels`): a page that is left plays its
  leaving order, the chosen page plays its showing order, each piece fading by its own ramp. Reuse: give each piece a start and a
  length in vblanks and call `ramp(age, start, len)`.
- **Flourishes** (`flourishes`, `unfurl`): the finished flourish seen through a window that widens from the middle. The original swaps nine
  sprites; this keeps one in video memory (nine frames of every flourish would be 60 KB of linked art).
- **Cursor** (`pointer`, `leaf`): the original's opening frames at every second sprite frame, left leaf mirrored for the right.
- **Fonts** (`font_strip.rs`, `dialogue_art.rs`): Perpetua at 14 px grid-fitted and TrajanPro-Bold capitals, 16 coverage levels drawn
  additively through one shared palette; advances and layout constants are generated.
- **Dialogue boxes, pause, charm board, area cards** (`dialogue.rs`, `pause.rs`, `charms.rs`, `title_card.rs`): the same ramps over
  pieces cooked once; fades under additive text fade to nothing, not to black.
- **Title styles** (below).

## The title styles pipeline

1. **Scene to picture** (`hk-cook title-style`): the style's active sprites in world space, far to near, seen through the title camera
   (perspective, 24 degrees); glows and beams screen-blended, pictures alpha-blended; the style's colour correction (three channel
   curves, then saturation); 320x240; `cook_menu.py` quantizes to 256 colours with error diffusion onto the console's 5-bit palette.
2. **Particles** (`title_particles.rs`, `title_extra.rs`): every particle system at its true (rotated) pose; the steady-state population
   sampled, projected, culled to what crosses the view; four alternative tracks per slot; costed in GPU cycles and trimmed to the budget.
3. **Animated sprites**: an animator's clip becomes a table of samples (frame, strength, offset) for a quad; flipbook frames are packed
   in texture pages.
4. **Disc layout** (`build_guest.py boot_art`): common part first (what the boot reads), then one block per further style from a sector
   boundary, so a style is one disc read; checksums per block; the pack directory still sees one chunk.
5. **Runtime** (`menu.rs upload_style`, `extras`; `disc.rs stage_menu`): the style's blob is staged in the arena after the common part,
   its pieces uploaded, the draw lists walked each frame (`emitter`, `layer`, `piece`). Changing style stops the title's song for the read
   (the drive cannot read and play), fades through black and starts the song again.
6. **Unlocks** (`save.rs`): a 12-byte profile record on the card (docs/SAVE.md); the selector skips locked styles.

To add a style of another game's menus: pick the scene objects (sprites, particle systems, animators), list them in the style table,
run the cooker, read `.hkpsx/title-styles/report.txt` for slot counts and GPU costs, and look at `<style>_live.png`.

## Costs, measured

The ten styles on emulator 0e87b3a, route-input disc: every style presents one frame per video period.

| Style | Disc: picture + extras (B) | VRAM: picture + extras (B) | Particle slots | Quads a frame | Quad GPU cycles a frame |
| --- | ---: | ---: | ---: | ---: | ---: |
| Classic | 100,224 | 91,648 | 129 | 45 | 12,128 |
| HD | 98,984 | 97,792 | 15 | 14 | 114,089 |
| Grimm | 101,340 | 92,672 | 130 | 95 | 25,465 |
| GG | 105,004 | 97,792 | 102 | 77 | 20,561 |
| Infected | 130,540 | 124,800 | 76 | 64 | 128,653 |
| Shade | 117,380 | 105,984 | 172 | 160 | 43,047 |
| Steel | 98,560 | 91,648 | 103 | 89 | 23,801 |
| Eternal | 191,096 | 186,240 | 66 | 57 | 165,365 |
| Voidheart | 113,056 | 105,984 | 105 | 89 | 23,917 |
| Modern | 99,212 | 91,648 | 113 | 96 | 25,798 |

The logo adds two 4bpp halves of about 13.7 KB (disc, shared by nine styles; Voidheart's own) and the same in video memory at x 640 and 704. The whole
boot art chunk is 1,230,848 B (it was 248,420 B with two styles), of which the first 163,840 B are read at boot (the title was 248,420 B
before: the boot reads less and starts sooner). The picture and the text cost about 190,000 GPU cycles a frame and a frame fits one video
period up to about 400,000 together.

Linked RAM (code, bytes) for the whole system, against the round 7 build: see docs/BUDGET.md; the pictures, tracks and flipbooks are
disc data. The system's code is the particle and layer player (`extras`, `emitter`, `layer`, `piece`), the style read and upload
(`stage_menu`, `upload_style`), the selector, and the profile record.

## Limits and what is not reproduced

- The camera's bloom, the moon layer's one-pixel bob, flipbook frames of the flourishes (one frame with a window instead), the button
  press flash, the dialogue box's scale, the pause view's closing fade, the save indicator, the pre-menu logo cards.
- Style sounds: the original's `AudioSource`s have no clip in the scene data (docs/MENU_STYLES.md).
- A particle's depth sorting and rotation: billboards are drawn in emitter order, unrotated.
- Texture filtering: the console draws nearest-neighbour, so a texture magnified many times (the caustics, 64x64 over about 300 px) shows
  its texels.

## Where a console differs from the emulator

Costs above are the emulator's model of the GPU; the title frame budget (about 165,000 GPU cycles of quads with the picture and text) was
measured there. A style change reads the disc with the song stopped: on the console the drive's stop and restart cost about a second
each way (the fight music measures the same); the fade covers part of it.

## The title as a boot overlay

The title (`menu.rs`) runs once at boot and never comes back (the pause menu's reset does not return to it), so its code is
not linked into the resident image. It is the code module `title` (host/code_modules.py `MODULES`, `BOOT_MODULES`):

- **Chunk.** The cooked package is a chunk of kind `boot` at the head of the disc's boot group, right before the title art, so
  the read costs no seek (host/hk-build/disc.rs `disc_order`, game/src/disc.rs `member`).
- **Load.** `title::open` (game/src/title.rs) calls `Cache::title_code`, which stages the chunk in the arena behind the art's
  common part, checks it and installs it into the module pool (`modules::install_boot`). `main` does this before `menu_art`.
- **Calls.** Only the forwarders in title.rs call into the module (`5title` in the allowed callers). `skip_drop` and the statics
  (`STYLE`, `ART`, `EXTRA`, the `HK_MENU_*` probes) stay resident, as does `menu::state`, which the pause menu shares.
- **Evict.** `title::close` frees the module right after the loading frame, so the first scene has the whole pool. A failed scene
  load reopens it for the retry screen and closes it again. If the disc will not give the chunk, the game starts with fresh
  settings on slot 0.
- **Diagnostic builds.** `audio-probe` and `card-diag` call `menu::probe_screen` from outside the forwarders; build them with
  `HK_RESIDENT_MODULES=title`, which links the title resident again.
