#!/usr/bin/env python3
"""Build and drive tools/boss_sim.rs, the offline False Knight fight simulator.

This is not an emulator test and it is not the disc. It models the fight and
nothing else: read the module docstring of tools/boss_sim.rs before trusting a
tape it approves, and replay that tape through tools/replay_cue.py before
anyone believes it. tests/test_boss_sim.py is what catches this drifting out of
step with the guest.

Everything the simulator needs about the cooked world is derived here rather
than carried in the Rust, so a recook that moves a region, a gate or the boss's
own ActorSpec moves the simulator with it:

  * the boss scene's catalogue slots, their activation and collision bounds and
    their room packs, from data/regions.json, with every pack hash checked
    against that report the way tools/route_probe.py checks its own;
  * the terrain of the gates the source loads open, from data/battle_gates.rs,
    because game/src/battle_gates.rs lifts exactly those until the arena's
    BG CLOSE puts the room's whole set back and seals the floor;
  * the boss's cooked ActorSpec, from data/regions.rs.

usage:
  boss_sim.py replay <tape.pxtape> [--trace t.csv] [--summary s.json]
  boss_sim.py capture <replay_cue output dir> --out capture.json
  boss_sim.py run <masks.txt> [--trace t.csv] [--summary s.json]
  boss_sim.py search <prefix.txt|default> --out masks.txt [--route r.route]
              [--segment N] [--horizon N] [--beam N] [--alphabet a,b,c]

Every mode takes --start x,y,facing (Q16) for a tape that does not begin at
boss-fight.mcd's bench: journey-false-knight's fight is simulated from the
poll after its pause menu closes, standing at the arena's right end.

`default` is the fixed opening every boss tape shares: leave the title and walk
into the arena. Authoring the committed kill from it takes about 18 seconds:

  .venv/bin/python tools/boss_sim.py search default --out /tmp/masks.txt \
      --route /tmp/boss.route --beam 900
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ONE = 65536
SCENE_NAME = 'Crossroads_10'
# tools/cards/boss-fight.mcd's saved stand, measured off HK_PLAYER_X/Y in a
# replay's first gameplay poll. tests/test_boss_sim.py pins it against the
# committed capture, so a card or a spawn change fails there rather than here.
START = (786432, 1798268, 1)
# The sealed arena floor, between the two Battle Gate colliders the fight puts
# back. A search guard only: it prunes tapes that walk off the floor, where the
# region machinery this does not model would take over.
KEEP_X = (int(11.5 * ONE), int(45.5 * ONE))
# The fixed opening every boss tape shares: leave the title, then walk right
# through the Battle Scene trigger at x 13.5 to 14.5 and stop short of the
# boss's own body box at x 23.81, which the hero would otherwise walk into for
# a free mask. The search takes over from there.
PREFIX_END = 180
BUTTONS = {'select': 1, 'l3': 2, 'r3': 4, 'start': 8, 'up': 16, 'right': 32, 'down': 64,
           'left': 128, 'l2': 256, 'r2': 512, 'l1': 1024, 'r1': 2048,
           'triangle': 4096, 'circle': 8192, 'cross': 16384, 'square': 32768}


def cooked_regions():
    """The boss scene's catalogue slots, with their packs verified against the report."""
    catalogue = json.loads((ROOT/'data/regions.json').read_text())
    if not catalogue.get('complete'):
        raise ValueError('Refusing an incomplete/in-progress cook')
    rows = []
    for region in catalogue['regions']:
        if region['scene_name'] != SCENE_NAME:
            continue
        path = ROOT/region['path']
        if hashlib.sha256(path.read_bytes()).hexdigest() != region['sha256']:
            raise ValueError('Pack changed since the cook report: '+str(path))
        rows.append((region['chunk_id'], region['activation_bounds'],
                     region['collision_bounds'], path))
    if not rows:
        raise ValueError('No cooked '+SCENE_NAME+' regions to simulate against')
    return catalogue, rows


def open_gate_edges():
    """Per catalogue slot, the terrain of every gate the source loads open.

    game/src/battle_gates.rs starts CLOSED at PLACEMENT_CLOSED and its first
    apply lifts `!PLACEMENT_CLOSED & COOKED`, so those edges are not terrain
    until the arena broadcasts BG CLOSE. Read from the generated table rather
    than restated, because a recook moves both the gate bits and the indices.
    """
    text = (ROOT/'data/battle_gates.rs').read_text()
    placement = re.search(r'PLACEMENT_CLOSED\s*:\s*u16\s*=\s*(0b[01]+)', text)
    if not placement:
        raise ValueError('data/battle_gates.rs no longer declares PLACEMENT_CLOSED')
    closed_on_load = int(placement[1], 2)
    rows = re.findall(r'\((\d+),(\d+),&\[([0-9,]*)\]\)', text)
    if not rows:
        raise ValueError('data/battle_gates.rs no longer declares a REGIONS table')
    per_slot = {}
    for slot, gate, indices in rows:
        if closed_on_load >> int(gate) & 1:
            continue  # loads closed, so its terrain is never lifted
        per_slot.setdefault(int(slot), []).extend(
            int(i) for i in indices.split(',') if i)
    return per_slot


def broken_floor_edges():
    """Per catalogue slot, `Break Floor`'s edges, which the floor break lifts.

    Read from data/false_knight_floor.rs, the table game/src/battle_gates.rs
    links, rather than from the cook report, for the same reason as the gates.
    """
    path = ROOT/'data/false_knight_floor.rs'
    if not path.is_file():
        raise ValueError('data/false_knight_floor.rs is missing; the cook writes it')
    table = re.search(r'FK_BREAK_FLOOR_EDGES: \[\(u16, u16\); \d+\] = \[([^\]]*)\]', path.read_text())
    if not table:
        raise ValueError('data/false_knight_floor.rs no longer declares FK_BREAK_FLOOR_EDGES')
    per_slot = {}
    for slot, edge in re.findall(r'\((\d+), (\d+)\)', table.group(1)):
        per_slot.setdefault(int(slot), []).append(int(edge))
    return per_slot


def boss_placement(catalogue):
    """Where the one admitted False Knight stands, out of data/regions.json.

    `ActorSpec` is a catalogue of enemy types now, so the transform, the facing
    and the scene-unique source id are not in data/regions.rs at all: they are
    the placement, which rides in the scene's metadata bank. The cook report is
    what both of those are generated from, so it is what this reads.
    """
    found = {}
    for region in catalogue['regions']:
        if region['scene_name'] != SCENE_NAME:
            continue
        for actor in region.get('actors', []):
            if actor.get('movement_supported') and actor['movement_control']['kind'] == 'FalseKnight':
                found[actor['source']] = actor
    if len(found) != 1:
        raise ValueError(f'expected one admitted False Knight in {SCENE_NAME}, found {len(found)}')
    actor = found[next(iter(found))]
    return dict(source_id=actor.get('spec_source_id', int(actor['source'].rsplit(':', 1)[1])),
                x=round(actor['position'][0] * ONE), y=round(actor['position'][1] * ONE),
                initial_direction=int(actor['movement_control']['initial_direction']))


def boss_spec(catalogue=None):
    """The False Knight's cooked type from data/regions.rs, with its placement."""
    text = (ROOT/'data/regions.rs').read_text()
    match = re.search(r'hk_sim::ActorSpec \{bounds:\[(-?\d+),(-?\d+),(-?\d+),(-?\d+)\],'
                      r'health:hk_sim::EnemyParams \{health:(-?\d+),contact_damage:(\d+),'
                      r'evasion_ticks:(\d+),invincible:(\w+),damage_override:(\w+)\},'
                      r'controller:hk_sim::ActorController::FalseKnight \{[^}]*?'
                      r'trigger:\[(-?\d+),(-?\d+),(-?\d+),(-?\d+)\],barrel_spawn_y:(-?\d+)\}', text)
    if not match:
        raise ValueError('data/regions.rs no longer carries a False Knight ActorSpec')
    g = match.groups()
    placement = boss_placement(catalogue if catalogue is not None else cooked_regions()[0])
    # Actor::new's stable per-source seed, in 32-bit arithmetic because the
    # guest's usize is 32 bits and this host's is not.
    seed = (placement['source_id'] * 1664525 + 1013904223) & 0xffffffff
    return dict(bounds=[int(v) for v in g[0:4]],
                health=int(g[4]), contact_damage=int(g[5]), evasion_ticks=int(g[6]),
                invincible=int(g[7] == 'true'), damage_override=int(g[8] == 'true'),
                trigger=[int(v) for v in g[9:13]], barrel_spawn_y=int(g[13]),
                seed=seed, **placement)


WAVE_FIELDS = ('FK_WAVE_ORIGIN_Y', 'FK_WAVE_START_SPEED', 'FK_WAVE_ACCEL', 'FK_WAVE_BOX',
               'FK_WAVE_GROUND_RAY', 'FK_SPURT_BOX', 'FK_SPURT_DAMAGE_FROM', 'FK_SPURT_DAMAGE_TO',
               'FK_SPURT_DAMAGE', 'FK_SPURT_TICKS')


def wave_params():
    """The slam wave's cooked numbers, out of data/false_knight_art.rs, in
    `WAVE_FIELDS` order with the two boxes spread to four values each."""
    text = (ROOT/'data/false_knight_art.rs').read_text()
    values = []
    for name in WAVE_FIELDS:
        match = re.search(rf'pub const {name}: [^=]+= ([^;]+);', text)
        if not match:
            raise ValueError(f'data/false_knight_art.rs no longer declares {name}')
        values += [int(v) for v in re.findall(r'-?\d+', match.group(1))]
    if len(values) != 16:
        raise ValueError('unexpected wave constant shape')
    return values


def write_world(path, start=START):
    catalogue, regions = cooked_regions()
    gates = open_gate_edges()
    spec = boss_spec(catalogue)
    lines = ['# Generated by tools/boss_sim.py; do not edit.',
             f'start {start[0]} {start[1]} {start[2]}',
             f'keep_x {KEEP_X[0]} {KEEP_X[1]}',
             'actor {x} {y} {b0} {b1} {b2} {b3} {health} {contact_damage} {evasion_ticks} '
             '{invincible} {damage_override} {initial_direction} {t0} {t1} {t2} {t3} '
             '{barrel_spawn_y} {seed}'.format(
                 b0=spec['bounds'][0], b1=spec['bounds'][1], b2=spec['bounds'][2],
                 b3=spec['bounds'][3], t0=spec['trigger'][0], t1=spec['trigger'][1],
                 t2=spec['trigger'][2], t3=spec['trigger'][3], **spec),
             'wave ' + ' '.join(str(v) for v in wave_params())]
    floor = broken_floor_edges()
    for slot, activation, collision, pack in regions:
        bounds = ' '.join(str(v*ONE) for v in activation)
        collide = ' '.join(str(v*ONE) for v in collision)
        # `slot` here is the chunk id; the generated guest tables key the
        # runtime's catalogue slot, which is the row index, one less.
        indices = ' '.join(str(i) for i in sorted(gates.get(slot - 1, ())))
        # The Rust splits this line on whitespace and takes the last field as the
        # pack, so the path is written relative to ROOT (the binary runs there):
        # a checkout under a directory with a space in its name still parses.
        pack = Path(pack).relative_to(ROOT)
        lines.append(f'slot {slot} {bounds} {collide} {indices} {pack}'.replace('  ', ' '))
        if floor.get(slot - 1):
            lines.append(f'floor {slot} ' + ' '.join(str(i) for i in sorted(floor[slot - 1])))
    path.write_text('\n'.join(lines)+'\n')
    return len(regions)


def build(start=START):
    """Compile tools/boss_sim.rs against the shared host rlibs, as tools/route_sim.py does."""
    output = ROOT/'.hkpsx/boss-sim'
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(output/'cargo'))
    result = subprocess.run(['cargo', 'build', '--offline', '--locked', '--manifest-path',
                             str(ROOT/'shared/hk-sim/Cargo.toml'), '--message-format=json'],
                            cwd=ROOT, env=env, check=True, capture_output=True, text=True)
    # hk_sim itself is uplifted to the profile directory while its
    # dependencies (hk_format, psx_math) stay in deps/, so rustc needs every
    # directory an rlib was written to, not just hk_sim's.
    libraries, directories = {}, set()
    for line in result.stdout.splitlines():
        record = json.loads(line)
        if record.get('reason') != 'compiler-artifact':
            continue
        rlib = next((p for p in record['filenames'] if p.endswith('.rlib')), None)
        if rlib:
            directories.add(str(Path(rlib).parent))
        if record['target']['name'] in ('hk_sim', 'hk_format'):
            libraries[record['target']['name']] = rlib
    for crate in ('hk_sim', 'hk_format'):
        if crate not in libraries:
            raise ValueError('shared/hk-sim did not produce '+crate)
    binary = output/'boss-sim'
    command = ['rustc', '--edition=2021', '-Awarnings', '-O', str(ROOT/'tools/boss_sim.rs'),
               '-o', str(binary)]
    for name, path in sorted(libraries.items()):
        command += ['--extern', f'{name}={path}']
    for directory in sorted(directories):
        command += ['-L', f'dependency={directory}']
    # params.rs is included as CARGO_MANIFEST_DIR/../data/params.rs, the same
    # relation the guest crate has to it.
    subprocess.run(command, cwd=ROOT, env=dict(env, CARGO_MANIFEST_DIR=str(ROOT/'game')), check=True)
    world = output/'world.txt'
    write_world(world, start)
    return binary, world


def default_prefix():
    masks = [0]*PREFIX_END
    masks[9] = BUTTONS['start']
    masks[12] = BUTTONS['cross']
    for poll in range(100, PREFIX_END):
        masks[poll] |= BUTTONS['right']
    return masks


# Left, right and cross: the Knight reads them a tick late (game/src/input.rs
# hero_latency), and tools/boss_sim.rs reads a tape the same way.
LATE = BUTTONS['left'] | BUTTONS['right'] | BUTTONS['cross']


def tape_masks(masks):
    """Masks as the Knight should see them -> the pad a tape must hold: the
    late buttons one poll earlier."""
    masks = list(masks)
    return [(m & ~LATE) | (n & LATE) for m, n in zip(masks, masks[1:] + [0])]


def masks_to_route(masks):
    """A mask per poll as a tools/validate.py poll_tape route string."""
    events = []
    for name, bit in BUTTONS.items():
        run = None
        for poll, mask in enumerate(list(masks)+[0]):
            if mask & bit:
                if run is None:
                    run = poll
            elif run is not None:
                events.append((run, name, poll-run))
                run = None
    events.sort()
    return ','.join(f'{p}:{n}:{h}' for p, n, h in events)


def capture(output, tape):
    """Distil what the disc actually did out of a tools/replay_cue.py output.

    The whole route.csv is tens of megabytes and most of it says nothing about
    the fight, so what is committed is the three counters a replay can see move
    plus the run's final telemetry.

    A run whose hero dies is cut off at the death, because the simulator stops
    there by design: what follows is a respawn, a scene load and a re-armed
    arena, none of which it models.

    Regenerate every capture after a disc change, one replay each:

        for tape in boss-fight boss-death boss-sim-brawl; do
          cp tools/cards/boss-fight.mcd <scratch>/$tape.mcd
          .venv/bin/python tools/replay_cue.py --tape tools/tapes/$tape.pxtape \
              --output <scratch>/$tape --memcard <scratch>/$tape.mcd \
              --frontend <frontend>
          .venv/bin/python tools/boss_sim.py capture <scratch>/$tape \
              --tape tools/tapes/$tape.pxtape --out tools/tapes/$tape.capture.json
        done
    """
    import csv
    report = json.loads((output/'command.json').read_text())
    watches = report['watches']
    rows = list(csv.DictReader((output/'route.csv').open()))
    polls = [int(row['port1_polls']) for row in rows]

    def series(name):
        return [int(row['ram_'+watches[name][2:]]) for row in rows]

    # The hero's death, as the first fall to zero from a live mask count. The
    # boot leaves the watch word at zero, so a zero that was never preceded by
    # a live value is the counter not yet published rather than a death.
    health_series = series('HK_HEALTH')
    died, previous = None, None
    for poll, value in zip(polls, health_series):
        if previous and value == 0:
            died = poll
            break
        previous = value

    def transitions(name):
        out, previous = [], None
        for poll, value in zip(polls, series(name)):
            if previous is not None and value != previous:
                out.append([poll, previous, value])
            previous = value
        # The first row of a counter is the guest publishing it for the first
        # time, out of RAM the boot left at zero. That is not damage, and the
        # simulator starts with the fight already set up, so it has no such row.
        if out and out[0][1] == 0:
            out.pop(0)
        # The simulator stops at the hero's death by design: what follows is a
        # respawn, a scene load and a re-armed arena, none of which it models.
        if died is not None:
            out = [row for row in out if row[0] <= died]
        return out

    def signed(value):
        return value-(1 << 32) if value & (1 << 31) else value

    # Where the card's save leaves the hero standing, once the body has settled
    # onto the floor: the first sample that is still true ten polls later. The
    # raw first gameplay poll is mid-drop and would pin a y the guest passes
    # through rather than the one the simulator has to start from.
    x, y = series('HK_PLAYER_X'), series('HK_PLAYER_Y')
    first = next(i for i, v in enumerate(x)
                 if v and all(x[i+n] == v and y[i+n] == y[i] for n in range(1, 11)))
    final = {name: report['final_ram'][name] for name in (
        'HK_CHEATS', 'HK_DEATHS', 'HK_HEALTH', 'HK_FK_STAGGERS', 'HK_FK_CONVERSIONS',
        'HK_FK_DEATHS', 'HK_FK_ARENA', 'HK_INPUT_FAULT', 'HK_FK_HP', 'HK_FK_HEAD_HP',
        'HK_FK_ACTIVATED', 'HK_ARENA_GATE_CLOSES', 'HK_ARENA_GATE_OPENS',
        'HK_FK_BARRELS', 'HK_FK_BARRELS_BROKEN', 'HK_SOUL')}
    # The slam wave's counters, on discs that export them.
    final.update({name: report['final_ram'][name] for name in ('HK_FK_WAVES', 'HK_FK_WAVE_HITS')
                  if name in report['final_ram']})
    build_report = json.loads((output/'build-report.json').read_text())
    return {
        'note': 'Captured from a tools/replay_cue.py run of the tape below against the '
                'real disc. tests/test_boss_sim.py replays the same tape through '
                'tools/boss_sim.py and asserts the fight it predicts is this one. '
                'See capture() in tools/boss_sim.py to regenerate after a disc change.',
        'tape': tape,
        'tape_sha256': hashlib.sha256((ROOT/tape).read_bytes()).hexdigest(),
        'hero_died_at_poll': died,
        'disc_sha256': {name: info['sha256'] for name, info in build_report['outputs'].items()},
        'completed': report['completed'],
        'faults': report['faults'],
        'hero_start': {'poll': polls[first], 'x': signed(x[first]), 'y': signed(y[first])},
        'final': final,
        'transitions': {
            'fk_hp': transitions('HK_FK_HP'),
            'head_hp': transitions('HK_FK_HEAD_HP'),
            'health': transitions('HK_HEALTH'),
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('mode', choices=('replay', 'run', 'search', 'capture'))
    parser.add_argument('input')
    parser.add_argument('--trace', type=Path)
    parser.add_argument('--summary', type=Path)
    parser.add_argument('--out', type=Path, help='search: where to write the chosen masks')
    parser.add_argument('--route', type=Path, help='search: where to write the route string')
    parser.add_argument('--tape', help='capture: the repo-relative tape the run replayed')
    parser.add_argument('--start', help='x,y,facing in Q16 world units: where the hero stands at the'
                        ' first simulated poll, for a tape that does not start from boss-fight.mcd'
                        ' (the journey reaches the arena on foot)')
    parser.add_argument('--segment', type=int, default=5)
    parser.add_argument('--horizon', type=int, default=6000)
    parser.add_argument('--beam', type=int, default=800)
    parser.add_argument('--alphabet', default='left,right,cross,square,'
                        'cross+left,cross+right,square+left,square+right,'
                        'square+cross,square+cross+left,square+cross+right')
    args = parser.parse_args()
    if args.mode == 'capture':
        if not args.out:
            parser.error('capture needs --out')
        if not args.tape:
            parser.error('capture needs --tape, the route tape the run replayed')
        args.out.write_text(json.dumps(capture(Path(args.input), args.tape), indent=2)+'\n')
        print('wrote', args.out)
        return
    binary, world = build(tuple(int(v) for v in args.start.split(',')) if args.start else START)
    # The binary runs from ROOT, where world.txt's pack paths are rooted, so
    # every path handed to it is made absolute first.
    absolute = lambda value: str(Path(value).resolve()) if value and value != '-' else '-'
    trace = absolute(args.trace)
    summary = absolute(args.summary)
    if args.mode in ('replay', 'run'):
        subprocess.run([str(binary), str(world), args.mode, absolute(args.input), trace, summary],
                       check=True, cwd=ROOT)
        return
    if not args.out:
        parser.error('search needs --out')
    prefix = args.input
    if prefix == 'default':
        generated = ROOT/'.hkpsx/boss-sim/prefix.txt'
        generated.write_text(' '.join(str(m) for m in default_prefix()))
        prefix = str(generated)
    prefix = absolute(prefix)
    masks = [0]
    for entry in args.alphabet.split(','):
        value = 0
        for name in entry.split('+'):
            if name not in BUTTONS:
                raise ValueError('unknown button: '+name)
            value |= BUTTONS[name]
        masks.append(value)
    subprocess.run([str(binary), str(world), 'search', prefix, str(args.segment),
                    str(args.horizon), str(args.beam), absolute(args.out),
                    ','.join(str(m) for m in masks), trace, summary], check=True, cwd=ROOT)
    if args.route:
        chosen = [int(v) for v in args.out.read_text().split()]
        args.route.write_text(masks_to_route(tape_masks(chosen)))


if __name__ == '__main__':
    sys.exit(main())
