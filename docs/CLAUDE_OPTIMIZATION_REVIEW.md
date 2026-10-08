# Optimization branch review, September 15

The user asked to salvage useful changes, remove Claude's worktree, and push
validated improvements to main. The clean worktree at
`.claude/worktrees/hk-psx-project-state-d72a31` was removed with `git worktree
remove`. Its commit history is retained at branch
`claude/hk-psx-project-state-d72a31`, commit `8db50a5`, and the existing remote
`codex/bootstrap` branch. Shared asset/build directories were symlinks; removing
the worktree did not remove their targets or create another playable disc.

All nine files changed since the common base `6517d70` were reviewed:

| Change | Decision |
| --- | --- |
| Admission-time scenery bounds (`7908b54`) | Ported and benchmarked as pass38, then compacted to480bytes of corner indices in the retained pass40. Reject invisible/fully hidden draws before full vertex projection. |
| Larger rectangle set and multi-subtraction | Already superseded here by bounded partitions, repaired-child clipping, opaque cores and certified tile coverage. |
| Full frame pipeline (`907a17e`) | Preserve in history. Its measured branch remains slower overall than root pass36 and adds a displayed-frame latency tradeoff. Root pass37 instead prepares static background packets during the existing wait and validates exact current state before reuse. |
| Timed build hold (`755c765`) | Coupled to the full pipeline; not copied independently. |
| Occluder scratchpad (`022de5d`) | Already implemented with a different, checked 972-byte layout here. |
| Deferred animation uploads | Entirely coupled to the full pipeline; no independent safety fix found. |
| Five profiling watches | Only describe that pipeline's build/raster/hold/ring counters. Root retains its own relevant diagnostics. |
| README, status, budget and handoff | Preserve as historical records, not current build identity. |

Claude's exact rotated-cover and enlarged-occluder experiments reduced raster
work but increased total CPU cost. Its texel-cell experiment changed sampling,
lost existing clipping savings and faulted the input queue; it was reverted.
Do not repeat these unchanged. The detailed experiment record remains in
`8db50a5:docs/PERFORMANCE.md`. Claims that all exact optimization is exhausted
are not treated as proof for this different renderer.

Native bounds validation compares the original projection against translated
extrema over full-range valid integer coordinates, strict viewport boundaries,
degenerate quads and rotated/repair shapes. Retail-asset checks and profile
artifacts remain ignored under `.hkpsx/claude-bounds-review/`.
