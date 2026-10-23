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

`hk-cook title-style` composes a style's own sprites to 320x240 through the title camera
(`_GameCameras/CameraParent/tk2dCamera`: perspective, 24 degrees vertical; sprites far to near with their
colour and alpha; `UIBlendScreen` as screen blending, the default sprite material as alpha blending; the
camera's saturation) and cooks the logo as two 4bpp halves. Today the port carries Modern (the default) and
Voidheart, listed in `title_style::SHIPPED`, `cook_menu.py` `STYLES`, `build_guest.py` `BOOT_ART_PARTS` and
`menu_state::STYLE_NAMES`; the options page's "Menu style" row steps through them and the title fades through
black (0.25 s) around the change. `hk-cook title-style all` writes a preview of every style under
`.hkpsx/title-styles/`. The camera's bloom is not reproduced, and neither are the moving sprites or the
looping sources.

Adding a style: put its object name and logo sprite in `SHIPPED`, `STYLES` and `BOOT_ART_PARTS`, its name in
`STYLE_NAMES`, and give `menu.rs` its particles if it has any (`particles` is the Voidheart's). A style with
an unlock key also needs a profile-wide flag, which the port has no save slot for yet.

## What each further style would cost

Measured for the two carried styles: a baked background is 89,712 bytes of the boot art chunk (palette, two
8bpp pages and the loading prompts), a logo about 3.7 KB per half, and no linked RAM and no extra video memory
(one style is resident; the options page re-uploads). Everything else is an estimate from the components, not
a measurement:

- the five styles that are only glow, beams and vignette (Classic, HD, Grimm without its embers, Shade without
  its specks, Steel without its symbol) are one more baked background each, about 90 KB of disc;
- mote and ember systems (Classic, HD, Grimm, GG, Infected, Steel, Eternal, Modern's own) need the 29x39
  `default_mote` or `ember_particle` as a 16-level piece and the slot code the Voidheart's particles already
  use; the emitters of the carried Modern style lie below the picture as the scene poses them, so they draw
  nothing there;
- animated sprites (GG's disc, the infected moon, Eternal's waterfall and caustics) need their frames cooked
  as a flipbook and drawn over the baked layer;
- the sources' clips are assigned at run time and are not in the scene data, so their sound cost is not known.
