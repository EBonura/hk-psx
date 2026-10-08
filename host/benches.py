"""RestBench placements: trigger box, seat position and the Knight's sit clips.

Source `Bench Control` FSM (Town RestBench, level7): the hero body inside the
bench trigger and UP pressed start the rest; Rest Burst heals, sets the
respawn marker (respawnType 1) and saves; jump/attack/left/right/up/down get
the Knight off through `Get Off`. Map, charm prompt, sleep and the tilting
benches are not cooked.
"""
BENCH_CLIPS = ('Sit', 'Sit Idle', 'Get Off')


def bench_sources(sc, bounds):
    result = []
    for sid, (typ, tree) in sc.objects.items():
        if typ != 'RestBench':
            continue
        gid = tree['m_GameObject']['m_PathID']
        if not sc.active(gid):
            continue
        position = sc.point(gid)
        trigger = None
        for _, (kind, col) in sc.objects.items():
            if kind != 'BoxCollider2D' or col['m_GameObject']['m_PathID'] != gid or not col['m_IsTrigger']:
                continue
            off = col['m_Offset']; size = col['m_Size']
            points = [sc.point(gid, x + off['x'], y + off['y'])[:2] for x, y in
                      [(-size['x'] / 2, -size['y'] / 2), (size['x'] / 2, size['y'] / 2)]]
            trigger = [min(points[0][0], points[1][0]), min(points[0][1], points[1][1]),
                       max(points[0][0], points[1][0]), max(points[0][1], points[1][1])]
        if trigger is None:
            continue
        if not (trigger[0] <= bounds[2] and trigger[2] >= bounds[0] and trigger[1] <= bounds[3] and trigger[3] >= bounds[1]):
            continue
        fsms = [d['fsm']['name'] for _, (kind, d) in sc.objects.items() if kind == 'PlayMakerFSM' and d['m_GameObject']['m_PathID'] == gid]
        if 'Bench Control' not in fsms:
            continue
        result.append({'source': f'{sc.file.name.split("/")[-1]}:{sid}', 'name': sc.gos[gid]['m_Name'],
                       'position': [position[0], position[1]], 'bounds': trigger,
                       'limitations': ['Sit/Sit Idle/Get Off only: no map, charm prompt, sleep or bench tilt; respawn stands at the seat instead of waking on it']})
    return result
