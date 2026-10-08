"""Cook the compiled FSM programs the guest actually runs (P10 step 4).

`host/script_ir.py` is the compiler and stays one: it answers "what does this
FSM mean", never "what ships". This is the cooker on top, and it makes three
decisions the compiler has no business making.

**Only a program with ops is cooked.** 255 of the 1,876 script-side FSM
instances compile, and 245 of those compile to nothing at all: `Spawn Offset`,
`PlayMaker Unity 2D`, `RespawnTriggerFSM` and the Hollow Shade markers are
single empty states. Cooking them would put 245 instances in the guest's pool
to tick zero ops forever, and would make the shipped count look like coverage it
is not. Ten instances have behaviour, and those are the bank.

**A definition is cooked once.** Instances outnumber definitions everywhere in
this game, and the ten live instances share ten programs only because the seven
`Area Title Controller` placements each resolve their own owner id into their
constant pool. Identical programs still collapse.

**A trigger needs bounds, or the instance is refused.** A `HeroTrigger` op asks
the host whether the hero body is inside an object's volume, and the host can
only answer for an object whose collider this cook could reduce to a world
rectangle. An instance whose volume has no bounds would run with a trigger that
never fires, which is the silent failure the IR exists to avoid.

No retail payload is embedded; the generated table and report are build outputs.
"""
import sys as _sys
from pathlib import Path as _Path
# actor_sources runs every recognizer, and host/husk_guard.py reads the pooled
# shockwave through host/false_knight_art.py, which imports from tools/.
_sys.path.insert(0, str(_Path(__file__).resolve().parents[1] / 'tools'))
import collections, json, math, re
from pathlib import Path

from source import Source, ROOT, dump
from scene import Scene
from script_ir import (BOOL_TEST, HERO_TRIGGER, INT_COMPARE, NATIVE, NATIVE_CONST, NEXT_FRAME,
                       PD_BOOL_TEST, PD_SET, SEND_EVENT, WAIT, SceneObjects, Unsupported,
                       compile_fsm, scene_fsms, _variable_values)

# The opcode names the guest's `hk_sim::script::Code` variants carry, in the
# order script_ir.py numbers them. The generated table names the variant rather
# than the number so a future reorder is a compile error in the guest instead of
# a bank that runs the wrong op.
CODES = ('Nop', 'Wait', 'SetBool', 'SetInt', 'BoolTest', 'IntCompare', 'SendEvent',
         'NextFrameEvent', 'Native', 'NativeConst', 'PlayerDataGet', 'PlayerDataSet',
         'PlayerDataBoolTest', 'HeroTrigger')


# Which operand of an op is an event id, by opcode. An op not named here sends
# nothing; the write-only ones are in `writes`.
EVENT_OPERANDS = {WAIT: (3,), BOOL_TEST: (3, 4), INT_COMPARE: (4,), SEND_EVENT: (2,),
                  NEXT_FRAME: (2,), PD_BOOL_TEST: (3, 4), HERO_TRIGGER: (4,)}


def sent_events(op):
    """The event ids one op can send. Zero is 'no event' and never sent."""
    return tuple(e for e in (op[i] for i in EVENT_OPERANDS.get(op[0], ())) if e)


def writes(op):
    """Whether an op changes something outside its own FSM's variables."""
    return op[0] in (PD_SET, NATIVE, NATIVE_CONST)


def field_slots():
    """The PlayerData reserve the save record holds, read out of the guest.

    The record cannot follow a generated field count: changing its length
    invalidates every committed card fixture. So the reserve is fixed in
    `game/src/save.rs` and a bank that outgrows it fails the build here, rather
    than shipping a record that silently holds some of the fields. Read rather
    than repeated, the way tools/migrate_cards.py reads the same file.
    """
    text = (ROOT / 'game/src/save.rs').read_text()
    found = re.search(r'pub const SCRIPT_FIELD_SLOTS: usize = (\d+);', text)
    if not found:
        raise ValueError('cannot read SCRIPT_FIELD_SLOTS out of game/src/save.rs')
    return int(found.group(1))


def field_identity(names):
    """fnv1a-32 over the ordered field list, as the record's slot identity.

    Slot order is this cook's. A record written by one bank and read by another
    would put one field's value on another's, which the record checksum cannot
    see because the bytes are intact. The guest compares this and drops the
    values when it disagrees.
    """
    value = 0x811c9dc5
    for byte in '\0'.join(names).encode():
        value = ((value ^ byte) * 16777619) & 0xffffffff
    return value


def q(value):
    """A world coordinate as the Q16 the guest compares hero bodies in."""
    if not math.isfinite(float(value)) or abs(float(value)) > 512:
        raise ValueError(f'world coordinate outside Q16 bounds: {value}')
    return int(round(float(value) * 65536))


def _axis_aligned(sc, gid):
    """Whether this object's world transform leaves x and y unrotated.

    Only the circle colliders need it. A box or polygon reduces to the AABB of
    its transformed corners whatever the rotation, but a circle's silhouette
    does not follow its four cardinal points once the frame is turned.
    """
    m = sc.world(sc.go_transform[gid])
    return abs(m[0][1]) < 1e-6 and abs(m[1][0]) < 1e-6


def volume_bounds(sc, gid):
    """The world rectangle of an object's enabled 2D trigger colliders.

    The union, because three of the admitted owners carry two boxes, and Unity
    raises the callback for whichever one the Knight crosses. Refuses rather
    than approximating: a collider this cannot reduce leaves the instance out of
    the bank instead of giving it a volume that is not the authored one.
    """
    points = []
    for component in sc.gos[gid]['m_Component']:
        ref = component['component']
        if ref['m_FileID']:
            continue
        typename, tree = sc.objects.get(ref['m_PathID'], (None, None))
        if not (typename or '').endswith('Collider2D'):
            continue
        if not tree['m_Enabled'] or not tree['m_IsTrigger']:
            continue
        off = tree['m_Offset']
        if typename == 'BoxCollider2D':
            x, y = tree['m_Size']['x'] / 2, tree['m_Size']['y'] / 2
            local = [(-x, -y), (x, -y), (x, y), (-x, y)]
        elif typename == 'PolygonCollider2D':
            local = [(p['x'], p['y']) for path in tree['m_Points']['m_Paths'] for p in path]
        elif typename == 'CircleCollider2D':
            if not _axis_aligned(sc, gid):
                raise ValueError('rotated CircleCollider2D trigger')
            r = tree['m_Radius']
            local = [(-r, -r), (r, -r), (r, r), (-r, r)]
        else:
            raise ValueError(f'unsupported trigger collider {typename}')
        points += [sc.point(gid, x + off['x'], y + off['y'])[:2] for x, y in local]
    if not points:
        raise ValueError('no enabled trigger collider')
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    return [q(min(xs)), q(min(ys)), q(max(xs)), q(max(ys))]


def initial_variables(fsm, program):
    """Each variable slot's serialized initial value, as the i32 a slot holds.

    A slot the IR cannot read as an integer starts at zero, and that is not an
    approximation: the only ops that read a slot are the bool, int and object
    ones, an unset object reference is `NO_OBJECT` which is zero, and a float or
    string variable has no op that can reach it. Compiler temporaries, which
    have no declared variable at all, start at zero for the same reason: every
    one of them is written before it is read.
    """
    values = _variable_values(fsm)
    slots = [0] * program['var_count']
    for name, slot in program['variables'].items():
        value = values.get(name)
        if isinstance(value, bool):
            slots[slot] = int(value)
        elif isinstance(value, int):
            slots[slot] = value
    return slots


def start_state(fsm, program):
    """The state index PlayMaker begins in, which is not always the first.

    `Instance::new` starts at zero, so the cooker carries this and the guest
    seats the instance on it. An FSM started from the wrong state is a wrong
    answer with no symptom at the seam, which is why it is cooked rather than
    assumed.
    """
    try:
        return program['state_names'].index(fsm['startState'])
    except ValueError:
        raise ValueError(f'start state {fsm["startState"]!r} is not one of this FSM\'s states')


def collect(source, scene_files):
    """Every live program, its instances and the volumes they watch."""
    defs, definitions = {}, []
    instances, volumes, fields = [], {}, []
    refused, empty, inactive, scanned = [], 0, 0, 0
    for file, scene_name in scene_files.items():
        sc, fsms = scene_fsms(source, file)
        resolver = SceneObjects(sc)
        scene_id = scene_files[file]
        for _path_id, owner_id, fsm, runs_natively in fsms:
            if runs_natively:
                continue
            scanned += 1
            try:
                program = compile_fsm(fsm, owner_id=owner_id, objects=resolver)
            except Unsupported:
                continue
            except Exception:
                continue
            if not program['ops']:
                empty += 1
                continue
            if not sc.active(owner_id):
                # An object the cooked world does not build is an FSM the
                # original never starts. Nothing in this slice can activate one,
                # so ticking it would run behaviour the source has gated off.
                inactive += 1
                continue
            identity = f'{Path(sc.file.name).name}:{owner_id}'
            try:
                watched = sorted({program['constants'][op[2]] for op in program['ops']
                                  if op[0] == HERO_TRIGGER})
                for source_id in watched:
                    key = (scene_id, source_id)
                    if key not in volumes:
                        # The op carries the owner's masked source id and the
                        # scene's path ids are that id unmasked, so the lookup
                        # back to the object is the identity for every id the
                        # cooker has produced.
                        volumes[key] = volume_bounds(sc, source_id)
                start = start_state(fsm, program)
                initial = initial_variables(fsm, program)
            except (ValueError, KeyError) as error:
                refused.append({'source': identity, 'scene': scene_name,
                                'fsm': fsm['name'], 'error': str(error)})
                continue
            key = (tuple(program['ops']), tuple(program['states']), tuple(program['transitions']),
                   tuple(program['constants']), tuple(program['player_data_fields']),
                   start, tuple(initial))
            if key not in defs:
                defs[key] = len(definitions)
                definitions.append({'fsm': fsm['name'], 'ops': program['ops'],
                                    'states': program['states'],
                                    'transitions': program['transitions'],
                                    'constants': program['constants'],
                                    'var_count': program['var_count'],
                                    'start': start, 'initial': initial,
                                    'field_names': program['player_data_fields'],
                                    'state_names': program['state_names'],
                                    'events': program['events']})
            instances.append({'scene': scene_id, 'scene_name': scene_name,
                              'def': defs[key], 'fsm': fsm['name'],
                              'source': identity, 'source_id': owner_id,
                              'owner': sc.gos[owner_id]['m_Name']})
    for definition in definitions:
        # A definition that writes nothing and whose every send names an event
        # no state can take is a dead authored action. It is still cooked,
        # because leaving it out would be the cooker inventing a reason the
        # source does not give, but it is never counted as behaviour: seven of
        # the ten are `Area Title Controller`, one state with no transitions at
        # all, whose title the source puts up from its C# component instead.
        takeable = {event for event, _target in definition['transitions']}
        definition['acts'] = any(writes(op) or (set(sent_events(op)) & takeable)
                                 for op in definition['ops'])
    for definition in definitions:
        for name in definition['field_names']:
            if name not in fields:
                fields.append(name)
        definition['fields'] = [fields.index(name) for name in definition['field_names']]
    return {'definitions': definitions, 'instances': instances, 'fields': fields,
            'volumes': [{'scene': scene, 'source_id': source_id, 'bounds': bounds}
                        for (scene, source_id), bounds in sorted(volumes.items())],
            'scanned': scanned, 'empty': empty, 'inactive': inactive, 'refused': refused}


def emit(bank):
    """The generated Rust the guest includes, plus the byte cost of each table."""
    lines = ['// Generated script programs; no retail payload is embedded.',
             '// One record per cooked FSM definition; instances bind to it and own only',
             '// their variables. host/cook_scripts.py is the cooker.']
    base, bases = 0, []
    for instance in bank['instances']:
        bases.append(base)
        base += bank['definitions'][instance['def']]['var_count']
    parts = {}
    parts['definitions'] = 'pub static SCRIPT_DEFS:&[ScriptDef]=&[' + ''.join(
        'ScriptDef{states:&[' + ','.join(
            'State{first_op:%d,op_count:%d,first_transition:%d,transition_count:%d}' % s
            for s in d['states']) + '],transitions:&[' + ','.join(
            'Transition{event:%d,target:%d}' % t for t in d['transitions']) + '],ops:&[' + ','.join(
            'Op{code:Code::%s,flags:%d,a:%d,b:%d,c:%d}' % (CODES[o[0]], o[1], o[2], o[3], o[4])
            for o in d['ops']) + '],constants:&[' + ','.join(
            str(c) for c in d['constants']) + '],var_count:%d,start:%d,initial:&[' % (
            d['var_count'], d['start']) + ','.join(
            str(v) for v in d['initial']) + '],fields:&[' + ','.join(
            str(f) for f in d['fields']) + ']},'
        for d in bank['definitions']) + '];'
    parts['instances'] = 'pub static SCRIPT_INSTANCES:&[ScriptInstance]=&[' + ''.join(
        'ScriptInstance{scene:%d,def:%d,var_base:%d,source_id:%d},' % (
            i['scene'], i['def'], v, i['source_id'] & 0x7fff_ffff)
        for i, v in zip(bank['instances'], bases)) + '];'
    parts['volumes'] = 'pub static SCRIPT_VOLUMES:&[ScriptVolume]=&[' + ''.join(
        'ScriptVolume{scene:%d,source_id:%d,bounds:[%s]},' % (
            v['scene'], v['source_id'] & 0x7fff_ffff, ','.join(str(b) for b in v['bounds']))
        for v in bank['volumes']) + '];'
    parts['fields'] = ('pub static SCRIPT_FIELD_NAMES:&[&str]=&['
                       + ','.join(json.dumps(name) for name in bank['fields']) + '];')
    lines += [f'pub const SCRIPT_VARS:usize={base};',
              f"pub const SCRIPT_FIELD_FNV:u32={field_identity(bank['fields'])};",
              parts['definitions'], parts['instances'], parts['volumes'], parts['fields']]
    return '\n'.join(lines) + '\n', {name: len(text) for name, text in parts.items()}


def linked_bytes(bank):
    """What the tables cost in the guest's linked image, table by table.

    Counted from the declared record widths rather than from the generated text,
    because the text is not what the linker stores. A `&'static [T]` field is a
    fat pointer, eight bytes on this target, which is why a definition record is
    dominated by its five slices rather than by its three scalars.
    """
    defs = bank['definitions']
    slice_fields = 5  # states, transitions, ops, constants, initial, fields is six
    per_def = slice_fields * 8 + 8 + 8  # five slices, the fields slice, and the scalars
    rows = {
        'definition records': len(defs) * per_def,
        'states': sum(len(d['states']) for d in defs) * 8,
        'transitions': sum(len(d['transitions']) for d in defs) * 4,
        'ops': sum(len(d['ops']) for d in defs) * 8,
        'constants': sum(len(d['constants']) for d in defs) * 4,
        'initial variables': sum(len(d['initial']) for d in defs) * 4,
        'field maps': sum(len(d['fields']) for d in defs) * 2,
        'instance records': len(bank['instances']) * 12,
        'volume records': len(bank['volumes']) * 24,
        'field names': sum(len(name) + 8 for name in bank['fields']),
    }
    pool = sum(bank['definitions'][i['def']]['var_count'] for i in bank['instances'])
    rows['variable pool (bss)'] = pool * 4
    rows['instance pool (bss)'] = len(bank['instances']) * 24
    rows['field store (bss)'] = len(bank['fields']) * 4
    rows['overlap bits (bss)'] = len(bank['volumes']) * 2
    return rows


def cook(source=None):
    source = source or Source()
    scenes = {e['file']: e['scene_id']
              for e in json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']}
    names = {e['file']: e['scene_name']
             for e in json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']}
    bank = collect(source, scenes)
    for instance in bank['instances']:
        instance['scene_name'] = names[[f for f, s in scenes.items()
                                        if s == instance['scene']][0]]
    slots = field_slots()
    if len(bank['fields']) > slots:
        raise ValueError(f"the bank needs {len(bank['fields'])} PlayerData slots and the save "
                         f'record reserves {slots}; raise SCRIPT_FIELD_SLOTS in game/src/save.rs '
                         'and migrate the card fixtures')
    code, text_bytes = emit(bank)
    (ROOT / 'data/scripts.rs').write_text(code)
    rows = linked_bytes(bank)
    acting = [i for i in bank['instances'] if bank['definitions'][i['def']]['acts']]
    report = {
        'scanned_instances': bank['scanned'],
        'cooked_instances': len(bank['instances']),
        'cooked_definitions': len(bank['definitions']),
        'instances_that_can_act': len(acting),
        'acting_instances': [f"{i['scene_name']} {i['fsm']} on {i['owner']}" for i in acting],
        'compiled_but_empty': bank['empty'],
        'compiled_but_inactive': bank['inactive'],
        'volumes': len(bank['volumes']),
        'player_data_fields': bank['fields'],
        'player_data_slots_reserved': slots,
        'player_data_field_fnv': field_identity(bank['fields']),
        'refused': bank['refused'],
        'linked_bytes': rows,
        'linked_bytes_total': sum(rows.values()),
        'generated_text_bytes': text_bytes,
        'per_scene': dict(collections.Counter(i['scene_name'] for i in bank['instances'])),
        'instances': [{k: v for k, v in i.items() if k != 'def'} | {'definition': i['def']}
                      for i in bank['instances']],
        'definitions': [{'fsm': d['fsm'], 'states': d['state_names'], 'start': d['start'],
                         'ops': len(d['ops']), 'events': d['events'],
                         'fields': d['field_names'], 'var_count': d['var_count']}
                        for d in bank['definitions']],
        'limitations': [
            'Only instances with at least one op are cooked. 245 of the 255 compiling '
            'instances compile to an empty program, so cooking them would tick nothing '
            'and would report as coverage.',
            'An instance is bound to its source scene, not to a region. With at most two '
            'live instances in any scene the whole scene\'s set is ticked while it is '
            'resident; a region binding is the next step and needs a world-bank object '
            'kind, which needs host/regions.py to emit the rows.',
            'PlayerData rides in the bench save record, in a fixed reserve of slots rather '
            'than in this bank\'s own count, because the record length cannot follow a '
            'generated table. Slot order is this cook\'s, so the record carries the field '
            'list identity and the guest drops the values when it is running a different '
            'bank rather than reading one field as another.',
            'Non-integer variables start at zero. No compiled op reads a slot as anything '
            'but a bool, an int or an object reference, and zero is NO_OBJECT, so this is '
            'exact for what can be read rather than an approximation.',
            'The cooked bank is not a replay claim. Nothing here has been compared against '
            'the original running the same FSM.',
        ],
    }
    dump(ROOT / '.hkpsx/script-bank.json', report)
    print(f"Script bank: {len(bank['instances'])} instances over {len(bank['definitions'])} "
          f"definitions, {sum(len(d['ops']) for d in bank['definitions'])} ops, "
          f"{len(bank['volumes'])} trigger volumes, {len(bank['fields'])} PlayerData fields")
    print(f"  {bank['empty']} compiling instances have no ops and are not cooked, "
          f"{bank['inactive']} sit on inactive objects; "
          f"{len(bank['refused'])} refused at cook time")
    print(f"  {len(acting)} of them can act; the rest send events their definition "
          f"has no transition for, which is what the source does too")
    print(f"  linked cost {sum(rows.values())} bytes: "
          + ', '.join(f'{name} {value}' for name, value in rows.items() if value))
    for instance in bank['instances']:
        mark = 'acts' if bank['definitions'][instance['def']]['acts'] else '    '
        print(f"  {mark} {instance['scene_name']:24} {instance['fsm']:22} on {instance['owner']}")
    return report


if __name__ == '__main__':
    cook()
