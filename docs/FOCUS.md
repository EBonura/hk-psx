# Focus and resident effects

The no-charm healing subset is derived from the Windows HeroController and
Spell Control FSM. Hold Circle while grounded with at least33 SOUL to focus.
The entry sequence waits0.25 seconds for a held input, then0.25 seconds for
Focus Start. The drain consumes one SOUL every0.027 seconds; completing33
charges restores one mask. The guest retains the fractional drain phase at60Hz
rather than rounding every charge to two simulation ticks.

Releasing the button or leaving the floor during the initial0.2-second drain
grace period restores the SOUL present at that cycle's start. Later cancellation
keeps the spent SOUL. Repeated healing cycles use the source0.45-second grace
window and a0.2-second heal state; finishing waits another0.23 seconds, while
the cancel animation lasts0.25 seconds. Accepted damage interrupts immediately without
refunding; scene changes and resets also stop focus. Starting focus resets nail
attacks and motion. Movement, jumping and new attacks are locked during its
active animation phases. The observed attack recovery gate is0.1 seconds.

The original entry state does not reject full health. Consequently one complete
cycle can consume33 SOUL at full health before the finish state stops repetition.
This preserves the observed FSM condition. It is not an inferred convenience
rule. Holding the button after an interruption does not silently restart focus.

The cooker appends Focus, Focus Get, Focus End and Focus Get Once after the
existing Knight/nail clips. The original timing, frame pivots and wrap modes
remain cooked data. `host/focus.py` validates the relevant source action fields
and records source file/method hashes in ignored `.hkpsx/focus-source.json`.
The native tests cover progressive charging, repeats, insufficient SOUL,
airborne/attack gates, early and late cancellation, damage and full health.
Original PlayMaker/Unity event ordering is resampled to a deterministic60Hz;
no original-input timing capture has yet established frame-exact parity.

The six resident effects are door break, accepted jump, landing, nail swing,
accepted damage and enemy nail impact. They occupy31,984 bytes at SPU
`0x1010..0x8D00` and use separate voices0..5. Most play at22,050Hz mono; the hurt
sample uses11,025Hz mono to keep this bank below32KiB. Original door bytes are
preserved. Events cause no CD reads or texture uploads. The cooker records
source references, hashes, resampling and decode checks in
`.hkpsx/audio-provenance.json` and `.hkpsx/audio-verification.json`.

The full original death layers are cooked separately under `.hkpsx/audio-extra/`
and are not yet played by the guest. Focus particles/sounds, spells, charms,
SOUL reserves, Geo and full death effects remain incomplete. Music/ambience
source preparation and pending transport are described in MUSIC.md.

## Final-disc validation

`tools/validate_focus.py --output captures/focus-earned-final` uses the
source-earned encounter fixture for the current streaming build. It stops in
the two Crawler encounter windows, then holds Circle. Four hits earn44 SOUL;
Focus drains33, restores health4 to5 and leaves11. Charging/healed states and
software/native hardware-renderer captures are bound to the final build in
`captures/focus-earned-final/report.json`.

The older ENEMY_ROUTE/FOCUS_ROUTE constants remain unchanged for performance
A/B. They walk farther after loading holds disappear, so their traversal pass
is separate from the EARNED_COMBAT_ROUTE/EARNED_FOCUS_ROUTE gameplay proof.
Both final variants also pass strict continuity checks on the earned-heal input.
