#!/usr/bin/env python3
"""Bridge PS1 route tapes and traces to the original-game reference runner.

The PS1 side records one PXITAPE2 sample per port-1 poll (tools/validate.py
poll_tape) and a per-route-tick trace (route.csv from tools/replay_cue.py, its
RAM watch columns named by the link map). The original side takes a
`test_frame,buttons` CSV that starts once the hero accepts input
(tools/reference_game.py, docs/ORIGINAL_REFERENCE.md) and writes state.csv.

Subcommands:
  tape-to-csv  window of a .pxtape -> reference input CSV (change rows only)
  csv-to-tape  reference input CSV -> .pxtape, after a lead-in of idle polls
  settle       insert idle polls into a .pxtape before a window, so the PS1
               Knight is grounded and idle when the compared inputs start
  compare      PS1 route.csv + link map vs reference state.csv, aligned on
               an anchor frame each side, reports position deltas

Both games run their simulation at 60 Hz here (psx-tick on the PS1, Unity's
captureDeltaTime of 1/60 in the reference), so one poll of gameplay input is
one reference test frame. route.csv rows are VBlanks: `port1_polls` is read
at whatever point of the frame the row is taken, and the PS1 simulates in
catch-up bursts, so a row's poll count and the state it shows can be a tick
apart either way. Guests that export HK_SIM_TICKS (ticks simulated, one tape
sample each) and HK_SIM_PAD (the last tick's buttons) are keyed exactly:
the tick count is tied to the tape by the one offset under which every row
around the window shows the buttons the tape holds there. Loads pause PS1 polling; the window must not span
one. Scaled time (hit pauses) differs between the two: compare on the
reference's `time_scale`-aware clock only where the route has no hits, or
read the per-frame report rather than the summary.
"""
import argparse
import csv
import json
import struct
import sys
from pathlib import Path

MAGIC = b'PXITAPE2'
# The masks the reference driver's virtual device maps today
# (docs/ORIGINAL_REFERENCE.md "Input and state control"): Start, the D-pad,
# Circle (cast/Focus), Cross (jump), Square (nail). L1/R1/Triangle drive the
# port's dash, dream nail and quick map and have no reference binding yet.
REFERENCE_MASK = 0x0008 | 0x00F0 | 0x2000 | 0x4000 | 0x8000
ONE = 65536


def read_tape(path):
    data = Path(path).read_bytes()
    if data[:8] != MAGIC:
        raise ValueError(f'{path}: not a PXITAPE2 tape')
    count, _flags = struct.unpack_from('<II', data, 8)
    body = data[16:]
    if len(body) != count * 6:
        raise ValueError(f'{path}: {len(body)} sample bytes for {count} samples')
    return [struct.unpack_from('<H', body, i * 6)[0] for i in range(count)]


def write_tape(path, masks):
    Path(path).write_bytes(MAGIC + struct.pack('<II', len(masks), 0)
                           + b''.join(struct.pack('<HBBBB', m, 128, 128, 128, 128) for m in masks))


def masks_to_rows(masks, drop_unmapped=False):
    """Change-compressed `test_frame,buttons` rows; frame 0 is always written."""
    rows = []
    previous = None
    for frame, mask in enumerate(masks):
        extra = mask & ~REFERENCE_MASK
        if extra and not drop_unmapped:
            raise ValueError(f'frame {frame}: buttons 0x{extra:04x} have no reference binding')
        mask &= REFERENCE_MASK
        if mask != previous:
            rows.append((frame, mask))
            previous = mask
    return rows


def read_rows(path):
    rows = []
    with Path(path).open() as stream:
        lines = [line for line in stream if line.strip() and not line.lstrip().startswith('#')]
    for row in csv.DictReader(lines):
        value = row['buttons'].strip()
        rows.append((int(row['test_frame']), int(value, 16) if value.lower().startswith('0x') else int(value)))
    frames = [f for f, _ in rows]
    if frames != sorted(set(frames)) or (frames and frames[0] < 0):
        raise ValueError(f'{path}: frames must be nonnegative and strictly increasing')
    return rows


def rows_to_masks(rows, length):
    masks = [0] * length
    for index, (frame, mask) in enumerate(rows):
        end = rows[index + 1][0] if index + 1 < len(rows) else length
        for f in range(frame, min(end, length)):
            masks[f] = mask
    return masks


def map_symbols(map_path):
    """Name -> 'ram_xxxxxxxx' route.csv column, from a psoxide-link map."""
    columns = {}
    for line in Path(map_path).read_text().splitlines():
        parts = line.split()
        if len(parts) == 5 and parts[4].startswith('HK_'):
            columns[parts[4]] = 'ram_' + parts[0].lower().rjust(8, '0')
    return columns


def signed(value):
    value = int(value)
    return value - (1 << 32) if value >= 1 << 31 else value


def ps1_track(route_csv, map_path):
    """Per route tick: the row's poll count, and with HK_SIM_TICKS the tick
    count and last consumed buttons (`align_ticks` turns them into samples)."""
    cols = map_symbols(map_path)
    need = ('HK_PLAYER_X', 'HK_PLAYER_Y')
    missing = [n for n in need if n not in cols]
    if missing:
        raise ValueError(f'link map lacks {missing}')
    track = []
    with Path(route_csv).open() as stream:
        for row in csv.DictReader(stream):
            if cols['HK_PLAYER_X'] not in row:
                raise ValueError(f'route.csv does not watch HK_PLAYER_X ({cols["HK_PLAYER_X"]})')
            entry = {'poll': int(row['port1_polls']), 'tick': int(row['route_tick']),
                     'x': signed(row[cols['HK_PLAYER_X']]) / ONE,
                     'y': signed(row[cols['HK_PLAYER_Y']]) / ONE}
            if cols.get('HK_PLAYER_FACING') in row:
                entry['facing'] = signed(row[cols['HK_PLAYER_FACING']])
            if cols.get('HK_SIM_TICKS') in row and cols.get('HK_SIM_PAD') in row:
                entry['ticks'] = int(row[cols['HK_SIM_TICKS']])
                entry['pad'] = int(row[cols['HK_SIM_PAD']])
            track.append(entry)
    return track


def align_ticks(track, tape, anchor, window=300, reach=16):
    """Re-key rows on the tape sample their last tick consumed: sample =
    offset + tick count, one sample per tick. The offset is the one, within
    `reach` of the rows' own poll counts, under which every row from just
    before `anchor` to `window` samples after it shows exactly the buttons
    the tape holds at its sample; a catch-up burst can hide a short press, so
    the whole window decides, not one edge. Returns the re-keyed rows, or
    None without a tick count or when no offset fits the window."""
    if not track or 'ticks' not in track[0]:
        return None
    rows = [r for r in track if anchor - 30 <= r['poll'] <= anchor + window]
    if not rows or not any((tape[i] ^ tape[i - 1]) & REFERENCE_MASK for i in range(max(anchor - 30, 1), min(anchor + window, len(tape)))):
        return None
    guess = sorted(r['poll'] - r['ticks'] for r in rows)[len(rows) // 2]

    def fits(offset):
        return all(0 <= offset + r['ticks'] < len(tape)
                   and (r['pad'] ^ tape[offset + r['ticks']]) & REFERENCE_MASK == 0 for r in rows)
    offsets = sorted((o for o in range(guess - reach, guess + reach + 1) if fits(o)), key=lambda o: abs(o - guess))
    if not offsets:
        return None
    offset = offsets[0]
    return [dict(row, poll=offset + row['ticks'], exact=True) for row in track]


def reference_track(state_csv):
    track = []
    with Path(state_csv).open() as stream:
        for row in csv.DictReader(stream):
            frame = int(row['test_frame'])
            if frame < 0:
                continue
            entry = {'frame': frame, 'x': float(row['x']), 'y': float(row['y']),
                     'scene': row['scene'], 'time_scale': float(row.get('time_scale') or 1)}
            if row.get('facing_right') in ('True', 'False'):
                entry['facing'] = 1 if row['facing_right'] == 'True' else -1
            track.append(entry)
    return track


def compare(ps1, reference, ps1_anchor_poll, reference_anchor_frame, frames, tolerance, tape=None):
    """Per-frame deltas from the two anchors. PS1 rows are taken at the last
    route tick of each poll, i.e. after that poll's simulation ran; with
    HK_SIM_POLL, at the last route tick showing that sample's tick."""
    exact = align_ticks(ps1, tape, ps1_anchor_poll) if tape is not None else None
    if exact is not None:
        ps1 = exact
    by_poll = {}
    for row in ps1:
        by_poll[row['poll']] = row
    by_frame = {row['frame']: row for row in reference}
    report = []
    first = None
    worst = 0.0
    for k in range(frames):
        a = by_poll.get(ps1_anchor_poll + k)
        b = by_frame.get(reference_anchor_frame + k)
        if a is None or b is None:
            report.append({'k': k, 'missing': 'ps1' if a is None else 'reference'})
            continue
        dx, dy = a['x'] - b['x'], a['y'] - b['y']
        d = max(abs(dx), abs(dy))
        worst = max(worst, d)
        if first is None and d > tolerance:
            first = k
        entry = {'k': k, 'ps1': [a['x'], a['y']], 'reference': [b['x'], b['y']],
                 'dx': round(dx, 4), 'dy': round(dy, 4), 'time_scale': b['time_scale']}
        if 'facing' in a and 'facing' in b:
            entry['facing'] = [a['facing'], b['facing']]
        if tape is not None and 'pad' in a and ps1_anchor_poll + k < len(tape):
            entry['pad_matches_tape'] = (a['pad'] & REFERENCE_MASK) == (tape[ps1_anchor_poll + k] & REFERENCE_MASK)
        report.append(entry)
    paused = sum(1 for r in report if r.get('time_scale', 1) != 1)
    deltas = sorted(max(abs(r['dx']), abs(r['dy'])) for r in report if 'dx' in r)
    return {'frames': frames, 'tolerance': tolerance, 'worst': round(worst, 4),
            'median': round(deltas[len(deltas) // 2], 4) if deltas else None,
            'first_over_tolerance': first, 'reference_scaled_time_frames': paused,
            'keyed_on': 'HK_SIM_TICKS' if exact is not None else 'port1_polls',
            'facing_mismatches': sum(1 for r in report if 'facing' in r and r['facing'][0] != r['facing'][1]),
            'pad_mismatches': sum(1 for r in report if r.get('pad_matches_tape') is False),
            'missing': sum(1 for r in report if 'missing' in r), 'rows': report}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest='cmd', required=True)
    a = sub.add_parser('tape-to-csv')
    a.add_argument('tape')
    a.add_argument('csv')
    a.add_argument('--from-poll', type=int, required=True, help='first poll with hero control')
    a.add_argument('--to-poll', type=int, help='end poll (exclusive)')
    a.add_argument('--drop-unmapped', action='store_true')
    s = sub.add_parser('settle')
    s.add_argument('tape')
    s.add_argument('out')
    s.add_argument('--at-poll', type=int, required=True, help='insert before this poll (its mask must be idle)')
    s.add_argument('--idle', type=int, required=True, help='idle polls to insert')
    b = sub.add_parser('csv-to-tape')
    b.add_argument('csv')
    b.add_argument('tape')
    b.add_argument('--frames', type=int, required=True)
    b.add_argument('--lead-in', type=int, default=0, help='idle polls before frame 0')
    c = sub.add_parser('compare')
    c.add_argument('--route', required=True)
    c.add_argument('--map', required=True)
    c.add_argument('--state', required=True)
    c.add_argument('--ps1-anchor-poll', type=int, required=True)
    c.add_argument('--reference-anchor-frame', type=int, default=0)
    c.add_argument('--frames', type=int, required=True)
    c.add_argument('--tolerance', type=float, default=0.05)
    c.add_argument('--out')
    c.add_argument('--tape', help='the PS1 tape, to check HK_SIM_PAD against it')
    args = p.parse_args(argv)
    if args.cmd == 'tape-to-csv':
        masks = read_tape(args.tape)
        end = len(masks) if args.to_poll is None else args.to_poll
        if not 0 <= args.from_poll < end <= len(masks):
            raise SystemExit('poll window outside the tape')
        rows = masks_to_rows(masks[args.from_poll:end], args.drop_unmapped)
        with Path(args.csv).open('w') as out:
            out.write(f'# from {Path(args.tape).name} polls {args.from_poll}..{end}\n')
            out.write('test_frame,buttons\n')
            for frame, mask in rows:
                out.write(f'{frame},0x{mask:x}\n')
    elif args.cmd == 'settle':
        masks = read_tape(args.tape)
        if not 0 <= args.at_poll <= len(masks) or args.idle < 0:
            raise SystemExit('settle point outside the tape')
        if args.at_poll < len(masks) and masks[args.at_poll] & REFERENCE_MASK:
            raise SystemExit(f'poll {args.at_poll} holds buttons 0x{masks[args.at_poll]:04x}; settle on an idle poll')
        write_tape(args.out, masks[:args.at_poll] + [0] * args.idle + masks[args.at_poll:])
    elif args.cmd == 'csv-to-tape':
        masks = [0] * args.lead_in + rows_to_masks(read_rows(args.csv), args.frames)
        write_tape(args.tape, masks)
    else:
        result = compare(ps1_track(args.route, args.map), reference_track(args.state),
                         args.ps1_anchor_poll, args.reference_anchor_frame, args.frames, args.tolerance,
                         read_tape(args.tape) if args.tape else None)
        text = json.dumps(result, indent=1)
        if args.out:
            Path(args.out).write_text(text + '\n')
        summary = {k: v for k, v in result.items() if k != 'rows'}
        print(json.dumps(summary, indent=1))


if __name__ == '__main__':
    sys.exit(main())
