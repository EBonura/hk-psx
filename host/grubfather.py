"""The Grubfather's rewards, for game/src/grubfather.rs.

Crossroads_38's `Grub King` carries one FSM, `King Control`, and his 46
`Reward N` children carry one each (`grub_reward_geo`,
`grub_rewards_heartpiece` or `grub_rewards_shinyitem`). None of them talks: he
has no `Conversation Control`, only a dream line behind an inactive `Dream
Dialogue`. So there is no text to cook, and the state machine is small enough
to port by hand. Its actions are not what the scene script kernel runs
(Tk2dPlayAnimationWithEvents and its frame trigger, GetPosition with
FloatInRange every frame, CheckGeoCap, SendEventByName into a child found by a
built string, FlingObjectsFromGlobalPool, SendRandomEvent), and adding them
would mean the kernel learning the animation clock, the coin pool and a
string-built child lookup for one owner.

What `King Control` does, in the order grubfather.rs runs it:

* `Pause` waits a frame. `Init Check`: `fatGrubKing` hides him (`Fat`); no grub
  saved is `Alone`; `grubsCollected == grubRewards` is `All Given`; otherwise
  `Beckon`.
* `Beckon`: every frame, the Knight's x below 0 or within [30, 42] is GIVE.
* `Cheer?`: the first time this scene load (`Given`), the `Cheer` clip to its
  end; then `Cheer Pause` (0.5 s), then `Gift Anim`.
* `Gift Anim`: the `Gift` clip; its trigger frame is `Activate Reward`:
  `grubRewards` + 1, and GIVE to `Reward <grubRewards>`. `Final Reward?`: below
  46 goes on to `Recover`; at 46 it sets `finalGrubRewardCollected` and stops.
* `Recover` waits for `Gift` to end, `Recover Pause` 1.3 s, `Recheck` reads the
  two counts again: all given is `All Given`, otherwise `At cap?` (the coin
  pool full waits another `Recover Pause`) and the next `Gift Anim`.

A `grub_reward_geo` reward flings its `Geo` from its own position as single
coins, each a weighted random draw of 1, 5 or 25 that falls back to the
smaller coin when what is left cannot pay it. The heart piece and the shiny
items are activated under `Loose Rewards`, flung, and picked up; their grants
are read the way host/pickups.py reads every other shiny (`items._shiny_grant`).

Every number, name and transition below is read from the install and asserted,
so an install whose Grubfather changed fails this cook.
"""
import hashlib, json, re
from source import Source, ROOT, dump
from scene import Scene
from shops import Catalog
from focus import fsm_variables, action_parameters
from props import _actions, _scalar, _variables, _kids
from geo import q16
from title_cards import ascii_text
import language

SCENE = 'Crossroads_38'
OBJECT = 'Grub King'
FSM = 'King Control'
OUTPUT = ROOT / 'data/grubfather.rs'
REPORT = ROOT / '.hkpsx/grubfather.json'
TICKS = 60
TRANSITIONS = {
    'Pause': [('FINISHED', 'Init Check')],
    'Init Check': [('ALONE', 'Alone'), ('ALL GIVEN', 'All Given'), ('FINISHED', 'Beckon'), ('FAT', 'Fat')],
    'Beckon': [('GIVE', 'Cheer?')],
    'Cheer?': [('FINISHED', 'Cheer Pause'), ('GIVE', 'Gift Anim')],
    'Cheer Pause': [('FINISHED', 'Gift Anim')],
    'Gift Anim': [('FINISHED', 'Activate Reward')],
    'Activate Reward': [('FINISHED', 'Final Reward?')],
    'Final Reward?': [('FINISHED', 'Recover')],
    'Recover': [('FINISHED', 'Recover Pause')],
    'Recover Pause': [('FINISHED', 'Recheck')],
    'Recheck': [('ALONE', ''), ('ALL GIVEN', 'All Given'), ('FINISHED', 'At cap?')],
    'At cap?': [('TRUE', 'Recover Pause'), ('FALSE', 'Gift Anim')],
}
GEO_TRANSITIONS = {
    'Idle': [('GIVE', 'Remaining?')],
    'Remaining?': [('END', 'End'), ('SMALL', 'Small'), ('MED', 'Med'), ('LARGE', 'Large')],
    'Large': [('LESS', 'Med'), ('FINISHED', 'Remaining?')],
    'Med': [('LESS', 'Small'), ('FINISHED', 'Remaining?')],
    'Small': [('FINISHED', 'Remaining?')],
}
# game/src/pickups.rs `Grant` variants, by what `items._shiny_grant` and the
# shiny's own state say it gives.
# Pale Ore is the shiny's `Trinket Num` 14 (`Ore`), so it rides the trinket grant.
GRANTS = {'charm': 'Grant::Charm({})', 'trinket': 'Grant::Trinket({})', 'rancid_egg': 'Grant::RancidEgg',
          'mask_shard': 'Grant::MaskShard'}


def _transitions(state):
    return [(t['fsmEvent']['name'], t['toState']) for t in state['transitions']]


def _check(states, contract, who):
    for name, expected in contract.items():
        if name not in states or _transitions(states[name]) != expected:
            raise ValueError(f'{who} {name} moved: {_transitions(states.get(name, {"transitions": []}))}')


def _fsm_of(sc, gid, name):
    found = [t['fsm'] for _c, (typ, t) in sc.objects.items()
             if typ == 'PlayMakerFSM' and t['m_GameObject']['m_PathID'] == gid and t['fsm']['name'] == name]
    if len(found) != 1:
        raise ValueError(f'{sc.gos[gid]["m_Name"]} carries {len(found)} {name}')
    return found[0]


def _value(v):
    return v['value'] if isinstance(v, dict) else v


def _one(state, action):
    found = _actions(state, action)
    if len(found) != 1:
        raise ValueError(f'{state["name"]} has {len(found)} {action}')
    return found[0]


def _clip_ticks(library, name, upto=None):
    """Ticks until the clip's frame `upto` shows, or until a play-once clip ends."""
    clip = next(c for c in library['clips'] if c['name'] == name)
    frames = len(clip['frames']) if upto is None else upto
    ticks = frames * TICKS / clip['fps']
    if ticks != int(ticks):
        raise ValueError(f'{name} does not end on a tick')
    return int(ticks), clip


def _king(sc, source, states, gid):
    """The FSM's tests and times, asserted against the reading in the module doc."""
    check = states['Init Check']
    fat = _one(check, 'PlayerDataBoolTest')
    if (_scalar(fat['boolName']), fat['isTrue']) != ('fatGrubKing', 'FAT'):
        raise ValueError('Init Check no longer hides him on fatGrubKing')
    for state in ('Init Check', 'Recheck'):
        reads = [_scalar(a['intName']) for a in _actions(states[state], 'GetPlayerDataInt')]
        if reads != ['grubsCollected', 'grubRewards']:
            raise ValueError(f'{state} reads {reads}')
        compares = [(a['integer2'].get('name') or _value(a['integer2']), a['equal']) for a in _actions(states[state], 'IntCompare')]
        if compares != [(0, 'ALONE'), ('Rewards Given', 'ALL GIVEN')]:
            raise ValueError(f'{state} compares {compares}')
    below = _one(states['Beckon'], 'FloatCompare')
    if (_value(below['float2']), below['lessThan'], below['everyFrame']) != (0.0, 'GIVE', True):
        raise ValueError('Beckon no longer gives below x 0')
    rng = _one(states['Beckon'], 'FloatInRange')
    beckon = [_value(rng['lowerValue']), _value(rng['upperValue'])]
    if rng['trueEvent'] != 'GIVE' or not rng['everyFrame']:
        raise ValueError('Beckon range no longer gives every frame')
    given = _one(states['Cheer?'], 'BoolTest')
    if (given['boolVariable']['name'], given['isTrue']) != ('Given', 'FINISHED'):
        raise ValueError('Cheer? no longer skips the cheer once Given')

    def played(state, action, field):
        a = _one(states[state], action)
        return _scalar(a['clipName']), a[field]
    if played('Cheer?', 'Tk2dPlayAnimationWithEvents', 'animationCompleteEvent') != ('Cheer', 'FINISHED'):
        raise ValueError('Cheer? no longer waits for Cheer to end')
    if played('Gift Anim', 'Tk2dPlayAnimationWithEvents', 'animationTriggerEvent') != ('Gift', 'FINISHED'):
        raise ValueError('Gift Anim no longer rewards on the Gift trigger')
    if _one(states['Recover'], 'Tk2dWatchAnimationEvents')['animationCompleteEvent'] != 'FINISHED':
        raise ValueError('Recover no longer waits for Gift to end')
    waits = {s: _value(_one(states[s], 'Wait')['time']) for s in ('Cheer Pause', 'Recover Pause')}
    increment = _one(states['Activate Reward'], 'IncrementPlayerDataInt')
    if _scalar(increment['intName']) != 'grubRewards':
        raise ValueError('Activate Reward no longer counts grubRewards')
    final = _one(states['Final Reward?'], 'IntCompare')
    final_count = _value(final['integer2'])
    if final['lessThan'] != 'FINISHED' or final['equal'] or final['greaterThan']:
        raise ValueError('Final Reward? moved')
    if _scalar(_one(states['Final Reward?'], 'SetPlayerDataBool')['boolName']) != 'finalGrubRewardCollected':
        raise ValueError('Final Reward? no longer sets finalGrubRewardCollected')
    cap = _one(states['At cap?'], 'CheckGeoCap')
    if (cap['IsAtCapEvent'], cap['IsUnderCapEvent']) != ('TRUE', 'FALSE'):
        raise ValueError('At cap? moved')
    animator = next(t for _c, (typ, t) in sc.objects.items()
                    if typ == 'tk2dSpriteAnimator' and t['m_GameObject']['m_PathID'] == gid)
    library = source.read(source.ref(sc.file, animator['library']))
    cheer, _ = _clip_ticks(library, 'Cheer')
    gift_end, gift = _clip_ticks(library, 'Gift')
    trigger = [i for i, f in enumerate(gift['frames']) if f.get('triggerEvent')]
    if len(trigger) != 1:
        raise ValueError(f'Gift triggers on frames {trigger}')
    gift_trigger, _ = _clip_ticks(library, 'Gift', trigger[0])
    for name, seconds in waits.items():
        if seconds * TICKS - round(seconds * TICKS) > 1e-3:
            raise ValueError(f'{name} waits {seconds} s, not whole ticks')
    return {'beckon': beckon, 'cheer': cheer, 'cheer_pause': round(waits['Cheer Pause'] * TICKS),
            'gift_trigger': gift_trigger, 'gift_rest': gift_end - gift_trigger,
            'recover_pause': round(waits['Recover Pause'] * TICKS), 'final': final_count}


def _geo_reward(fsm):
    """A `grub_reward_geo`'s Geo, its fling and its draw weights."""
    states = {s['name']: s for s in fsm['states']}
    _check(states, GEO_TRANSITIONS, 'grub_reward_geo')
    data = states['Remaining?']['actionData']
    i = next(i for i, n in enumerate(data['actionNames']) if n.endswith('SendRandomEvent'))
    pairs = action_parameters(data, i)
    params = [v for k, v in pairs if k is None]
    events = [p for p in params if isinstance(p, str)]
    weights = [_value(p) for p in params if isinstance(p, dict)]
    delay = [_value(v) for k, v in pairs if k == 'delay']
    if events != ['SMALL', 'MED', 'LARGE'] or len(weights) != 3 or delay != [0.0]:
        raise ValueError(f'Remaining? draws {events} {weights} after {delay}')
    percent = [round(w * 100) for w in weights]
    if sum(percent) != 100 or any(abs(w * 100 - p) > 1e-4 for w, p in zip(weights, percent)):
        raise ValueError(f'draw weights {weights} are not whole percents')
    coins = []
    for name in ('Large', 'Med'):
        less = _one(states[name], 'IntCompare')
        add = _one(states[name], 'IntAdd')
        if less['lessThan'] != 'LESS' or _value(add['add']) != -_value(less['integer2']):
            raise ValueError(f'{name} no longer pays what it tests')
        coins.append(_value(less['integer2']))
    if _value(_one(states['Small'], 'IntAdd')['add']) != -1:
        raise ValueError('Small no longer pays 1')
    flings = {name: _one(states[name], 'FlingObjectsFromGlobalPool') for name in ('Large', 'Med', 'Small')}
    shapes = {tuple(_value(f[k]) for k in ('spawnMin', 'spawnMax', 'speedMin', 'speedMax', 'angleMin', 'angleMax',
                                           'originVariationX', 'originVariationY')) for f in flings.values()}
    if len(shapes) != 1:
        raise ValueError('the three coin flings disagree')
    shape = shapes.pop()
    if shape[:2] != (1, 1) or shape[6:] != (0.0, 0.0):
        raise ValueError(f'a coin fling is no longer one coin from the reward: {shape}')
    return {'geo': _variables(fsm)['Geo'], 'weights': percent, 'coins': [1] + coins[::-1],
            'speed': list(shape[2:4]), 'angle': list(shape[4:6])}


def _item_grant(sc, item_name, loose):
    """A reward's item under `Loose Rewards`: its Grant and its message line."""
    from items import _shiny_grant
    from pickups import _name
    kids = _kids(sc, loose)
    if item_name not in kids:
        raise ValueError(f'Loose Rewards has no {item_name}')
    gid = kids[item_name]
    fsms = {t['fsm']['name']: t['fsm'] for _c, (typ, t) in sc.objects.items()
            if typ == 'PlayMakerFSM' and t['m_GameObject']['m_PathID'] == gid}
    if 'Heart Container Control' in fsms:
        return ('mask_shard', 0), ''
    fsm = fsms['Shiny Control']
    grant = _shiny_grant(fsm, _variables(fsm))
    route, state = grant['route'], grant.get('state', '')
    ui = language.sheet(sc.source, 'UI')
    if route == 'charm' and grant.get('charm_id'):
        kind = ('charm', grant['charm_id'])
    elif route == 'trinket' and state.startswith('Trink '):
        kind = ('trinket', grant['trinket_num'])
    elif route == 'trinket' and state == 'Egg':
        kind = ('rancid_egg', 0)
    elif route == 'trinket' and state == 'Ore':
        if [w['field'] for w in grant['writes'] if w['field']] != ['ore']:
            raise ValueError(f'the Ore state writes {grant["writes"]}')
        state_ = next(s for s in fsm['states'] if s['name'] == 'Ore')
        keys = [_scalar(a['convName']) for a in _actions(state_, 'GetLanguageString')]
        if len(keys) != 1 or keys[0] not in ui:
            raise ValueError(f'Ore names {keys}')
        return ('trinket', grant['trinket_num']), ascii_text(ui[keys[0]])
    else:
        raise ValueError(f'{item_name} grants {grant}, which the port has no Grant for')
    return kind, _name(fsm, kind, ui)


def cook():
    source = Source()
    catalog = Catalog(source)
    regions = json.loads((ROOT / 'data/regions.json').read_text())
    scene = next(s for s in regions['scenes'] if s['scene_name'] == SCENE)
    sc = Scene(source, catalog.files[SCENE])
    gid = next(g for g, go in sc.gos.items() if go['m_Name'] == OBJECT)
    fsm = _fsm_of(sc, gid, FSM)
    states = {s['name']: s for s in fsm['states']}
    _check(states, TRANSITIONS, FSM)
    king = _king(sc, source, states, gid)
    parent = fsm_variables(fsm)['Rewards Parent']['m_PathID']
    rewards_parent = _kids(sc, gid).get('Rewards Parent')
    if rewards_parent != parent:
        raise ValueError('Rewards Parent is no longer the Grub King\'s child')
    loose = [g for g, go in sc.gos.items() if go['m_Name'] == 'Loose Rewards']
    if len(loose) != 1:
        raise ValueError('expected one Loose Rewards')
    rewards, fling, origin = {}, None, None
    for name, kid in _kids(sc, parent).items():
        number = int(re.fullmatch(r'Reward (\d+)', name)[1])
        fsms = [t['fsm'] for _c, (typ, t) in sc.objects.items()
                if typ == 'PlayMakerFSM' and t['m_GameObject']['m_PathID'] == kid]
        if len(fsms) != 1:
            raise ValueError(f'{name} carries {len(fsms)} FSMs')
        f = fsms[0]
        if f['name'] == 'grub_reward_geo':
            geo = _geo_reward(f)
            shape = {k: geo[k] for k in ('weights', 'coins', 'speed', 'angle')}
            if fling not in (None, shape):
                raise ValueError(f'{name} flings differently from the others')
            fling = shape
            at = list(sc.point(kid)[:2])
            if origin not in (None, at[0]):
                raise ValueError(f'{name} stands apart from the other Geo rewards')
            origin = at[0]
            rewards[number] = ('geo', geo['geo'], at[1])
        elif f['name'] in ('grub_rewards_heartpiece', 'grub_rewards_shinyitem'):
            item = _variables(f).get('Piece Name') or _variables(f).get('Item Name')
            rewards[number] = ('item',) + _item_grant(sc, item, loose[0])
        else:
            raise ValueError(f'{name} runs {f["name"]}')
    if sorted(rewards) != list(range(1, king['final'] + 1)):
        raise ValueError(f'rewards {sorted(rewards)} do not run 1 to {king["final"]}')

    def rust_reward(r):
        if r[0] == 'geo':
            return f'Reward::Geo({r[1]},{q16(r[2])})'
        (kind, value), name = r[1], r[2]
        return f'Reward::Item({GRANTS[kind].format(value)},{json.dumps(name)})'

    lines = ['// Generated by host/grubfather.py from the local Windows source; no retail payload is embedded.',
             f'pub const GRUBFATHER_SCENE:usize={scene["scene_id"]};',
             f'pub const GRUBFATHER_SOURCE:u32={gid};',
             '/// `Beckon`\'s FloatInRange on the Knight\'s x (Q16 world units).',
             f'pub const BECKON_X:[i32;2]=[{q16(king["beckon"][0])},{q16(king["beckon"][1])}];',
             f'pub const CHEER_TICKS:u16={king["cheer"]};',
             f'pub const CHEER_PAUSE_TICKS:u16={king["cheer_pause"]};',
             f'pub const GIFT_TRIGGER_TICKS:u16={king["gift_trigger"]};',
             f'pub const GIFT_REST_TICKS:u16={king["gift_rest"]};',
             f'pub const RECOVER_PAUSE_TICKS:u16={king["recover_pause"]};',
             '/// The x every `grub_reward_geo` flings from (Q16 world units); each has its own y.',
             f'pub const REWARD_X:i32={q16(origin)};',
             '/// FlingObjectsFromGlobalPool: speed and angle ranges (Q16).',
             f'pub const REWARD_SPEED:[i32;2]=[{q16(fling["speed"][0])},{q16(fling["speed"][1])}];',
             f'pub const REWARD_ANGLE:[i32;2]=[{q16(fling["angle"][0])},{q16(fling["angle"][1])}];',
             '/// `Remaining?`\'s SendRandomEvent weights in percent, small, medium, large.',
             f'pub const DRAW_PERCENT:[u8;3]=[{",".join(map(str, fling["weights"]))}];',
             '/// What each draw pays: the coin values `Small`, `Med` and `Large` take off.',
             f'pub const COIN_VALUES:[u16;3]=[{",".join(map(str, fling["coins"]))}];',
             '/// `Reward 1` to `Reward N`, in order; `grubRewards` after the gift is the number.',
             f'pub static REWARDS:[Reward;{len(rewards)}]=[' + ','.join(rust_reward(rewards[n]) for n in sorted(rewards)) + '];']
    OUTPUT.write_text('\n'.join(lines) + '\n')
    items = {n: [r[1][0], r[1][1], r[2]] for n, r in rewards.items() if r[0] == 'item'}
    dump(REPORT, {'format': 'HKGRUB01', 'scene': SCENE, 'scene_id': scene['scene_id'], 'source': gid,
                  'king': king, 'origin': origin, 'fling': fling,
                  'geo': {n: r[1:] for n, r in rewards.items() if r[0] == 'geo'}, 'items': items,
                  'sha256': hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
                  'not_reproduced': ['his art and clips (he is not drawn)', 'his voice and the gift sounds',
                                     'Fat (fatGrubKing has no producer in the port)', 'StoryRecord_visited',
                                     'the items are handed over at once rather than flung out of Loose Rewards',
                                     'the dream dialogue']})
    print(f'Grubfather: {len(rewards)} rewards, {len(items)} items '
          f'({", ".join(f"{n}: {v[0]}" for n, v in sorted(items.items()))})', flush=True)


if __name__ == '__main__':
    cook()
