# Saving on a console

The bench Save writes the port-1 memory card (`game/src/save.rs`, two files per profile).
This page is what the player sees, what each failure means, how the card layout is checked,
and what to look at when a real console disagrees with the emulator.

## What the player sees

| Moment | Screen |
| --- | --- |
| Sitting at a bench | `Save game?` Yes / No, in the conversation box (the same fleurs and backboard as Elderbug's) |
| Yes | `Saving...` / `Do not remove the memory card.` The box is drawn for several frames before the write starts, because the write holds the CPU for longer than a video period and the frame on screen is the last one drawn until it ends |
| Done | `Game saved` for 1.5 s |
| Card not formatted | `Memory card is not formatted.` / `Format it and save?` Yes / No. The card is never formatted without an answer; a blank card used to be formatted silently by the first save |
| Anything else | `Save failed`, the reason, what to do, a `Code` number and the box's stop mark (X or O closes it). It stays until closed |

Reasons: no memory card in slot 1; card not responding (it answered and broke protocol); could not write
to the card; card full (free a block); card data damaged; unformatted (the question above).

`Code` is `fault * 10000 + hardware * 1000 + exchange`: `fault` is `save::Fault` (0 no card, 1 full, 2 damaged,
3 unformatted, 4 other, 5 not responding, 6 write error), `hardware` is `HK_CARD_FAULT` (1 TX never ready, 2 RX
never filled, 3 `/ACK` never came, 4 `/ACK` never released) and `exchange` is the byte in the frame
transaction where it happened (the response bytes of a read are numbered from 0 at the `0x81` select).
`SELECT SAVE` on the title shows the same reasons for a missing or unformatted card.

Card behaviour that changed with this page: three retries per frame (two video periods apart) for a protocol
error, and for an absent card on reads (the first frame after a chain-load is the one that finds SIO0 as the
launcher left it); when a replacement does not fit, the older copy of that profile is deleted first and the
newer one stays; the SC title frame is written in double-width Shift-JIS and the icon is the Knight's mask.

## The profile record (menu style unlocks)

Besides the profiles, the card holds one small file that no slot owns, `BASLUS-00000HKPSXMS`, the port's
counterpart of the original's roaming shared data. It carries the title style unlocks: one bit each for
`infectedMenu` (1), `shadeMenu` (2), `steelSoulMenu` (4) and `eternalOrdealMenu` (8), which the endings
and the Mighty Zote fight set in the original (`MenuStyleUnlock`, `MenuStyleUnlockAction`;
docs/MENU_STYLES.md). Twelve bytes: `HKU1`, the mask, three zero bytes, and the FNV-1a of those eight. It uses one
card block (the profiles' eight, so nine of fifteen), with the same title and icon as the saves. A missing, damaged or
unreadable record reads as nothing unlocked, and a card that is missing, unformatted (never formatted
unasked) or full leaves the keys held for the session only.

- `save::survey` reads it with the profiles, before the title; the title's options row skips the styles whose key is
  clear, exactly as `MenuStyle.IsAvailable` does.
- `save::unlock_styles(mask)` sets keys and writes the record: the Cheats page's last action (`Unlock all menu
  styles`, the port's `unlockAllMenuStyles`) calls it with all four, and the endings and the fight will call it
  with theirs once the port has them. The selector sees the styles at once, and so does the next boot.
- A card that holds only an older save set (no record) is untouched until something unlocks; the layout check
  (`card-check`) lists the record as one more block of the same title.

## Layout check

PSoXide cannot run a BIOS (its HLE BIOS has no card manager), so the console's listing is checked against
the format rules instead: `cargo hk-build card-check CARD.mcd...` prints each card as a manager would
list it and exits 1 on anything the manager would refuse (`host/hk-build/card_check.rs`, with tests over every
committed fixture). It needs a real console to prove the manager actually draws the title and icon.

## Telemetry

`HK_SAVE_UI` (0 idle, 1 ask, 2 format question, 3 saving, 4 saved, 5 failed), `HK_SAVE_WRITES`, `HK_SAVE_ERRORS`,
`HK_SAVE_FAULT` (fault + 1), `HK_CARD_FAULT`, `HK_CARD_FAULT_EXCHANGE`, `HK_CARD_RETRIES`, `HK_CARD_FRAMES`,
`HK_CARD_LAST` (bit 16 set for a write, frame in the low bits) and `HK_CARD_WRITE_VBLANKS` (video periods the last
save held the CPU for: the number that says what a real card costs).

## Diagnostic disc

`HK_GUEST_FEATURES=card-diag cargo hk-build build --library DIR` builds a disc with `game/src/card_diag.rs`:
a CARD DIAG page after the title and two lines at the bottom of every frame. Never ship it, and never install it
over the library copy. The page shows the boot-time first card transaction, a fresh read of frame 0, the
`MC` header, and a write-back of frame 63 (the BIOS's own write-test frame, written with the bytes it already
holds) with a timing sweep if it fails. One photograph of that page says whether a console card answers, whether
a write is accepted, how long it takes in video periods and which timing works.
