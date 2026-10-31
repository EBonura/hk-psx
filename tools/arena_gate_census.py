#!/usr/bin/env python3
"""Every `BG Control` arena gate of the source and whether the cook runs it.

    .venv/bin/python tools/arena_gate_census.py [SCENE ...]

Selected by FSM (not layer). For each gate: its layer, the FSM family its digest belongs to
(`family`, `bone` or `OTHER`) and the verdict of `breakables.battle_gates`, which is what the
arena gate table is built from. REFUSED rows are the gates the cook leaves out of the table:
read them before adding an arena.
"""
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

import breakables  # noqa: E402
from false_knight import fsm_digest  # noqa: E402
from scene import Scene, _build_settings  # noqa: E402
from source import Source  # noqa: E402


def main():
    source = Source()
    files = _build_settings(source)
    tally = Counter()
    for name in sys.argv[1:] or sorted(files):
        try:
            sc = Scene(source, files[name])
        except Exception:  # unreadable scenes are the census's blind spot, not its subject
            continue
        errors = []
        accepted = {g['source'] for g in breakables.battle_gates(sc, errors)}
        refused = {e['id']: e['error'] for e in errors}
        for index, (kind, tree) in sorted(sc.objects.items()):
            if kind != 'PlayMakerFSM' or tree['fsm']['name'] != breakables.BG_CONTROL_FSM:
                continue
            gid = tree['m_GameObject']['m_PathID']
            if gid not in sc.gos or not sc.active(gid):
                continue
            digest = fsm_digest(tree['fsm'], (breakables.BG_CONTROL_PLACEMENT_VARIABLE,))
            family = {breakables.BG_CONTROL_SHA256: 'family', breakables.BONE_GATE_SHA256: 'bone'}.get(digest, 'OTHER')
            source_id = f'{Path(sc.file.name).name}:{index}'
            verdict = 'accepted' if source_id in accepted else f'REFUSED: {refused.get(source_id, "?")}'
            tally[(family, verdict.split(':')[0])] += 1
            print(f"{name:24} {sc.gos[gid]['m_Name']:24} layer {sc.gos[gid]['m_Layer']:2} {family:6} {verdict}")
    print(dict(tally))


if __name__ == '__main__':
    main()
