# Complete-game execution roadmap

The current plan is [Complete Hollow Knight for GPT-5.6-Sol](plans/COMPLETE_GAME_SOL.md).
Use [Sol start here](plans/SOL_START_HERE.md) for the immediate handoff.

The user selected whole-world import followed by shared systems across all areas.
Generic FPS optimization was paused for playtesting/gameplay expansion; maintain
performance during development and close the final30fps/hardware requirement
alongside completion. Do not use the historical performance-first gate below to
indefinitely block missing gameplay work. A first ending remains an intermediate
milestone; optional content, installed expansions, modes and alternate endings
are included in the complete-game plan.

## Historical milestone notes (superseded sequencing)

### Earlier playable-game milestones

The user requested this order on September 15, 2026: meet the existing FPS goal,
then implement the missing gameplay toward the entire game playable from start
to finish. This is the development objective, not a claim of current parity or
established full-game compatibility.

## Performance gate

Keep 320×240, current camera framing, source layers/effects and existing texture
quality. The saved long controller route must pass the two-VBlank maximum with
no input faults, traversal CD/static atlas uploads or boundary loading stalls.
Lifeblood and Great Door progression must remain correct. Average 30 fps alone
does not pass. Preserve final binary hazards, memory budgets and pixel checks.
Emulator results do not establish physical-console timing.

After this passes, add representative routes for each new area/system so later
content cannot silently abandon the frame budget. Continue performance work
alongside features whenever an added route regresses.

## Gameplay expansion after the gate

1. Restore the normal start and persistent play loop: original opening flow,
   benches, saving/loading, death and Shade recovery. Remove dependence on the
   development spawn/reset route for progression validation.
2. Expand the connected world from Dirtmouth into the required early areas.
   Use the existing offline room/neighbor inventories with bounded resident
   scene, texture and animation data. Larger world coverage must keep traversal
   responsive; the present two-scene residency does not scale to the whole game.
3. Implement source-derived enemy families, bosses, NPC/FSM interactions,
   abilities, inventory, pickups, purchases and progression flags as the route
   encounters them. Preserve authored prerequisites instead of bypassing them.
4. Extend through the complete main progression and an original ending, with a
   recorded new-game-to-ending route and no debug grants, warps or blocked exits.
5. Expand alternate paths, optional areas, quests, endings and remaining content;
   audit effects, audio/music, scripts and unsupported source object types until
   the whole requested game is covered.

Track each milestone as implemented, replay-verified, or still missing. A
first ending is an intermediate milestone, not proof that all content works.
Source behavior and content come from the read-only Windows Steam installation.
Generated retail assets and captures stay local; only source and documentation
are committed. Keep one playable BIN/CUE pair in the PS1 library throughout.
