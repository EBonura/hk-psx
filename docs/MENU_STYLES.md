# Title menu styles

The original's title screen is one scene (`Menu_Title`, level1) with a `Menu_Styles` object holding a child
per style. `MenuStyles.styles` lists ten of them; the options menu's `MenuStyleSetting` steps through the
available ones and `MenuStyles.SetStyle` fades the screen through black (`fadeTime` 0.25 s), switches the
style object, the camera's colour correction (saturation 0.6 to 0.92 per style), the music snapshot and the
logo sprite. Everything below is read from the install with the lab tools (`font-lab`: serialized fields,
the PlayMaker dumps and the CIL of `MenuStyles`, `MenuStyle`, `MenuStyleUnlock`, `MenuStyleUnlockAction`,
`MenuStyleSetting`, `MenuStyleTitle`).

## How a style is chosen and unlocked

- `MenuStyles.StyleDefault` is `styleIndex 9` (Modern) with `autoChangeVersion "1.5.0"`: a save last played by
  1.5.0 keeps the style it saved (`menuStyle` in the roaming shared data); anything older starts on Modern.
- `MenuStyle.IsAvailable`: a style with no `unlockKey` is available when its `enabled` flag is set (all ten
  are); a style with a key needs `RoamingSharedData[unlockKey] == 1`, or the `unlockAllMenuStyles` game
  config.
- Unlocking: `MenuStyleUnlock.Unlock(key)` sets that roaming key and `LocalSharedData.unlockedMenuStyle = key`;
  the next `LoadStyle` switches to the style with that key, so a new unlock shows itself on the next visit.
  `MenuStyles.UnlockFromAchievements` also unlocks a style when any of its `achievementKeys` is earned.
- Who sets a key: `MenuStyleUnlock` components named `Unlock Menu Style` in the three ending cinematics
  (`Cinematic_Ending_A` and `_B`: `infectedMenu`; `_C`: `shadeMenu`; the component also unlocks
  `steelSoulMenu` when the save is in Steel Soul mode), and a `MenuStyleUnlockAction` in the `GG_Mighty_Zote`
  fight (`eternalOrdealMenu`).

## The ten styles

| # | Style (object) | Available | What the scene draws | Particles | Sound |
| - | - | - | - | - | - |
| 0 | Classic (`Classic_Style`) | from the start | one blue-grey glow, vignette | 2 additive motes, 105 and 50 at 30 a second, 3 to 5 s | 2 looping sources |
| 1 | HD (`HD_Style`, holds a `Classic_Style (1)`) | from the start | purple glow, vignette | 3 soft additive dream particles (20, 10, 2), 4 to 8 s | 1 looping source |
| 2 | Grimm (`Grimm_Style`) | from the start | three red hazes, vignette | 300 embers (`ember_particle`, 3 to 6 s) and 2 mote systems | 2 looping sources |
| 3 | GG (`GG_Style`) | from the start | about 15 gold light beams, a half-transparent rotating disc (an Animator), three glows, vignette | motes (90, 90, 10) and 4 dream particles | 2 looping sources |
| 4 | Infected (`Infected_Style`) | key `infectedMenu`; achievements `ENDING_A`, `ENDING_B`; set in Cinematic_Ending_A and _B | the layered orange moon (an Animator on its first layer), vignette | wispy smoke (15) and 3 mote systems, plus a follow FSM | 2 looping sources |
| 5 | Shade (`Shade_Style`) | key `shadeMenu`; achievement `ENDING_C`; set in Cinematic_Ending_C | one grey glow, vignette | the abyss particles: 500 and 30 black specks, 46 background specks, 2 multiply smoke systems | 3 looping sources |
| 6 | Steel (`Steel_Style`) | key `steelSoulMenu`; achievement `STEELSOUL`; also set by an ending in Steel Soul mode | the steel soul symbol with a glow, 2 beams, vignette | 3 additive mote systems | 2 looping sources |
| 7 | Eternal (`Brilliant_Style`) | key `eternalOrdealMenu`; set in the GG_Mighty_Zote fight | Zote's statue, an animated waterfall and 2 animated caustics, 4 beams, glows, vignette | 3 additive mote systems | cave, water and a singing source driven by two FSMs; a different music snapshot |
| 8 | Voidheart (`VHeart_menu_Style`) | from the start | the Voidheart picture, 4 beams, vignette | the abyss particles (8 multiply smoke puffs, 32 foreground specks, 46 background specks) and 2 off-screen additive systems | 2 looping sources |
| 9 | Modern (`Modern_Style`) | from the start, and the 1.5.0 default | 4 glows (blues and violet), 3 light beams, vignette | 3 additive mote systems | 2 looping sources |

Voidheart is the only style with its own logo: `MenuStyleTitle.TitleSprites[0]` replaces the `title` sprite by
`vheart_title` ("Hollow Knight, Voidheart Edition", one sprite per language). The scene
also holds a `Radiant_Style` that no `styles` entry points at.

## In the port

All ten styles are carried, in the options' order (the original's `MenuStyles.styles`): Classic, HD, Grimm, GG,
Infected, Shade, Steel, Eternal, Voidheart, Modern. Modern, the original's 1.5.0 default, is what the title starts
on. The options row's Left, Right and Cross step through the styles that are available, wrapping, and the
title fades through black (0.25 s) around the change as `MenuStyles.SetStyle` does.

What is cooked, per style (`hk-cook title-style`, host/hk-cook/src/title_style.rs, title_extra.rs, title_particles.rs, title_clips.rs):

- **The picture**: the style's active sprites seen through `_GameCameras/CameraParent/tk2dCamera` (perspective, 24 degrees),
  far to near, the glows and light shafts screen-blended and the pictures alpha-blended, then the style's own colour
  correction (`cameraColorCorrection`: the red, green and blue curves, then the saturation about the luminance, as
  Unity's `ColorCorrectionCurves` does). The camera's bloom is not reproduced. `host/cook_menu.py` turns each into
  a 256-colour 320x240 picture, dithered onto the palette as the console will show it.
- **The logo**: `title` for nine styles and `vheart_title` for Voidheart (`MenuStyleTitle`), two 4bpp halves.
- **The particles**: every active `ParticleSystem` of the style is read (shape, life, start speed and size and colour,
  gravity, the velocity, size and colour over life, the UV sheet, the material's shader and tint, the renderer's
  `maxParticleSize`) at its true world pose, which is rotated: the emitters stand in front of the camera at all angles
  (an earlier inventory placed them below the picture by ignoring the rotation). The steady-state population is
  sampled and projected through the title camera; what never crosses the 320x240 view is dropped. Each remaining slot
  keeps four alternative tracks (a quadratic path on the screen, a size and a colour gain) and plays one per life, chosen by
  a hash of the life number: the guest does no simulation and no allocation (menu.rs `emitter`).
  Additive and soft-additive systems add, the plain sprite and multiply systems take away from the picture. Textures:
  `default_mote`, `ember_particle`, `particle_01`, `wispy_smoke_particle_abyss`, `wispy_smoke_particle_effect`,
  `dream_particle_01`, each cooked to 16 coverage levels and tinted at draw time.
- **The animated sprites** (the original's `Animator`s, read from their clips, `title_clips.rs`): GG's disc breathes by
  `GG_menu_disc_ebb` (its alpha only: the clip has no rotation), Eternal's waterfall is the six sprites of `waterfall`
  at 15 a second (8bpp with one shared palette) and its two caustics are every second sprite of the 48 of
  `caustic_small_0` (6 a second, the second copy half a loop later), the Infected moon's two dark tendrils sway by
  `infected_blob_anim`. Unity's streamed clip format is decoded (a cubic per key).
- **Not reproduced**: the moon layer's own one-pixel bob (the tendrils' motion is), the camera bloom, GG's `dream_particle_03`
  system (inactive in the scene), `Radiant_Style` (no `styles` entry), and the original's multiply smoke of Shade and
  Voidheart is cooked but comes out invisible: it rises from below the picture where the picture is black.
- **Sounds**: none. The `AudioSource`s under the style objects (two or three `Audio Player Actor 2D` per style, the three of
  Eternal's FSMs, `Singing`, cave and water) have `m_audioClip` unset in the scene and nothing in the data assigns
  one (`MenuStyles` only fades the sources' volumes; `initialAudioVolumes` is empty for all ten, and Eternal's FSM
  plays whatever the source holds). There is no loop to identify in the install, so none is ported. The
  title's song is the one the port already plays.

How a style gets to the screen (menu.rs, disc.rs): the default style (Modern) is part of the boot art chunk's first part
(`BOOT_COMMON_BYTES`, read before the title). The other nine follow it in the same chunk, each from a sector boundary
(host/build_guest.py `boot_art`), and are read when the options page picks one: the title's song is stopped for the
read (the drive cannot play and read), the arena after the common part receives the style's background and extras, its
pieces go up to video memory (one style resident), and the song starts again. A failed read leaves the style on screen.
The particle and animated sprites' pieces live in the title-only part of video memory: 4bpp pages at x 768 and 832,
an 8bpp page at x 896, rows 0 to 255, and a palette at (768, 479).

Drawing budget: the emulator's GPU model puts the title's frame inside one video period at about 400,000 GPU
cycles, of which the picture and the text take about 190,000. The cooker costs every slot (a quad about 230 cycles and
1.9 a pixel) and keeps the same share of each emitter's slots when a style would ask for more (HD and Infected). The
measured frames (route replay, 0e87b3a): every style presents one frame per video period.

## Unlocking

`MenuStyle.IsAvailable`: a style with an unlock key is available when the key is set in the profile's unlock mask. The mask is a
record on the memory card of its own (docs/SAVE.md, `save::unlocked_styles`), read with the profiles before the
title; `save::unlock_styles(mask)` sets keys and writes it. Nothing in the port calls it with a real ending's key yet (the
port has no endings or Mighty Zote fight); the Cheats page of the title has a last action row, "Unlock all menu styles"
(the original's `unlockAllMenuStyles`, as an action), which sets all four keys, writes the record and shows DONE: the
options row then steps through all ten. The keys: 1 `infectedMenu`, 2 `shadeMenu`, 4 `steelSoulMenu`, 8
`eternalOrdealMenu`.

## Costs

Per style (disc is the style's bytes of the boot art chunk: the 89,712-byte picture and the extras; VRAM is what the extras
upload on top of the picture's 89,600 and the logo's two halves): see the table in docs/MENUS.md.

Linked RAM: the style system costs code (the particle and layer player, the disc read of a style, the selector and the
unlock record), counted in docs/BUDGET.md; the pictures, tracks and flipbooks cost none.

## Adding or changing a style

The style list is `title_style::STYLES` (object name, options name, logo), `menu_state::STYLE_NAMES` and
`STYLE_KEY` (the guest checks the names against the cooker's at compile time), `cook_menu.py` `STYLES` and the
`menu-bg-N.hk` names `build_guest.py` expects. A style's look is the scene's: change the camera, the sprites
or a particle system there and `hk-cook title-style` follows. `HK_TITLE_STYLE_VERBOSE=1` lists the layers;
`.hkpsx/title-styles/` holds each style's picture, `<name>_live.png` with particles and layers at two moments, and
`report.txt` with slot counts and costs.
