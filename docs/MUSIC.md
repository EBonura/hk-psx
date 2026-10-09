# Source ambience and CD-streamed music

## XA songs instead of Red Book tracks (2026-10-04)

The title, the False Knight's Boss Battle 1, Brooding Mawlek's and Gruz
Mother's Enemy Battle and the Boss Defeat sting are the four channels of one
XA-ADPCM file, `MUSIC.XA` (37.8 kHz stereo, single speed: the drive plays one
sector in four, so one song costs a quarter of what its Red Book track did).
The disc has no audio tracks and its cue is one data track. Area music is
unchanged: it still streams as mono ADPCM through the SPU ring, because XA
keeps the drive for itself (below).

Cook: `hk-cook xa-music` (host/hk-cook/src/xa_music.rs, in-process from
`hk-psx-build`) decodes the four clips with FMOD, pads the longest with three
seconds of silence so the head is still inside the file when the guest's
one-second poll sees the song end, and runs the SDK's `xa-encode` and the
reference decoder's `xa-score`. It writes `data/music.xa`, `data/xa_music.rs`
(each song's channel and the sectors from the file start to the end of its own
audio) and `.hkpsx/xa-music.json`. The encoder comes from the SDK revision in
`xa-encoder.lock.json`, exported once under `.hkpsx/xa-encoder`: the guest's pin
(sdk.lock.json) predates it and cannot move until host/stack_budget.py and
host/code_modules.py are ported to the SDK's single-region linker script and
Rust hazard patcher (the SDK dropped `tools/hazard_patch.py`).

Disc: `hk-psx-build disc` (host/hk-build/disc.rs, ported from the packaging
half of build_guest.py) stages WORLD.PAK and calls `mkisopsx --xa-file`. The
file follows WORLD.PAK; the guest finds it by name in the root directory
(`disc::Cache::find_xa`, before the CD interrupt is installed), so a program
moved to another LBA keeps working.

Guest: `game/src/xa_player.rs` is the logic of the SDK's
`psx_io::cd::xa::Player` over the pinned SDK's drive commands (delete it for the
import when the pin moves). Each song loops at its own end: the player is given
the song's span, not the file's. `music.rs` plays the title (after the
source's one second delay) and the fights; the fade, the Music volume option,
`Floor Break`'s snapshot fade, the one-shot sting and the stop before a room
load work as they did on CD-DA, because the CD input's volume is the SPU's.

Why area music is not XA: a read of any other sector stops the XA stream, and
restarting it seeks back, so every gate load, room prefetch and ambience clip
read would cut the song. See the measurements in the 2026-10-04 hand-back.

## Area music and the False Knight's CD-DA (2026-09-23; the fights are XA now)

Area music streams mono from main RAM into an SPU ring, so it keeps playing
through room loads; the False Knight's fight plays full-quality CD-DA, because
a fight reads nothing. `host/area_music.py` reads each admitted scene's
SceneManager music cue and snapshot, the MusicRegion colliders (as bounding
boxes) and the persistent AudioManager's music mixer groups. A cue is up to six
looping layers and a snapshot picks the audible ones (Crossroads Normal: bass
and main; Sub Area: main only), so every (cue, audible layer set) a player can
reach through the gate graph is premixed into one 22,050 Hz mono ADPCM stream,
resampled to whole 2,048-byte sectors (under a cent of pitch for loops past
6 s). Eleven streams, 14 MB, sit at the end of WORLD.PAK. Boss1 (Boss Battle 1,
168 s) is CD-DA track 3 beside the title's track 2.

Guest: `game/src/audio_stream.rs` is the ring (two 8 KiB halves on voice 6,
refilled on the SPU IRQ boundary from a 98,304-byte FIFO, 7.8 s); the FIFO took
the RAM that used to hold cave_noises, which is an ordinary per-area SPU clip
now. `game/src/music.rs` applies scene and region states with linear fades on
VBlanks, switches premix at the same offset when only the layer set changes,
fades out and rebuffers on a cue change, and starts a stream once two halves
and a refill chunk are buffered. `disc.rs` owns the drive: a refill reads 16
sectors in the background from the CD interrupt; a room read waits for one in
flight; during a load a refill runs between room reads only if the FIFO drops
under 20 sectors; a gate into a scene that does not continue the stream fades
it out and stops refilling it. CD-DA takes the drive (CD interrupt masked at
the CPU, polls once a second for the loop) from the Rubble End kill-all until
the boss dies or the Knight does, and any room load stops it first.

Measured on the route tapes (emulator, frozen frontend): 0 underruns, FIFO
minimum 24 sectors (3.9 s), music continuous through the Crossroads_01 to _07
load (capture correlation 0.95 to 0.99 against the cooked stream across the
load), Dirtmouth 0.988 and Greenpath 0.875 against the cooked streams (0.07 and
0.06 without music), Boss1 0.694 against the CD-DA payload over the fight's
SFX. Cost: Crossroads gameplay 21.18 to 20.67 fps with instructions per frame
unchanged; the extra cycles are the sector IRQ's PIO drain (about 41,000
cycles per sector in the emulator model) and ring DMA. 16 kHz only moved it to
20.73. Limitations: a Normal to Sub Area change is heard once the buffered
audio ahead of it has played, not as a layer fade; the Dirtmouth accordion
(nymmInTown) is not tracked; silicon timing is unmeasured.

## Per-area ambience residency (2026-09-23)

Ambience SPU residency follows the area. Boot loads only `cave_noises`' RAM
source; every other loop is read at the scene gate whose cue first plays it
(`disc::Cache::prepare_scene_ambience`, before the scene takes the arena) and
uploaded in 4 KiB slices with a pad checkpoint between them. `host/ambience.py`
`allocate` gives each clip an SPU address it shares with clips that are never
resident together: two clips conflict when one cue plays both, or when a
resolved gate joins a scene playing one to a scene playing the other, because
the outgoing cue fades out after the incoming one keys on. A transition no gate
describes (a respawn at a distant bench, the Select reset) is arbitrated by the
guest: an incoming clip whose bytes overlap a stem still sounding cuts that
stem during the black load and counts it in `HK_AMBIENCE_CUTS`.

Resident per area, before: all eight loops (280,928 SPU bytes plus the
99,936-byte RAM source) everywhere. After, measured from final SPU dumps on the
route tapes: Cave (King's Pass, Crossroads) cave wind, cave noises and indoor
rain; Surface (Dirtmouth, Sly's shop) the Dirtmouth wind and indoor rain;
Greenpath its own loop. A loop from the previous area stays in its slot until a
clip needing those bytes arrives. Fog Canyon and Waterways never load. Since
area music took cave_noises' RAM copy and ring, that loop is held whole in SPU
too: the widest set (Cave plus MiscWind) is 204,960 bytes and 16,656
contiguous SPU bytes are free below the music ring, which sits below the world
one-shot bank (see docs/BUDGET.md). An area change reads its
new clips (Town to Crossroads_01: cave wind and cave noises, 74 sectors);
gates within an area read none.

## Focus playback (build66,2026-09-15)

The full19.345-second charging loop uses8,000Hz mono; the full1.567-second heal
uses22,050Hz mono. This follows sustained-loop and short-effect categories,
without truncation. `host/focus_audio.py` validates Windows Spell Control FSMs,
AudioSources, clips and inspected assembly hashes. Local provenance and cooked
assets remain ignored. The108,208-byte bank loads through scene scratch once at
startup into0x5C960..0x77010, immediately above ambience and immediately below
Runner. Its base is hardcoded in `host/focus_audio.py` and moves by whatever
ambience grows or shrinks; `host/ambience.py` refuses a cook that leaves it
stale. Main RAM
has53,132B before the protected stack; Focus adds no persistent sample bank there.

Voice18 loops charging;19/20 alternate heals from shared data so tails survive
release and cancellation. The actual Focus state supplies start/repeat/end phases;
the source fade is cut off on phase exit. Damage/scene interruption stops charge
immediately. All voices follow the SFX setting. Original mixer DSP and pause-audio
parity are not established. Source no-charm timing fits two heal voices; future
charms must revisit the overlap budget. No event-time CD read is required.

Native tests run the actual guest dispatch against bounded fake SPU registers,
validating malformed-bank rejection, addresses, loop flags, one-shot ADSR,
repeat/full-health completion, interruption, tails and volume. Final-CUE Focus
and isolated-audio routes pass; charging phrases correlate above0.9997 and heal
tails above0.997 with independently decoded cooked clips after alignment. Resident
SPU bytes match the bank. Reports are `.hkpsx/game66/{focus-wave-check.json,
heal-wave-check.json,validation.json}`; final disc identity is in STATUS.md.
More enemy, break, death and UI sounds remain missing. CD-DA title and the complete
cave-noises ring below are retained; historical free-space figures are superseded.

## Title playback (build65,2026-09-15; XA now, see above)

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
stay ignored. Build65 had61,356B free before the protected48KiB stack;
136,704B contiguous SPU was available. Tutorial's Silent snapshot and the
uncooked Town music trigger are unchanged. Focus is restored in66; continue missing effects; historical capacity experiments below are not current allocation.

## Ambience milestone (build64,2026-09-15)

Source-derived player running and hard-landing sounds are integrated, along
with the Great Door's first hit variant/pitch range. Running uses the original
complete nonlooping rhythm sequence: stop outside running, restart after clip
completion, and defer a new start while soft landing plays. At build64 the port still lacked
music, Focus, many enemy/break/death/UI effects and original hard-land recovery.

All eight full ambience payloads are retained. `cave_noises` lives in main RAM
rather than occupying its full length in SPU: at 43.72 s it is the longest
resident loop and costs 99,936B of cache at 4,000 Hz. It keeps the streamed
slot under the eight-channel set, because a streamed clip occupies only the
ring whatever its length, so which channel streams is a question of which one
stays exercised rather than of bytes. The
SPU ring's boundary sanity window is derived from the cooked pitch rather than
fixed, since a half lasts twice as long at the lower rate. A continuous16KiB SPU
ring plus8KiB aligned refill scratch uses hardware playback IRQ boundaries,
polled from real-time input checkpoints even during simulation pause. DMA occurs
only into the inactive half with IRQ detection temporarily disabled; detection
is rearmed for the next playback boundary afterward. ENDX is read-only and is
not used. There is no playback-time CD traffic. Late/missing/early boundaries
stop the streamed voice and increment HK_AUDIO_STREAM_UNDERRUNS.

Allocation: player SFX0x1010..0x135C0 (75,184B), Geo0x14000..0x17640
(13,888B), ambience0x18000..0x5C960 (280,928B), Focus0x5C960..0x77010
(108,208B) and Runner0x77010..0x7C000 (20,464B). That leaves **16,384B**
contiguous, not the136,704B a previous version of this paragraph claimed, which
counted the Focus and Runner banks above ambience as free. Ambience shrank
16,272B when the resident set went from six channels to eight, because eight
only fit at4,000Hz; that is where the margin came from, and it is still not
room for a music ring of any useful size. `docs/BUDGET.md` prices what is left.
This does not implement music transport or reserve reverb workspace; future
CD-read arbitration and stereo refill deadlines need separate validation.

Quality categories:22,050Hz mono player one-shots;11,025Hz mono longer running
sequence;11,025Hz Geo. Ambience is its own category and is now one rate:
all eight resident loops are4,000Hz mono, because eight do not fit SPU
at8,000. What each channel pays for that is recorded beside
`cook_music.RESIDENT_ATMOS_CHANNELS`. Hurt was raised from11,025 to22,050Hz to match its short-effect
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
call sites. See tools/hkref/README.md for coverage limits and commands. Requests
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
- All eight referenced ambient AudioSources loop, play on awake, have volume/pitch
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

There are 17 unique clips and 52 profile conversions: 22,050 Hz stereo and
11,025 Hz mono for every clip, plus 10,000, 8,000 and 4,000 Hz mono for the six
resident ambience clips. Both of the last two are cooked for every resident clip
whatever `cook_music.RESIDENT_ATMOS_RATES` currently picks, so re-rating a
channel is an `host/ambience.py` run rather than a source re-conversion.
Complete clips are preserved; lower rates and mono are explicit quality
experiments. No silence replacement or length truncation is used. Each channel
is a separate PSX ADPCM plane, with at most 27 zero samples in its final block;
valid frame counts are retained separately.

Lower is not uniformly worse. PSX ADPCM spends four bits per sample whatever the
rate, so a clip whose energy is already under the lower Nyquist gains resolution
where the content is and loses a band that holds nothing. Measured as the cooked
payload's SNR against its own resampled source: `cave_atmos_misc_3`, 99.999% of
its energy below 2 kHz, reconstructs at 16.70 dB at 8 kHz and **24.47 dB at
4 kHz**. It is the only resident clip better at the lower rate. The other seven
pay between 0.45 dB (`fog_canyon_atmos_loop`) and 4.93 dB
(`green_path_atmos_loop`), and they are at 4 kHz anyway because eight loops do
not fit SPU at 8. The rate stays a per-channel decision recorded beside the
channel set rather than a property of which clip streams, and the table there
is what a future channel would have to argue against.

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

Mono conversion removes stereo detail and a 4 kHz sample rate reduces audio
bandwidth further. No source stem or clip duration was deliberately removed.
Pitch 372 plays at approximately 4,005.176 Hz; the eight clips have 1..24
padding samples in their final ADPCM block. The production flag changes
preserve independently measured decode quality. The measured decoded
end-to-start differences reach 250 PCM units on the widest-band clip, which is
not proof of click-free physical playback either way.

## Historical all-SPU startup and cue lifecycle (before build64)

`disc::Cache::prepare_ambience` runs before any room arena is leased or exposed.
It reuses the unused 256 KiB room arena for one complete raw clip at a time,
through the existing CD IRQ service. CD Done includes Pause retirement; only then
does the guest borrow the bytes for checksum validation and a synchronous SDK SPU
upload. The largest clip is 199,872 bytes and fits with sector padding. The arena
returns to room streaming afterward; there is no permanent main-RAM audio buffer.

All six clips must validate and finish uploading before the bank is ready.
Until then, no ambient voice can start. Retry silences the voices and clears bank
readiness before replacing any SPU bytes. Per-clip SPU ranges, pitch, headers
and loop flags are checked in addition to the payload checksum.

Ambience owns six SPU voices. A clip no longer owns one of them: `AMBIENCE_POOL_VOICES`
is taken when a stem keys on and given back once it has finished fading out, and
only `AMBIENCE_STREAM_VOICE` is held for the life of the disc, because
`audio_stream.rs` owns the single SPU IRQ address register while it runs. So what
bounds the resident set is not how many loops are resident but how many stems can
be audible at the same moment: a transition fades the outgoing cue's stems out
while the incoming cue's rise, and across all 456 catalogue scenes with an atmos
cue the widest such union is five. `host/ambience.py` refuses a set whose cooked
cues can outrun the pool, and a cue the pool cannot serve leaves the stem unplayed
and counts it in `HK_AMBIENCE_VOICE_DENIALS` rather than taking a voice from a stem
that is still fading.

The resident channel set is a cook-time budget decision and moves; it is
`cook_music.RESIDENT_ATMOS_CHANNELS` and the figures below are whatever the last
cook wrote into `data/ambience.rs`, not a second place to maintain it. It is now
channels 0, 1, 4, 5, 7, 9, 10 and 15. Eight channels are enough to leave no
catalogue scene without an audible stem, but neither covering set includes 4,
and dropping 4 would take a full-gain layer out of the 41 admitted Cave scenes
and stop the SPU ring from ever starting on this disc. Deepnest's 18 catalogue
scenes stay silent instead; the trade is recorded beside the tuple. With
full-scale voice volume 16,383, the Cave target volumes are
`[16383, 2, 16383, 16, 2, 2, 2, 2]`; Surface uses `[2, 16383, 2, 16, 2, 2, 2, 2]`,
Greenpath `[2, 2, 2, 2, 16383, 2, 2, 2]` and MiscWind
`[2, 2, 2, 2, 2, 2, 2, 16383]`. Cave enables bank mask 13, Surface 10,
Greenpath 16, MiscWind 128.

Two of the eight, channels 9 and 10, are enabled by no admitted scene: Fog
Canyon and the Waterways are both outside the current catalogue. Their loops
are on the disc and in SPU for scenes not yet admitted, and no cooked cue keys
them on, which is why the clip table is read off the persistent AudioManager
rather than off whichever scene happens to enable a channel.
Disabled stems stop and become silent after the 30-tick transition. Already-playing
shared stems keep their phase; repeated cues for the same scene do not restart loops.
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

## Sound effect and Geo resampling (build 108)

`convert_wav` in `host/hk-cook/src/cook_audio.rs`, which the eight effects, the footstep set and the six
Geo samples pass through, used a box average for integer rate factors and a
rational box integration for the 48000 to 11025 footsteps. A boxcar is a poor
lowpass: it rolls off inside the band it keeps and barely rejects above the new
Nyquist, so content folded back as aliasing. It now calls the same ffmpeg
polyphase resampler that `cook_music.cook_clip` already used for music,
ambience, Focus and Runner, so the whole game shares one conversion rather than
two, and the hand-written filter is gone rather than replaced by another.

Sizes are unchanged: the effects bank stays 63,376 bytes and Geo moves 13,904
to 13,888, one ADPCM block. A real filter has a finite stopband where the box
cancelled the Nyquist-alternating case exactly, so that test now asserts the
residue is 40 dB down rather than zero. The resampler primes its filter, which
can truncate a clip shorter than that window, so `convert_wav` fails when the
output length does not match the input duration instead of shipping a clip
missing its head or tail.
