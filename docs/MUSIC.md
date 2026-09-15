# Source ambience and CD-streamed music

## Title playback (build65,2026-09-15)

The title plays its complete81.8-second source track through SDK CD-DA. The
Windows48kHz stereo source is resampled to44,100Hz stereo PCM16;14,429,520 bytes
fill6135 sectors exactly, with no truncated audio or zero padding. The normal
build adds audio track02 after the data track with the SDK's150-sector pregap,
in the same canonical BIN/CUE pair. `host/title_music.py` validates the source
cue/channel, delay, loop, source hashes and mixer base gain; ignored provenance
is `.hkpsx/title-music.json`. Original managed calls confirm volume1 at Play;
serialized volume0 describes the inactive source. Original mixer effects are
not reproduced. The CD input uses one-third Q15 headroom plus the existing
master gain, and an independent0..10 Music control in title and pause menus.

`game/src/music.rs` uses the pinned SDK CddaStarter/CddaEndDetector, automatic
track-boundary pause and restart. It waits the source1s before handshake, then
tracks actual playing status. No music sample bank, SPU voice, refill buffer or
music IRQ owner is allocated. Starting the game fades CD input and requires
`try_pause_until_complete` before any data read; an accepted Stop is insufficient.
Title polling owns the drive until this completed handoff. Boss music can use
this transport after its whole arena is loaded; arbitrary traversal music needs
separate read arbitration. Bosses/music are not implemented yet.

Actual-CUE10500-poll validation crosses two track repeats without faults. Three
captured stereo phrases correlate above0.9999989 with cooked PCM after gain and
alignment, with under0.84 PCM-unit RMS residual. Emulator repeat gaps measure
0.500s and0.473s; no gapless or silicon timing claim is made. Active-title mute/
restore and handoff, plus early Start before the handshake, reach gameplay with
correct cave ambience and no reported underruns or missed polls. Gameplay and
music/pause UI captures were inspected. Full suite:266 Python and392 reported
native tests; final EXE hazards0. Final build identity is in STATUS.md.

Reproduce with `tools.validate.poll_tape`:10500 neutral polls for the long title;
`9:start:1,170:right:40` over300 polls for early Start; over850 polls use
`9:down:1,12:cross:1,15:down:1,18:down:1,21:left:70,300:right:70,390:circle:1,397:up:1,400:start:1,550:right:100,680:cross:12,750:start:1,753:down:1,756:down:1,759:down:1`
for mute/restore, active handoff and the pause Music row. Replay each with
`python3 tools/replay_cue.py --tape TAPE --output OUTPUT`. Then:

```sh
.venv/bin/python tools/check_title_capture.py .hkpsx/music65/cdda-title-loop --output .hkpsx/music65/cdda-pcm-check.json
```

Evidence lives in `.hkpsx/music65/cdda-{title-loop,handoff2,early}/`, plus
`cdda-tests.log`, `cdda-build.log`, and `cdda-pcm-check.json`. Retail PCM/captures
stay ignored. Main RAM now has61,356B free before the protected48KiB stack;
136,704B contiguous SPU remains available. Tutorial's Silent snapshot and the
uncooked Town music trigger are unchanged. Next restore source-traced Focus and
missing effects; historical capacity experiments below are not current allocation.

## Ambience milestone (build64,2026-09-15)

Source-derived player running and hard-landing sounds are integrated, along
with the Great Door's first hit variant/pitch range. Running uses the original
complete nonlooping rhythm sequence: stop outside running, restart after clip
completion, and defer a new start while soft landing plays. At build64 the port still lacked
music, Focus, many enemy/break/death/UI effects and original hard-land recovery.

All six full ambience payloads are retained. Cave noises (199,872B) now live in
main RAM rather than occupying their full length in SPU. A continuous16KiB SPU
ring plus8KiB aligned refill scratch uses hardware playback IRQ boundaries,
polled from real-time input checkpoints even during simulation pause. DMA occurs
only into the inactive half with IRQ detection temporarily disabled; detection
is rearmed for the next playback boundary afterward. ENDX is read-only and is
not used. There is no playback-time CD traffic. Late/missing/early boundaries
stop the streamed voice and increment HK_AUDIO_STREAM_UNDERRUNS.

Allocation: player SFX0x1010..0x107A0 (63,376B), Geo0x14000..0x17650
(13,904B), ambience0x18000..0x5EA00 (289,280B). The contiguous136,704B above
ambience is available for a future128KiB music ring. Main RAM still has63,448B
before the reserved stack. This does not implement music transport or reserve
reverb workspace; future CD-read arbitration and stereo refill deadlines need
separate validation.

Quality categories:22,050Hz mono player one-shots;11,025Hz mono longer running
sequence;11,025Hz Geo;8,000Hz mono ambience. Existing ambience quality is
unchanged. Hurt was raised from11,025 to22,050Hz to match its short-effect
category. The user permits lower long-sample rates when necessary, consistently
within categories. Music profiles below remain preparation only.

Actual-CUE stationary/traversal runs cross two full source loops without
underruns or missed input polls. A separate100-second capture verifies resident
bytes, both ring halves and the full RAM source. After startup alignment,
90.7666 seconds of stereo ambience output match the pre-streaming build63 exactly.
Tests cover PAL/NTSC boundary schedules, delayed/missing IRQs, incomplete DMA,
source wrap away from ring boundaries and identical decoded PCM. Silicon timing
is still untested. Evidence: `.hkpsx/audio64/` and `.hkpsx/audio-stream-tests/`.

The original runner now records playback/stop/mixer requests at217 managed
call sites. See ORIGINAL_REFERENCE.md for coverage limits and commands. Requests
and sampled isPlaying still do not prove final audibility; NullGfx permits audio
but the original's mixed waveform has not been captured. Traced movement confirms
stone footsteps, jump, soft landing, grass movement, water drips and hits.

Reproduce the PCM check with:

```sh
.venv/bin/python tools/compare_audio_capture.py --before .hkpsx/audio64/baseline/audio.wav --after .hkpsx/audio64/stream/audio.wav --offset 930 --output .hkpsx/audio64/wave-comparison.json
python3 tools/validate_ambience.py --output .hkpsx/audio64/ambience --verify-existing
```

Build64 follow-up was music transport; build65 implements Title with CD-DA above.
Correctly triggered Dirtmouth remains pending. Tutorial retains its
Silent snapshot. Continue with source-traced Focus, death/break layers, enemies
and UI. Current Town music trigger remains beyond cooked coverage.

## What the current rooms actually request

Tutorial_01's SceneManager (`level6:12820`) has no music cue and selects the
**Silent** music snapshot. Adding Dirtmouth music to the opening would contradict
this source configuration. Its **Cave** atmosphere cue enables channels 0, 4, 5
and 6: cave wind, cave noises, indoor rain and outdoor rain. In the selected
`at Cave` mixer snapshot, wind/noises have 0 dB internal gain and both rain stems
have −60.40684 dB. The quiet rain remains part of the extracted dependency set.

Town's SceneManager (`level7:4397`) has no direct music cue or music snapshot.
Its **Surface** atmosphere enables channels 1, 3, 5 and 6: two surface winds and
the same rain stems. The selected `at Surface` snapshot gives the two winds
0 dB and −2.28898 dB; both rain gains are −60.40684 dB. These gains include the
selected group and its parent chain, not downstream mixers or player settings.
The Atmos mixer has no serialized effects. Disabled wind/noise groups target
−80 dB before their voices stop. Both SceneManagers specify a 0.5-second atmosphere
transition. The guest uses these endpoints over 30 ticks, with a fixed linear
SPU-volume ramp; Unity's native mixer interpolation curve is not reproduced.

The active Town MusicRegion (`level7:4080`, collider `level7:2502`) occupies
x=95.59564..166.40063, y=5.88058..38.38046. Current Town geometry ends at x=48,
so this trigger is outside current playable coverage. It selects the **Dirtmouth**
cue (`sharedassets7.assets:639`), whose only populated channel is channel 0,
**Dirtmouth 1** (`sharedassets7.assets:93`). The `nymmInTown` PlayerData condition
selects **DirtmouthAccordion** instead, using `sharedassets7.assets:117`.

Serialized data and inspected Assembly-CSharp methods agree on these details:

- MusicRegion accepts Hero layer 9. Its first Dirtmouth cue fades in over 1 second;
  an already-current Dirtmouth cue uses the serialized 3 seconds. Exit selects
  Silent over 6 seconds.
- MusicCue resolves its conditional alternatives before comparing cue identity.
- ApplyAtmosCue starts enabled channels only if they are not already playing,
  applies the mixer transition, and stops disabled channels after that transition.
  Shared rain should therefore retain playback phase across Cave/Surface changes.
- All six referenced ambient AudioSources loop, play on awake, have volume/pitch
  1, and are nonspatial. In this Unity version their direct clip reference is
  `m_Resource`; the legacy `m_audioClip` field is null.

These are bounded source observations, not a general implementation of the game's
music state machine. The ignored provenance report contains source IDs, polygons,
cue alternatives, mixer fields and the relevant method dump hash.

## Reproduction and provenance

```sh
.venv/bin/python host/ambience.py
.venv/bin/python -m unittest discover -s tests -p 'test_music.py'
.venv/bin/python -m unittest discover -s tests -p 'test_ambience.py'
```

The normal build runs `host/ambience.py` after cooking the effects bank. It
regenerates music conversions automatically when source selection, source hashes,
conversion code or required output files change. `host/cook_music.py` remains the
standalone full-profile experiment. Production outputs are the descriptor-only
`data/ambience.rs`, six raw `data/ambience/clip_*.adpcm` files and ignored
`.hkpsx/ambience.json` provenance. Current WORLD.PAK chunks 1..98 are rooms and
99..104 are ambience. Direct guest builds verify the descriptor and effects-bank
hashes as well as each ambient payload.

The cooker reads only the Windows installation selected by `Source`. It records
143 source/assembly/resource file hashes, serialized clip and encoded-resource
hashes, the decoded WAV hash, conversion tool version, valid sample counts,
output hashes and encoder source hashes. Cached WAV data is reused only when
both source identity and decoded-file hash match. Resource traversal and
truncation fail explicitly.

There are eight unique clips and 28 profile conversions: 22,050 Hz stereo and
11,025 Hz mono for every clip, plus 10,000 and 8,000 Hz mono for the six ambience
clips. Complete clips are preserved; lower rates and mono are explicit quality
experiments. No silence replacement or length truncation is used. Each channel
is a separate PSX ADPCM plane, with at most 27 zero samples in its final block;
valid frame counts are retained separately.

The original host encoder tests all five predictors and shifts 0..12 per block.
Its initial block uses filter 0 to define decoder history. Experimental payloads
have zero flags; production packaging installs loop-start flag 4 in the first
block and end-plus-repeat flag 3 in the last, preserving every sample nibble.
Both host and guest validate block headers, loop flags, byte lengths and checksums
before publishing residency. PSX ADPCM stores 28 samples in 16 bytes; loop flags
and integer SPU pitch require explicit handling. See [PSX-SPX SPU documentation](https://psx-spx.consoledev.net/soundprocessingunitspu/).

Every profile is independently decoded through FFmpeg, with frame counts and
signal-to-noise metrics recorded. Decoder equality is not claimed: FFmpeg's
predictor-history rounding differs from the encoder's rounded integer metric.
See [FFmpeg's ADPCM_PSX decoder](https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/adpcm.c).
Synthetic tests cover deterministic encoding, decoding quality, framing, malformed
PCM, source cache invalidation and resource-boundary rejection.

## Historical resident-bank capacities (before build64)

Bytes below are actual encoded payload sizes, including final-block padding.
Durations are rounded source lengths. The six 8,000 Hz mono ambience payloads
are resident in the implemented guest; stereo and music columns remain experiments.

| Source clip | Seconds | 22,050 Hz stereo bytes | 8,000 Hz mono bytes |
| --- | ---: | ---: | ---: |
| cave_wind_loop | 21.574 | 543,680 | 98,624 |
| cave_noises | 43.720 | 1,101,760 | 199,872 |
| dirtmouth_wind_loop_a | 12.629 | 318,272 | 57,744 |
| dirtmouth_wind_loop_c_gate | 8.517 | 214,656 | 38,944 |
| ruins_rain_indoor_loop | 6.967 | 175,584 | 31,856 |
| ruins_rain_outdoor_loop | 10.000 | 252,032 | 45,728 |
| Dirtmouth 1 | 103.745 | 2,614,400 | — |
| S89 Accordion Dirtmouth-16 | 46.800 | 1,179,392 | — |

A complete Cave ambience bank at 11,025 Hz mono is 518,272 bytes, exceeding SPU
RAM after effects and SDK reservations. At 10,000 Hz mono it is 470,096 bytes and
fits alone, but all six Cave/Surface loops total 590,944 bytes: an exact overlapping
transition cannot keep both room banks resident at that rate.

At 8,000 Hz mono, all six complete loops total **472,768 bytes**. They occupy
SPU `[0x8D00, 0x7C3C0)` and voices 6..11, above the actual effects bank
`[0x1010, 0x8D00)` on voices 0..5. This leaves **15,424 SPU bytes**. No reverb
workspace is reserved. The host checks the current effects-bank size and refuses
an overlap rather than silently trusting this address forever.

Mono conversion removes stereo detail and an 8 kHz sample rate reduces audio
bandwidth. No source stem or clip duration was deliberately removed. Pitch 743
plays at approximately 7,999.585 Hz; the six clips have 2..24 padding samples in
their final ADPCM block. The production flag changes preserve independently
measured decode quality. The measured decoded end-to-start differences are at
most five PCM units, but this is not proof of click-free physical playback.

## Historical all-SPU startup and cue lifecycle (before build64)

`disc::Cache::prepare_ambience` runs before any room arena is leased or exposed.
It reuses the unused 256 KiB room arena for one complete raw clip at a time,
through the existing CD IRQ service. CD Done includes Pause retirement; only then
does the guest borrow the bytes for checksum validation and a synchronous SDK SPU
upload. The largest clip is 199,872 bytes and fits with sector padding. The arena
returns to room streaming afterward; there is no permanent main-RAM audio buffer.

All six clips must validate and finish uploading before the bank is ready.
Until then, no ambient voice can start. Retry silences the voices and clears bank
readiness before replacing any SPU bytes. Per-clip SPU ranges, voices, pitch,
headers and loop flags are checked in addition to the payload checksum.

The source channel order is 0, 1, 3, 4, 5, 6, mapped to SPU voices 6..11. With
full-scale voice volume 16,383, the Cave target volumes are
`[16383, 2, 2, 16383, 16, 16]`; Surface uses
`[2, 16383, 12588, 2, 16, 16]`. Cave enables bank mask 57, Surface mask 54.
Disabled stems stop and become silent after the 30-tick transition. Already-playing
shared rain keeps its phase; repeated cues for the same scene do not restart loops.
The mixer ticks independently of gameplay pause, and settled loops need no
per-tick volume writes. These are source endpoints and lifecycle with an explicit
linear-ramp approximation, not complete AudioMixer parity or player audio settings.

## Historical all-SPU validation and limits (before build64)

The final audio evidence is
[`captures/ambience-reader/report.json`](../captures/ambience-reader/report.json).
The validator binds EXE, CUE/BIN, link map and clip hashes, records its exact
commands, and reads the emulator's actual SPU RAM and audio output. Its normal
EXE SHA-256 is `32aaf06131ee6f6d99f9ec958f269ac51470ba2564034da7e7337d19ea13b5a1`;
the normal BIN SHA-256 is
`81ae3a6f34ba9d4cb21ed2df55c1de851082d1d4e7648dc64b51525f847d2092`.

Both final six-route pacing suites pass:
[normal](../.hkpsx/reader-suite/report.json) and
[telemetry](../.hkpsx/reader-telemetry-suite/report.json). The telemetry
EXE SHA-256 is `96beb6d0c5aac3ad2b2eb8b2faf688ec4acebc7f89028598ffe743031ff62448`.
The sixth route (`recording-3`) covers earned SOUL and focus healing. The final
normal build also passes the [actual-CUE main validator](../.hkpsx/reader-validation.log)
and [focus/ambience replay](../captures/focus-reader/report.json).
The final test run contains 201 passing tests, recorded in
[the test log](../.hkpsx/reader-tests.log).

- All **472,768 bytes** matched the six cooked payloads at their intended SPU
  addresses. Four Cave stems started once and remained selected.
- The capture contains **96 seconds** of output, exceeding two iterations of the
  longest 43.724-second loop, with continuing nonzero output and no clipped samples.
- This stationary audio replay recorded no guest faults, loading errors or boundary
  waits, and its maximum pad-poll gap was one VBlank.
- Corrupting the last clip left five clips uploaded but **zero voices started**:
  readiness stayed false, no room was admitted, and the retry screen reported the
  ambience error. All diagnostic BIN/CUE files stayed in the PS1 library.

`tools/validate_ambience.py --output <new-output-directory>` reproduces this check
against the selected build report; use a new output directory for each run.
Surface transition lifecycle has native tests, but **natural Cave-to-Town traversal
with ambience remains unverified**. Physical hardware, stereo fidelity, click-free
loops and downstream mixer/player-setting parity are unverified. The final pacing
suites cover their six recorded routes; neither those routes nor the stationary
audio replay establish whole-game coverage or validate a later binary.

At this earlier milestone Title and Dirtmouth/accordion were not implemented.
Build65 now handles Title above; these ring estimates remain historical. A future
Dirtmouth music ring could reclaim Cave-only SPU loops after their fade completes,
but the current implementation keeps all six ambience loops resident. Town's
four 8,000 Hz loops total 174,272 bytes; a 256 KiB music ring plus conservative
32 KiB effects and low-SPU reservations would leave 50,992 bytes. At 22,050 Hz
stereo, music consumes 25,200 bytes/second and each 128 KiB half holds about
5.20 seconds. These are capacity calculations, not an implemented music transport.

The pinned SDK exposes SPU uploads and loop-address chaining but no complete
high-level audio-ring refill service. Music refill deadlines and room-read
arbitration remain future work. CDDA cannot substitute for that arbitration while
fetching room data: the SDK's `cdda-read-contention` example documents the hardware
conflict between CDDA playback and data reads. Current resident ambience avoids
this contention entirely after startup.

## Focus audio source work

The isolated `.hkpsx/focus-audio-investigation/README.md` and `report.json` bind
source FSM actions, pooled-player behavior, five Windows input hashes, the SDK
revision/files and eight complete ADPCM profiles. No Focus audio is played by
the current guest. Charging starts in Focus Start and continues across repeated
heals; the heal one-shot starts on cycle completion even at full health, before
AddHealth, and its independently pooled tail survives Focus cancellation.
The requested0.33-second charging fade is cut short when the0.25-second cancel
animation or0.23-second finish state exits. Damage stops charging immediately.

The original charging loop is19.345 seconds. Complete mono ADPCM needs88,448
bytes at8kHz,121,888 at11,025Hz or176,880 at16kHz. The1.567-second heal plus silent
END uses14,352 bytes at16kHz, leaving1,072 SPU bytes; at11,025Hz it uses9,888 and
leaves5,536. A full charging reservoir fitted that earlier main-RAM gap; the current
61,356B gap cannot hold even the88,448B8kHz payload. A future
RAM-to-SPU ring could therefore avoid all Focus-time disc reads without shortening
the source loop. No ring state, linked allocation, audible-quality choice or
underrun guarantee is implemented. The pinned SDK has low-level upload, loop
address and ENDX APIs, but no complete refill/IRQ owner; this must coexist with
the existing CD exception wrapper and preserve ADPCM history and heal tails.
