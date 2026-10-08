# Start here: GPT-5.6-Sol execution handoff

> **Stale snapshot.** This is the handoff written before implementation began
> (build 71, two playable scenes). It is kept for history and no longer
> describes the port. The current state is the execution ledger,
> [COMPLETE_GAME_PROGRESS.md](COMPLETE_GAME_PROGRESS.md); the master plan is
> still [COMPLETE_GAME_SOL.md](COMPLETE_GAME_SOL.md).

The complete assignment is [COMPLETE_GAME_SOL.md](COMPLETE_GAME_SOL.md):
32 implementation packages, a 20-slice starting sequence, validation contracts,
an exhaustive completion definition and a copy/paste execution prompt.

This is a plan handoff. No implementation or import process is running on behalf
of the plan. Begin execution when the user asks you to take over.

## First session

1. Read repository `AGENTS.md`, `README.md`, `docs/BOOTSTRAP.md`, `NEXT_PROMPT.md`
   and the newest section of `docs/STATUS.md`.
2. Read the master plan's sections 1–6 once. Use section 7's selected package and
   section 8's starting sequence for implementation; do not reread 13,000 words
   every turn or start another planning cycle.
3. Check Git, the Windows source with `python3 tools/doctor.py`, the current
   playable disc hashes and any actual live process handles.
4. Start P01. Diagnose the failed import worker, then repair bounded supervision,
   resume and interruption behavior. Do not assume an out-of-memory cause.
5. Complete the all-scene pass with verified source/code/output identities, then
   follow the plan through source gaps, scalable metadata, shared art, streaming,
   connections, gameplay systems, progression and final validation.

## Snapshot to recheck

Source baseline when this plan was written: `ecc697e` on `main`.
Playable disc 71: Tutorial/King's Pass and Town/Dirtmouth only,106 spatial views.
World import:249 exported scenes,196 pool-failed jobs,56 not recorded; final
source verification was not completed. The first reported failed future was
Mines_02; that does not identify the process that crashed.

Current source and playable-disc state are different. The new host importer is
committed source; its exports are not playable PS1 rooms.344 Python tests passed
for that work. Current linked headroom is8,044 B before the reserved stack;
remove fixed two-scene metadata storage before scaling the whole world.

## Working priorities

- Import all scenes and account for every source variant.
- Implement one shared system across all verified matching instances.
- Use the original controlled headless runner to derive/reproduce behavior.
- Keep required audio, effects, persistence and cleanup in each system's work.
- Prove results with source checks, meaningful native tests, actual guest routes
  and inspected images/audio; distinguish emulator and hardware evidence.
- Keep the full game as the destination. A first ending is a milestone.

## Constraints to keep visible

- Windows CrossOver source and original saves are read-only; no macOS fallback.
- Keep320×240, current framing/layers/effects,48px static cap, approved95% sharing.
- SDK `7b929ce473ed44b10b0ffca08f413eebf4eeeec4`, nightly2026-03-25; final hazards0.
- Exactly one playable pair in `~/Downloads/ps1 games/hk-psx.bin/.cue`.
- Preserve disc 71 until replacement is requested/authorized; no candidate copies.
- Headless guest tests; user controls the emulator GUI. No automatic burns.
- Source-only push to private `EBonura/hk-psx` is authorized. Retail assets and
  generated content stay ignored, including source records in `.hkpsx/`.

For every package, update the execution ledger and current next action. Include
what changed, what was actually tested, build hashes when applicable, remaining
exceptions and the next command. Do not claim a system is done just because its
host data exists, its pure controller tests pass or its scene can be displayed.
