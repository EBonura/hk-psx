"""The charm catalogue, its equip rules and the effects the port can express.

`host/items.py` already recovers the forty charms' identity: id, sprite name,
notch cost and the four PlayerData fields each one owns. This module is what
P16 steps 2 and 4 to 6 need on top of that, and it cooks a guest table.

Three separate authorities are read rather than assumed.

`UI Charms`, the inventory's own PlayMaker FSM, is the equip contract. `Slot
Open?` refuses to start an equip unless `charmSlotsFilled < charmSlots`, so a
full notch board is never overcharmable. `Check Points` then adds `charmCost_N`
to `charmSlotsFilled` and branches on `filled > slots`. `Overcharm Check` takes
that branch: with `canOvercharm` the equip goes through and `overcharmed` is
set, and without it the attempt is counted, refused, and `Fail Back` gives the
notches back, until the fifth attempt sets `canOvercharm` and breaks through.
Unequipping runs `Return Points` (cost back) and `End Overcharm?`, which clears
`overcharmed` only once `filled <= slots`. `Broken?` refuses the three fragile
charms while their `brokenCharm_N` is set, and `Royal?` refuses 36 in its bound
quest states.

The effects come from the assembly and from the two FSMs that own a number:
`HeroController::SoulGain` (11 SOUL a hit, +3 with 20, +8 with 21),
`HeroController::CharmUpdate` (23 raises maxHealth by 2), the serialized
HeroController constants (`INVUL_TIME_STAL`, `RECOIL_DURATION_STAL`,
`GRUB_SOUL_MP`), `Set Slash Damage` (25 multiplies nail damage by 1.5) and
`Fury` (6 multiplies by 1.75 at one mask). A charm whose effect none of these
express is cooked as `Effect::None`, and the guest refuses to equip it rather
than pretending it does something; `unimplemented` in the report says why for
each one.

Nothing here embeds retail payload: the generated table carries the localized
strings the same way `data/read_points.rs` and `data/npc_lines.rs` already do,
and the report goes to .hkpsx.
"""
import hashlib, json, math, re, sys
from pathlib import Path

import dnfile
from dncil.cil.body.reader import read_method_body_from_bytes
from dncil.clr.token import Token
from PIL import Image

import items
import language
from combat import ticks
from cook import native_sprite
from focus import action_fields
from materials import quantize_alpha_coverage
from read_points import wrap_page
from source import ROOT, dump

CHARMS = items.CHARMS
# The inventory panel this feeds is narrower and shorter than the tablet
# reader's: the charm list keeps the rows above it, so the description gets the
# bottom six lines of the box. Six is what the longest cooked description needs
# at this width, measured over all forty rather than chosen.
DESC_WIDTH = 264
DESC_LINES = 6
# Charm rows drawn at once, above the description. The guest takes this from the
# cooked table so the icon measurement below and the panel can never disagree.
VISIBLE_ROWS = 6
# Charm ids the source treats specially, asserted against the FSM below rather
# than trusted from here.
FRAGILE = (23, 24, 25)
BOUND = 36

# The panel geometry, in the units the guest draws in. The description's line
# pitch and the cooked font strip's glyph box are what everything else is
# measured against; `panel_layout` derives the rest, because the icon size is
# whatever the leftover scanlines allow rather than a number chosen here.
DESC_PITCH = 11
GLYPH_PX = 12
SCREEN_LINES = 240
PANEL_X = 14
PANEL_W = 292
PANEL_BORDER = 2
PANEL_GAP = 3
# What the board keeps clear of the screen edge. The cheats page already sits
# eight lines off the bottom, so eight is what this port has decided is safe.
PANEL_MARGIN = 8
# Icon sizes the measurement below costs out. The panel takes the largest one
# it can seat; nothing here picks a number to make a budget work.
ICON_SIZES = (16, 24, 32)
# x320..335 y488..491, out of the fourteen rows host/shade.py reserved at y482.
# The Shade uses two and the Knight's ability clips reserve four, so these are
# the next four free; y492 went to the hit flash and y493..494 to host/props.py
# since (tests/test_clut_rows.py keeps the count). That block sits
# outside the per-view CLUT banks, which is why these four cost no texture slot
# in any view. docs/BUDGET.md carries the split.
ICON_CLUT = (320, 488, 16, 1)
# Icons sharing one resident palette. Fewer per row is better colour and more
# CLUT rows; forty over four rows is what the free block affords.
ICONS_PER_PALETTE = 10
# The animation slot caps a single upload, as it does for the Shade.
SLOT_BYTES = 2048


def advances():
    """The guest's glyph advances, from the strip `host/read_points.py` cooked.

    The charm panel draws with the same Perpetua strip as the tablet reader, so
    wrapping has to use the same table. Re-deriving it here would mean loading
    the font a second time and risking a different answer than the one the
    guest actually draws with, so the cooked table is the authority.
    """
    text = (ROOT / 'data/read_points.rs').read_text()
    match = re.search(r'pub const ADVANCES:\[u8;95\]=\[([0-9,]+)\];', text)
    assert match, 'data/read_points.rs no longer carries the glyph advances'
    table = [int(v) for v in match.group(1).split(',')]
    assert len(table) == 95 and all(1 <= v <= 12 for v in table), 'unexpected advance table'
    return table


def _short(name):
    return name.rsplit('.', 1)[-1]


def _fsm_named(source, name):
    """One PlayMakerFSM by name in resources.assets, refusing an ambiguous match."""
    file = source.file('resources.assets')
    needle = name.encode('utf8')
    found = []
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour' or needle not in o.get_raw_data():
            continue
        if source.typename(o) != 'PlayMakerFSM':
            continue
        fsm = source.read(o)['fsm']
        if fsm['name'] == name:
            found.append((o, fsm))
    assert len(found) == 1, f'{len(found)} FSMs named {name!r}'
    return found[0]


def _states(fsm):
    return {s['name']: s for s in fsm['states']}


def _actions(state, kind):
    d = state['actionData']
    return [action_fields(d, i) for i, n in enumerate(d['actionNames'])
            if _short(n) == kind and d['actionEnabled'][i]]


def _plain(value):
    """A PlayMaker scalar that must be a literal, not a variable reference."""
    assert isinstance(value, dict) and not value.get('useVariable'), f'{value} is not a literal'
    return value['value']


def _named(value):
    """The variable a PlayMaker field names, or its literal when it names none."""
    if isinstance(value, dict) and 'useVariable' in value:
        return value['name'] if value['useVariable'] else value['value']
    return value


def equip_rules(source):
    """The equip, overcharm and refusal contract, read out of `UI Charms`."""
    _, fsm = _fsm_named(source, 'UI Charms')
    states = _states(fsm)

    def compare(state, first, second):
        a = _actions(states[state], 'IntCompare')
        assert len(a) == 1, f'{state} no longer holds one IntCompare'
        assert _named(a[0]['integer1']) == first and _named(a[0]['integer2']) == second, \
            f'{state} no longer compares {first} against {second}'
        return a[0]

    # Gate: an equip may only start while at least one notch is open, and the
    # test cancels on equal as well as greater, so a full board never overcharms.
    gate = compare('Slot Open?', 'Notches Filled', 'Notches')
    assert gate['equal'] == 'CANCEL' and gate['greaterThan'] == 'CANCEL' and not gate['lessThan'], \
        'Slot Open? no longer refuses a full notch board'

    # Spend: the charm's own cost is added before the overcharm branch is taken.
    spend = _actions(states['Check Points'], 'PlayerDataIntAdd')
    assert len(spend) == 1 and _named(spend[0]['intName']) == 'charmSlotsFilled' \
        and _named(spend[0]['amount']) == 'Notch Cost', 'Check Points no longer spends the notch cost'
    over = compare('Check Points', 'Notches Filled', 'Notches')
    assert over['greaterThan'] == 'OVER' and not over['equal'] and not over['lessThan'], \
        'Check Points no longer takes the overcharm branch only above the notch count'

    # Overcharm: allowed outright once canOvercharm is set; otherwise counted,
    # refused twice, cracked twice and broken through on the fifth attempt.
    check = states['Overcharm Check']
    allowed = _actions(check, 'PlayerDataBoolTest')
    assert len(allowed) == 1 and _named(allowed[0]['boolName']) == 'canOvercharm' \
        and allowed[0]['isTrue'] == 'OVERCHARM' and not allowed[0]['isFalse'], \
        'Overcharm Check no longer gates on canOvercharm'
    step = _actions(check, 'IntAdd')
    assert len(step) == 1 and _named(step[0]['intVariable']) == 'Overcharm Attempts' \
        and _plain(step[0]['add']) == 1, 'Overcharm Check no longer counts one attempt'
    refuse = _actions(check, 'IntCompare')
    assert len(refuse) == 1 and _named(refuse[0]['integer1']) == 'Overcharm Attempts' \
        and refuse[0]['equal'] == 'CANCEL' and refuse[0]['lessThan'] == 'CANCEL', \
        'Overcharm Check no longer cancels the early attempts'
    quiet_attempts = _plain(refuse[0]['integer2'])
    switches = _actions(check, 'IntSwitch')
    thresholds = {}
    for switch in switches:
        assert _named(switch['intVariable']) == 'Overcharm Attempts'
        fields = [v for k, v in switch.items() if k != 'intVariable']
        values = [_plain(v) for v in fields if isinstance(v, dict)]
        events = [v for v in fields if isinstance(v, str)]
        assert len(values) == 1 and len(events) == 1, 'Overcharm Check switch shape changed'
        thresholds[values[0]] = events[0]
    assert set(thresholds.values()) == {'OVERCHARM CRACK 1', 'OVERCHARM CRACK 2', 'OVERCHARM BREAK'}, \
        f'Overcharm Check no longer cracks then breaks: {thresholds}'
    break_attempt = next(k for k, v in thresholds.items() if v == 'OVERCHARM BREAK')
    unlock = _actions(states['Break'], 'SetPlayerDataBool')
    assert len(unlock) == 1 and _named(unlock[0]['boolName']) == 'canOvercharm' \
        and _plain(unlock[0]['value']) is True, 'Break no longer unlocks overcharming'
    mark = _actions(states['Set Overcharm'], 'SetPlayerDataBool')
    assert any(_named(a['boolName']) == 'overcharmed' and _plain(a['value']) is True for a in mark), \
        'Set Overcharm no longer marks the save overcharmed'

    # Refusal refund: Fail Back negates the cost it just spent and gives it back.
    refund = _actions(states['Fail Back'], 'PlayerDataIntAdd')
    assert len(refund) == 1 and _named(refund[0]['intName']) == 'charmSlotsFilled' \
        and _named(refund[0]['amount']) == 'Notch Cost', 'Fail Back no longer refunds the notch cost'

    # Unequip: the same cost comes back, then overcharm ends once it fits again.
    ret = _actions(states['Return Points'], 'PlayerDataIntAdd')
    assert len(ret) == 1 and _named(ret[0]['intName']) == 'charmSlotsFilled' \
        and _named(ret[0]['amount']) == 'Notch Cost', 'Return Points no longer refunds the notch cost'
    end = compare('End Overcharm?', 'Notches Filled', 'Notches')
    assert end['equal'] == 'END' and end['lessThan'] == 'END' and end['greaterThan'] == 'OVERCHARM', \
        'End Overcharm? no longer clears overcharm only once the notches fit'
    clear = _actions(states['End Overcharm'], 'SetPlayerDataBool')
    assert len(clear) == 1 and _named(clear[0]['boolName']) == 'overcharmed' \
        and _plain(clear[0]['value']) is False, 'End Overcharm no longer clears the flag'

    # Refusals that are about the charm rather than the notches.
    fragile = sorted(_plain(a['int2']) for a in _actions(states['Broken?'], 'IntTestToBool'))
    assert tuple(fragile) == FRAGILE, f'the fragile charms are now {fragile}'
    broken = {_named(a['boolName']) for a in _actions(states['Broken?'], 'GetPlayerDataBool')}
    assert broken == {f'brokenCharm_{n}' for n in FRAGILE}, 'Broken? no longer reads the fragile flags'
    royal = _actions(states['Royal?'], 'IntCompare')
    assert len(royal) == 1 and _named(royal[0]['integer1']) == 'Current Item Number' \
        and _plain(royal[0]['integer2']) == BOUND, f'Royal? no longer guards charm {BOUND}'
    return {
        'fsm': 'UI Charms',
        'equip_requires_open_notch': 'charmSlotsFilled < charmSlots',
        'spend': 'charmSlotsFilled += charmCost_N',
        'overcharm_when': 'charmSlotsFilled > charmSlots',
        'overcharm_allowed_by': 'canOvercharm',
        'refused_attempts_before_first_crack': quiet_attempts,
        'attempt_events': {str(k): v for k, v in sorted(thresholds.items())},
        'overcharm_break_attempt': break_attempt,
        'refund_on_refusal': 'charmSlotsFilled -= charmCost_N',
        'unequip': 'charmSlotsFilled -= charmCost_N, then overcharmed = charmSlotsFilled > charmSlots',
        'fragile_charms': list(FRAGILE),
        'bound_charm': BOUND,
    }


def _method_bodies(assembly, wanted):
    """CIL for named `Type::Method` pairs, with each operand already resolved."""
    pe = dnfile.dnPE(str(assembly))
    tables = pe.net.mdtables
    def operand(value):
        if not isinstance(value, Token):
            return value
        if value.table == 0x70:
            return pe.net.user_strings.get(value.rid).value
        table = tables.tables.get(value.table)
        if not table:
            return value
        row = table.rows[value.rid - 1]
        return str(getattr(row, 'Name', getattr(row, 'TypeName', value)))
    out = {}
    for typ in tables.TypeDef.rows:
        for ref in typ.MethodList:
            method = ref.row
            key = (str(typ.TypeName), str(method.Name))
            if key not in wanted or not method.Rva:
                continue
            body = read_method_body_from_bytes(pe.get_data(method.Rva, 100000))
            out[key] = [(i.opcode.name, operand(i.operand)) for i in body.instructions]
    missing = wanted - set(out)
    assert not missing, f'the assembly lost {missing}'
    return out


_LOAD = {f'ldc.i4.{n}': n for n in range(9)}


def _literal(instruction):
    op, operand = instruction
    if op in _LOAD:
        return _LOAD[op]
    if op in ('ldc.i4', 'ldc.i4.s'):
        return int(operand)
    return None


def soul_per_hit_charms(bodies):
    """The SOUL a nail hit grants, and what 20 and 21 add, from `SoulGain`.

    The method has two arms: below maxMP it charges 11 and above it charges the
    reserve 6, and each arm adds its own pair of charm bonuses. Only the first
    arm matters here, because the port has no SOUL reserve, so this walks until
    the second base literal appears and refuses if the shape moved.
    """
    body = bodies[('HeroController', 'SoulGain')]
    base, bonuses, charm = None, {}, None
    for instruction in body:
        op, operand = instruction
        if op == 'ldfld' and isinstance(operand, str):
            match = re.fullmatch(r'equippedCharm_(\d+)', operand)
            charm = int(match.group(1)) if match else None
            continue
        value = _literal(instruction)
        if value is None:
            continue
        if base is None:
            base = value
            continue
        if charm is None:
            break  # the reserve arm's own base literal; the port has no reserve
        if charm not in bonuses:
            bonuses[charm] = value
    assert base == 11, f'SoulGain no longer charges 11 a hit ({base})'
    assert bonuses == {20: 3, 21: 8}, f'SoulGain charm bonuses are now {bonuses}'
    return base, bonuses


def fragile_heart_bonus(bodies):
    """The masks charm 23 adds, from `CharmUpdate`'s maxHealthBase arithmetic."""
    body = bodies[('HeroController', 'CharmUpdate')]
    start = next(i for i, (op, operand) in enumerate(body)
                 if op == 'ldfld' and operand == 'equippedCharm_23')
    window = body[start:start + 20]
    assert any(op == 'ldfld' and operand == 'brokenCharm_23' for op, operand in window), \
        'CharmUpdate no longer skips a broken charm 23'
    base = next(i for i, (op, operand) in enumerate(window)
                if op == 'ldfld' and operand == 'maxHealthBase')
    bonus = _literal(window[base + 1])
    assert window[base + 2][0] == 'add' and window[base + 3][1] == 'maxHealth' and bonus == 2, \
        f'charm 23 no longer adds a literal 2 to maxHealthBase ({bonus})'
    return bonus


def _multiplier(state, name):
    """The one distinct damage multiplier a state applies, however it writes it."""
    values = [_plain(a['multiplyBy']) for a in _actions(state, 'FloatMultiply')] \
        + [_plain(a['setValue']) for a in _actions(state, 'SetFsmFloat')
           if _named(a['variableName']) == 'Multiplier' and _plain(a['setValue']) != 1.0]
    assert values and len(set(values)) == 1, f'{name} multipliers are {values}'
    return values[0]


def nail_multipliers(source):
    """Charm 25's nail multiplier and charm 6's low-health multiplier."""
    _, slash = _fsm_named(source, 'Set Slash Damage')
    modifier = _states(slash)['Glass Attack Modifier']
    guards = _actions(modifier, 'PlayerDataBoolTrueAndFalse')
    assert len(guards) == 1 and _named(guards[0]['trueBool']) == 'equippedCharm_25' \
        and _named(guards[0]['falseBool']) == 'brokenCharm_25', \
        'Set Slash Damage no longer gates the nail multiplier on an unbroken charm 25'
    strength = _multiplier(modifier, 'Set Slash Damage/Glass Attack Modifier')
    # Fury writes its multiplier into every slash's damages_enemy FSM from
    # `Activate`, and keeps its own gate in `Check HP`.
    _, fsm = _fsm_named(source, 'Fury')
    fury = _multiplier(_states(fsm)['Activate'], 'Fury/Activate')
    check = _states(fsm)['Check HP']
    gate = _actions(check, 'PlayerDataBoolTest')
    assert any(_named(a['boolName']) == 'equippedCharm_6' and a['isFalse'] == 'CANCEL' for a in gate), \
        'Fury no longer gates on charm 6'
    health = _actions(check, 'IntCompare')
    assert len(health) == 1 and _named(health[0]['integer1']) == 'HP' \
        and _plain(health[0]['integer2']) == 1 and health[0]['equal'] == 'FURY', \
        'Fury no longer triggers at exactly one mask'
    return {'fragile_strength': strength, 'fury_of_the_fallen': fury, 'fury_at_health': 1}


def hero_constants(source):
    """HeroController's serialized scalars, ahead of its runtime state block."""
    from UnityPy.helpers import TypeTreeHelper
    file = source.file('resources.assets')
    hero = next(o for o in file.objects.values()
                if o.type.name == 'MonoBehaviour' and source.typename(o) == 'HeroController')
    node = hero._get_typetree_node()
    children = node.m_Children
    node.m_Children = children[:next(i for i, n in enumerate(children) if n.m_Name == 'hero_state')]
    boost = TypeTreeHelper.read_typetree_boost
    try:
        TypeTreeHelper.read_typetree_boost = None
        values = hero.read_typetree(nodes=node, check_read=False)
    finally:
        TypeTreeHelper.read_typetree_boost = boost
        node.m_Children = children
    return hero, values


def stalwart_shell(constants):
    """Charm 4's damage timings, in the same form `host/actors.py` cooks the base.

    `HeroController::Update` swaps RECOIL_DURATION for RECOIL_DURATION_STAL and
    StartRecoil swaps INVUL_TIME for INVUL_TIME_STAL, so the charm's ticks are
    the base formulas with the charmed constant substituted. Keeping the same
    formula means the charm and the base can never disagree about the freeze
    frame `DAMAGE_FREEZE_DOWN` contributes.
    """
    for field in ('INVUL_TIME', 'INVUL_TIME_STAL', 'RECOIL_DURATION', 'RECOIL_DURATION_STAL',
                  'DAMAGE_FREEZE_DOWN'):
        value = constants[field]
        assert math.isfinite(value) and value > 0, f'invalid HeroController scalar {field}'
    assert constants['INVUL_TIME_STAL'] > constants['INVUL_TIME'], 'charm 4 no longer lengthens invulnerability'
    assert constants['RECOIL_DURATION_STAL'] < constants['RECOIL_DURATION'], 'charm 4 no longer shortens recoil'
    down = constants['DAMAGE_FREEZE_DOWN']
    return {
        'invulnerable_ticks': ticks(constants['INVUL_TIME_STAL'] + down),
        'hazard_invulnerable_ticks': ticks(constants['INVUL_TIME_STAL'] / 2 + down),
        'recoil_ticks': ticks(constants['RECOIL_DURATION_STAL']),
        'base_invulnerable_ticks': ticks(constants['INVUL_TIME'] + down),
        'base_recoil_ticks': ticks(constants['RECOIL_DURATION']),
    }


def grubsong(bodies, constants):
    """Charm 3's SOUL on damage, and the amount charm 35 raises it to."""
    body = bodies[('HeroController', 'TakeDamageCharmEffects')]
    charms = [int(m.group(1)) for op, operand in body if op == 'ldfld' and isinstance(operand, str)
              for m in [re.fullmatch(r'equippedCharm_(\d+)', operand)] if m]
    assert charms == [3, 35], f'TakeDamageCharmEffects now reads {charms}'
    fields = [operand for op, operand in body if op == 'ldfld' and operand in
              ('GRUB_SOUL_MP', 'GRUB_SOUL_MP_COMBO')]
    assert fields == ['GRUB_SOUL_MP', 'GRUB_SOUL_MP_COMBO'], 'the Grubsong constants moved'
    alone, combo = constants['GRUB_SOUL_MP'], constants['GRUB_SOUL_MP_COMBO']
    assert isinstance(alone, int) and isinstance(combo, int) and 0 < alone < combo, \
        f'unexpected Grubsong constants {alone}/{combo}'
    return {'alone': alone, 'combo': combo, 'combo_charm': 35}


# Why each charm the cook cannot express is left unequippable. Every entry names
# the port system that is missing, not the charm's flavour.
UNIMPLEMENTED = {
    1: 'no loose-Geo attractor: geo.rs collects on hero overlap only',
    2: 'no map or compass system in the port',
    5: 'no Baldur Shell actor, and Focus has no absorbing shell state',
    6: 'the 1.75 multiplier is chosen per swing against live health, which the '
       'VitalParams composition point cannot see',
    7: 'Focus already cooks only the un-charmed drain rate (Time Per MP Drain UnCH)',
    8: 'Lifeblood masks are granted by lifeblood.rs cocoons, not by a bench charm hook',
    9: 'Lifeblood masks are granted by lifeblood.rs cocoons, not by a bench charm hook',
    10: 'no spore cloud actor and no Defender’s Crest trail',
    11: 'no Fluke spell variant; Vengeful Spirit is the only cast the port has',
    12: 'no contact-damage retaliation path on the hero',
    13: 'nail reach is cooked per slash polygon; no runtime scale applies',
    14: 'the hero bounce is NailResponse, which is not part of VitalParams',
    15: 'enemy knockback is per-actor recoil in hk-sim, with no hero modifier',
    16: 'no dash shadow damage volume',
    17: 'no spore cloud actor',
    18: 'nail reach is cooked per slash polygon; no runtime scale applies',
    19: 'spell damage and size are cooked into FIREBALL_PARAMS',
    22: 'no Hatchling minion actor',
    24: 'enemy Geo drops are cooked per actor with no multiplier hook',
    26: 'no nail art charge in the port',
    27: 'Joni’s Blessing converts every mask to Lifeblood, which the mask HUD '
        'and Focus healing rules do not model',
    28: 'no Focus movement state',
    29: 'no mask regeneration timer',
    30: 'Dream Nail has no charge time or essence economy here',
    31: 'no dash cooldown or ground-dash restriction to relax',
    32: 'no attack cooldown in the port: Nail drives the swing directly',
    33: 'spell cost is a single cooked constant with no charm branch',
    34: 'Focus heals exactly one mask and has no charged variant',
    35: 'no Grubberfly beam projectile',
    36: 'quest states, Void Heart binding and the dream world are outside the slice',
    37: 'run speed is cooked into PARAMS with no runtime modifier',
    38: 'no Dreamshield minion actor',
    39: 'no Weaverling minion actor',
    40: 'no Grimmchild minion actor',
}


def _effect(charm_id, shell, bonuses, heart, nails, grub):
    """The generated `Effect` literal for one charm, or None when unexpressed."""
    if charm_id == 3:
        return ('Effect::SoulOnDamage{alone:%d,combo:%d,combo_charm:%d}'
                % (grub['alone'], grub['combo'], grub['combo_charm']))
    if charm_id == 4:
        return ('Effect::Shell{invulnerable_ticks:%d,hazard_invulnerable_ticks:%d,recoil_ticks:%d}'
                % (shell['invulnerable_ticks'], shell['hazard_invulnerable_ticks'], shell['recoil_ticks']))
    if charm_id in bonuses:
        return 'Effect::SoulPerHit(%d)' % bonuses[charm_id]
    if charm_id == 23:
        return 'Effect::MaxHealth(%d)' % heart
    if charm_id == 25:
        numerator, denominator = _ratio(nails['fragile_strength'])
        return 'Effect::NailScale{numerator:%d,denominator:%d}' % (numerator, denominator)
    return None


def _ratio(multiplier, limit=64):
    """The exact small rational a serialized float32 multiplier stands for.

    The source does `(int)((float)damage * m)`, which truncates, so the guest
    needs the same truncation from integers rather than a fixed-point round.
    """
    for denominator in range(1, limit + 1):
        numerator = multiplier * denominator
        if abs(numerator - round(numerator)) < 1e-6:
            return round(numerator), denominator
    raise ValueError(f'{multiplier} is not a small rational')


def charm_table(catalogue, shell, bonuses, heart, nails, grub, table):
    """Every charm as a cooked guest row: name, cost, wrapped text and effect."""
    rows = []
    for charm in catalogue:
        name_key = min(charm['names'])
        desc_key = min(charm['descriptions'])
        name = charm['names'][name_key].replace('’', "'").replace('‘', "'")
        assert all(32 <= ord(c) <= 126 for c in name), f'charm {charm["id"]} name is not drawable'
        pages = wrap_page(charm['descriptions'][desc_key], table, DESC_WIDTH, DESC_LINES)
        assert len(pages) == 1, f'charm {charm["id"]} description needs {len(pages)} panels'
        effect = _effect(charm['id'], shell, bonuses, heart, nails, grub)
        rows.append({
            'id': charm['id'], 'name': name, 'name_key': name_key, 'description_key': desc_key,
            'cost': charm['notch_cost'], 'lines': pages[0], 'effect': effect,
            'fragile': charm['id'] in FRAGILE, 'bound': charm['id'] == BOUND,
            'playerdata': charm['playerdata'], 'sprite': charm['internal_name'],
            'unimplemented': None if effect else UNIMPLEMENTED[charm['id']],
        })
    unexplained = [r['id'] for r in rows if not r['effect'] and not r['unimplemented']]
    assert not unexplained, f'charms {unexplained} have no effect and no reason'
    return rows


def generated_charms_rs(rows, rules, defaults, layout, art):
    """The guest table. `Charm` and `Effect` are declared in game/src/charms.rs.

    The panel geometry is cooked with the table because the icon size sets the
    row pitch and the row pitch sets everything under it, so a guest that
    hard-coded the layout could disagree with the size that was measured.
    """
    def quoted(value):
        assert '"' not in value and '\\' not in value, f'unquotable cooked text {value!r}'
        return '"' + value + '"'
    entries = []
    for row in rows:
        lines = ','.join(quoted(line) for line in row['lines'])
        entries.append('Charm{name:%s,cost:%d,lines:&[%s],effect:%s,fragile:%s,bound:%s}' % (
            quoted(row['name']), row['cost'], lines, row['effect'] or 'Effect::None',
            'true' if row['fragile'] else 'false', 'true' if row['bound'] else 'false'))
    panel = layout['panel']
    return '\n'.join([
        '// Generated charm catalogue; localized text from the installed language sheets.',
        f'pub const CHARM_COUNT:usize={len(rows)};',
        '/// Charm rows the pause panel draws at once, above the description.',
        f'pub const VISIBLE:usize={VISIBLE_ROWS};',
        f"pub const STARTING_NOTCHES:u8={defaults['charmSlots']};",
        '/// `Overcharm Check` counts a refused attempt and breaks through on this one.',
        f"pub const OVERCHARM_BREAK_ATTEMPT:u8={rules['overcharm_break_attempt']};",
        f'pub const CHARMS:[Charm;CHARM_COUNT]=[' + ','.join(entries) + '];',
        '/// The board, derived by `host/charms.py::panel_layout` against the 240',
        '/// scanlines the screen has. The icon size picked the row pitch.',
        f'pub const PANEL_RECT:(i16,i16,u16,u16)=({panel[0]},{panel[1]},{panel[2]},{panel[3]});',
        f"pub const TITLE_Y:i16={layout['title_y']};",
        f"pub const NOTCH_Y:i16={layout['notch_y']};",
        f"pub const ROW_TOP:i16={layout['row_top']};",
        f"pub const ROW_PITCH:i16={layout['row_pitch']};",
        '/// A row\'s glyphs, centred against the icon beside them.',
        f"pub const ROW_TEXT_OFFSET:i16={layout['row_text_offset']};",
        f"pub const DESC_TOP:i16={layout['description_y']};",
        f"pub const DESC_PITCH:i16={layout['description_pitch']};",
        f"pub const FOOTER_Y:i16={layout['footer_y']};",
        f"pub const CURSOR_X:i16={layout['cursor_x']};",
        f"pub const ICON_X:i16={layout['icon_x']};",
        f"pub const MARK_X:i16={layout['mark_x']};",
        f"pub const NAME_X:i16={layout['name_x']};",
        f"pub const COST_X:i16={layout['cost_x']};",
        '/// Every icon is the same square, so one offset multiply replaces the',
        '/// `Frame` array the Shade and the ability clips need.',
        f"pub const ICON_PX:u16={layout['icon_px']};",
        f"pub const ICON_BYTES:usize={art['icon_bytes']};",
        f"pub const ICON_PALETTE_BYTES:usize={art['palette_bytes']};",
        f"pub const ICON_PALETTE_COUNT:usize={art['palettes']};",
        'pub const ICON_CLUT_RECT:(u16,u16,u16,u16)=(%d,%d,%d,%d);' % tuple(art['clut_rect']),
        'pub const ICON_PALETTE:[u8;CHARM_COUNT]=[' + ','.join(str(c) for c in art['clut_of']) + '];',
    ]) + '\n'


# What the frozen view behind the pause panel keeps hold of: the Knight's pose,
# the nail effect, the Hollow Shade and the Vengeful Spirit ball. Same four
# main.rs reserves every frame.
RESERVED_ANIMATION_SLOTS = 4


def panel_layout(icon_px):
    """Where the charm board's parts sit for an icon this many pixels square.

    The binding constraint on the icon is 240 scanlines, not linked RAM. The
    board has to seat a title, the notch counter, `VISIBLE_ROWS` rows whose
    pitch the icon sets, a `DESC_LINES`-line description and a footer, all
    inside the screen with a margin. Returning None when they do not all fit is
    how the icon size gets derived instead of chosen, and the answer does not
    hang on the gap or the margin: six 24-pixel rows alone are 144 lines, and
    with the text around them that is already past 240 at zero padding.
    """
    pitch = max(icon_px, GLYPH_PX + 2)
    parts = [GLYPH_PX, GLYPH_PX, VISIBLE_ROWS * pitch, DESC_LINES * DESC_PITCH, GLYPH_PX]
    height = sum(parts) + PANEL_GAP * len(parts) + 2 * PANEL_BORDER
    needed = height + 2 * PANEL_MARGIN
    if needed > SCREEN_LINES:
        return {'icon_px': icon_px, 'scanlines_needed': needed, 'fits': False}
    top = (SCREEN_LINES - height) // 2
    y = top + PANEL_BORDER + PANEL_GAP
    at = []
    for part in parts:
        at.append(y)
        y += part + PANEL_GAP
    title, notches, rows, description, footer = at
    # The row's own columns. Only the icon column moves with the icon; the
    # cursor and the notch cost sit where the text-only board already put them.
    cursor_x, cost_x = 20, 268
    icon_x = cursor_x + GLYPH_PX
    mark_x = icon_x + icon_px + 6
    name_x = mark_x + 14
    assert name_x + 100 < cost_x, 'the icon column crowds out the charm names'
    return {
        'icon_px': icon_px, 'scanlines_needed': needed, 'fits': True,
        'panel': [PANEL_X, top, PANEL_W, height],
        'title_y': title, 'notch_y': notches,
        'row_top': rows, 'row_pitch': pitch, 'row_text_offset': (pitch - GLYPH_PX) // 2,
        'description_y': description, 'description_pitch': DESC_PITCH,
        'footer_y': footer,
        'cursor_x': cursor_x, 'icon_x': icon_x, 'mark_x': mark_x,
        'name_x': name_x, 'cost_x': cost_x,
    }


def _charm_icon_sprites(source, catalogue):
    """Every charm's icon Sprite, in charm order, from `CharmIconList`."""
    file = source.file('resources.assets')
    icons = next(source.read(o) for o in file.objects.values()
                 if o.type.name == 'MonoBehaviour' and source.typename(o) == 'CharmIconList')
    return file, [source.ref(file, icons['spriteList'][charm['id']]) for charm in catalogue]


# Below this mean saturation an icon is one of the bone-and-shell discs, whose
# hue is noise; above it the charm has a colour worth keeping.
GREY_SATURATION = .12


def _hue(image):
    """Where one icon sorts, for grouping it with the icons it shares with.

    The near-grey discs go first, darkest to lightest, and the coloured ones
    follow around the wheel. Sorting the greys by hue instead would scatter
    them through the coloured groups on the hue of a handful of pixels.
    """
    import colorsys
    image = image.convert('RGBA')
    # Pillow 14 drops getdata; host/materials.py reads its sheets the same way.
    data = image.get_flattened_data() if hasattr(image, 'get_flattened_data') else image.getdata()
    pixels = [p for p in data if p[3]]
    if not pixels:
        return (0, 0.0)
    mean = [sum(c) / len(pixels) / 255 for c in zip(*((r, g, b) for r, g, b, _ in pixels))]
    hue, saturation, value = colorsys.rgb_to_hsv(*mean)
    return (1, hue) if saturation >= GREY_SATURATION else (0, value)


def _preview(width, height, palette, packed):
    """The quantized sheet as the guest will sample it, for .hkpsx inspection.

    Fifteen colours over ten icons is the one judgement call in the icon cook,
    so it gets looked at rather than assumed, the way host/shade.py's sheets do.
    """
    import struct
    words = struct.unpack('<16H', palette)
    out = Image.new('RGBA', (width, height))
    pixels = out.load()
    for y in range(height):
        for x in range(width):
            index = (packed[y * ((width + 1) // 2) + x // 2] >> ((x & 1) * 4)) & 15
            word = words[index]
            pixels[x, y] = (0, 0, 0, 0) if index == 0 else (
                (word & 31) << 3, ((word >> 5) & 31) << 3, ((word >> 10) & 31) << 3, 255)
    return out


def icon_art(source, catalogue, side):
    """The forty icons as 4bpp texels and resident palettes, the Shade's way.

    Frames live in linked RAM and reach VRAM through the shared 64x64 animation
    slots on demand, so nothing here costs a per-view texture slot. Every icon
    is the same size, which is why the guest table is one palette index each
    rather than the `Frame` array the Shade and the ability clips need.
    """
    _, objects = _charm_icon_sprites(source, catalogue)
    images, art_sources = [], []
    for obj in objects:
        image, _ = native_sprite(obj)
        # Square rather than letterboxed: every source icon is within a sixth of
        # square already, and at this size a border costs more than the squash.
        image = image.resize((side, side), Image.Resampling.LANCZOS)
        # A hard edge. The resample leaves a wide ring of part-transparent
        # pixels around each disc, and `quantize_alpha_coverage` splits its
        # fifteen colours between the coverage classes by pixel count, so that
        # ring was taking half the palette to draw an antialiased outline that
        # sixteen pixels cannot show anyway.
        image.putalpha(image.getchannel('A').point(lambda a: 255 if a >= 128 else 0))
        images.append(image)
        art_sources.append(source.sid(obj))
    stride = (side + 3) // 4 * 2
    assert stride * side <= SLOT_BYTES, 'a charm icon exceeds one animation slot'
    # Grouped by colour, not by id. Ten icons share fifteen colours, so which
    # ten decides whether Grubsong stays green: neighbours in this order want
    # the same colours, neighbours in charm order do not.
    order = sorted(range(len(images)), key=lambda i: _hue(images[i]))
    palettes, clut_of, previews = [], [0] * len(images), []
    texels_of = [None] * len(images)
    for start in range(0, len(order), ICONS_PER_PALETTE):
        group = order[start:start + ICONS_PER_PALETTE]
        sheet = Image.new('RGBA', (side * len(group), side))
        for column, index in enumerate(group):
            sheet.paste(images[index], (column * side, 0))
        sw, sh, palette, packed = quantize_alpha_coverage(sheet, 128)
        palettes.append(palette)
        previews.append(_preview(sw, sh, palette, packed))
        for column, index in enumerate(group):
            x0 = column * side
            texels = bytearray(stride * side)
            for y in range(side):
                for x in range(side):
                    byte = packed[y * ((sw + 1) // 2) + (x + x0) // 2]
                    nibble = (byte >> (((x + x0) & 1) * 4)) & 15
                    texels[y * stride + x // 2] |= nibble << ((x & 1) * 4)
            texels_of[index] = bytes(texels)
            clut_of[index] = len(palettes) - 1
    # Charm order on disc, whatever order they were quantized in: the guest
    # reaches a frame by multiplying its charm index, and carries no offsets.
    blob = b''.join(texels_of)
    palette_bytes = b''.join(palettes)
    return {
        'payload': bytes(palette_bytes) + bytes(blob),
        'palette_bytes': len(palette_bytes), 'palettes': len(palettes),
        'icon_bytes': stride * side, 'clut_of': clut_of, 'art_sources': art_sources,
        'clut_rect': list(ICON_CLUT), 'previews': previews,
    }


def _spare_halfwords():
    """VRAM `hk_cache::residency` leaves unclaimed, for the atlas route's cost.

    The CLUT budget is only half of what art resident in every view has to pay.
    The other half is somewhere to put the texels, and this is what the
    disjointness map says is left.
    """
    text = (ROOT / 'shared/hk-cache/src/residency.rs').read_text()
    return int(re.search(r'pub const SPARE_HALFWORDS\s*:\s*usize\s*=\s*(\d+);', text).group(1))


def icon_measurement(source, catalogue, rows_on_screen):
    """What the forty charm icons would cost, measured rather than guessed.

    Two numbers decide this and neither is a judgement call. `TEXTURE_BUDGET` is
    416 CLUT slots a view, and the pause screen is reachable from every view, so
    the number that applies is the tightest view in the whole world rather than
    any one scene's. `tools/texture_headroom.py` measures it off data/regions.json.
    The way past a small answer is the Hollow Shade route, which pays linked RAM
    and shared 64x64 animation slots instead of atlas slots, so the second
    number is `hk_cache::SLOTS`.

    A third number decides the size once the route is open, and it is not a
    budget at all: `panel_layout` says how many of the screen's 240 scanlines a
    board with icons of each size would need. Linked RAM allows any of the
    three; the screen allows exactly one.
    """
    sys.path.insert(0, str(ROOT / 'tools'))
    from texture_headroom import headroom, view_cluts
    regions = json.loads((ROOT / 'data/regions.json').read_text())
    budget = headroom(regions, view_cluts(regions))
    tightest = min(budget['scenes'].items(), key=lambda kv: kv[1]['headroom'])
    slots = int(re.search(r'pub const SLOTS: usize = (\d+);',
                          (ROOT / 'shared/hk-cache/src/lib.rs').read_text()).group(1))
    _, objects = _charm_icon_sprites(source, catalogue)
    sizes = []
    for charm, obj in zip(catalogue, objects):
        rect = source.read(obj)['m_Rect']
        sizes.append((charm['id'], round(rect['width']), round(rect['height'])))
    # A 4bpp frame is stride*height bytes; one animation slot holds 64x64.
    def linked(side):
        return sum((side + 3) // 4 * 2 * side for _ in sizes)
    layouts = {side: panel_layout(side) for side in ICON_SIZES}
    seatable = [side for side in ICON_SIZES if layouts[side]['fits']]
    chosen = max(seatable) if seatable else None
    needed_slots = rows_on_screen + RESERVED_ANIMATION_SLOTS
    verdict = [f"The atlas route is out and always will be: a pause screen is reachable from "
               f"every view, so the figure that applies is the tightest view in the world, "
               f"{budget['global_headroom']} free CLUT slots in {tightest[0]} chunk "
               f"{tightest[1]['tightest_chunk']}, and resident texels would need "
               f"{linked(16) // 2} halfwords against the {_spare_halfwords()} VRAM leaves "
               'unclaimed.']
    if needed_slots > slots:
        verdict.append(
            f'The Hollow Shade route has the RAM, {linked(32)} bytes at 32x32, but not the slots: '
            f'{rows_on_screen} visible rows beside the {RESERVED_ANIMATION_SLOTS} the frozen view '
            f'reserves is more than the {slots} that exist.')
    else:
        verdict.append(
            f'The Hollow Shade route fits and did not before: {rows_on_screen} slots for the '
            f'visible rows beside the {RESERVED_ANIMATION_SLOTS} the frozen view behind the panel '
            f'reserves is {needed_slots} of {slots}, and the RAM was never the problem at '
            f'{linked(32)} bytes even for 32x32.')
    if chosen:
        bigger = [s for s in ICON_SIZES if s > chosen]
        verdict.append(
            f'The screen is what set the size, not the RAM: {chosen}-pixel icons need '
            f"{layouts[chosen]['scanlines_needed']} of {SCREEN_LINES} scanlines beside six rows "
            f'and a six-line description'
            + (f", and {bigger[0]}-pixel ones need {layouts[bigger[0]]['scanlines_needed']}."
               if bigger else '.')
            + f' Icons ship at {chosen}x{chosen}: {linked(chosen)} bytes of texels, plus a'
              ' 32-byte palette for each resident CLUT row they share.')
    elif needed_slots <= slots:
        verdict.append(
            'No offered icon size can be seated beside six rows and a six-line description inside '
            f'{SCREEN_LINES} scanlines, so the board stays text only.')
    if not chosen:
        verdict.append('Shipping the screen without icons costs nothing to revisit; trimming an '
                       'icon set to squeeze under a cap does.')
    return {
        'source_sprite_pixels': [{'charm': i, 'width': w, 'height': h} for i, w, h in sizes],
        'largest_source_sprite': [max(s[1] for s in sizes), max(s[2] for s in sizes)],
        'texture_budget': budget['texture_budget'],
        'global_clut_headroom': budget['global_headroom'],
        'tightest_view': {'scene': tightest[0], 'chunk': tightest[1]['tightest_chunk'],
                          'cluts': tightest[1]['tightest_cluts'],
                          'textures': tightest[1]['tightest_textures']},
        'atlas_route': 'refused: a pause screen is reachable from every view, and the '
                       f"tightest view leaves {budget['global_headroom']} of "
                       f"{budget['texture_budget']} CLUT slots. The refusal does not turn on "
                       'that figure, which moved when a slot became a distinct palette rather '
                       'than a texture: art resident in every view also needs resident VRAM, and '
                       f"hk_cache::residency::SPARE_HALFWORDS measures {_spare_halfwords()} "
                       'halfwords unclaimed in fragments no bigger than sixteen wide',
        'shade_route_linked_bytes': {f'{side}x{side}': linked(side) for side in ICON_SIZES},
        # What the atlas route would have to find in VRAM, beside its CLUTs.
        'resident_vram_halfwords': {f'{side}x{side}': linked(side) // 2 for side in ICON_SIZES},
        'spare_halfwords': _spare_halfwords(),
        'animation_slots': slots,
        'rows_on_screen': rows_on_screen,
        # Derived rather than written down, because the slot count moved under
        # this once already: the cache went from 8 to 24 when an unused scenery
        # page was reclaimed, and the sentence explaining the refusal kept
        # interpolating the new number into the old argument.
        'shade_route_slots_needed': rows_on_screen + RESERVED_ANIMATION_SLOTS,
        'shade_route_fits': rows_on_screen + RESERVED_ANIMATION_SLOTS <= slots,
        # The size is the largest the screen can seat, not the largest the RAM
        # can hold. Both numbers are here so a later reader can see which one
        # was binding, the way the CLUT and slot halves above already do.
        'panel_scanlines': SCREEN_LINES,
        'panel_layouts': layouts,
        'icon_px': chosen,
        'layout': layouts[chosen] if chosen else None,
        'verdict': ' '.join(verdict),
    }


def source_charm_values(source, assembly, scenes):
    defaults = items.playerdata_defaults(assembly)
    ui = language.sheet(source, 'UI')
    catalogue = items.charms(source, defaults, ui)
    rules = equip_rules(source)
    bodies = _method_bodies(Path(assembly), {
        ('HeroController', 'SoulGain'), ('HeroController', 'CharmUpdate'),
        ('HeroController', 'TakeDamageCharmEffects'), ('PlayerData', 'CalculateNotchesUsed'),
        ('GameManager', 'RefreshOvercharm')})
    hero, constants = hero_constants(source)
    base_soul, bonuses = soul_per_hit_charms(bodies)
    heart = fragile_heart_bonus(bodies)
    nails = nail_multipliers(source)
    shell = stalwart_shell(constants)
    grub = grubsong(bodies, constants)
    # CalculateNotchesUsed is the source's own notch sum, and RefreshOvercharm
    # its own overcharm predicate; both must agree with the FSM contract above.
    notch_fields = {operand for op, operand in bodies[('PlayerData', 'CalculateNotchesUsed')]
                    if op == 'ldfld' and isinstance(operand, str) and operand.startswith('charmCost_')}
    assert notch_fields == {f'charmCost_{n}' for n in range(1, CHARMS + 1)}, \
        'CalculateNotchesUsed no longer sums every charm cost'
    refresh = [operand for op, operand in bodies[('GameManager', 'RefreshOvercharm')] if op == 'ldfld']
    assert refresh[:3] == ['playerData', 'charmSlotsFilled', 'playerData'] and 'charmSlots' in refresh, \
        'RefreshOvercharm no longer compares filled against slots'
    rows = charm_table(catalogue, shell, bonuses, heart, nails, grub, advances())
    icons = icon_measurement(source, catalogue, VISIBLE_ROWS)
    # No size the screen can seat means no icons, and the board stays text only
    # rather than losing rows or description lines to make one fit.
    art = icon_art(source, catalogue, icons['icon_px']) if icons['icon_px'] else None
    if art:
        assert art['palettes'] * 32 == art['palette_bytes']
        icons['linked_bytes'] = len(art['payload'])
        icons['palettes'] = art['palettes']
        icons['art_sources'] = art['art_sources']
        icons['payload_sha256'] = hashlib.sha256(art['payload']).hexdigest()
    return art, {
        'charm_count': len(rows),
        'starting_notches': defaults['charmSlots'],
        'equip_rules': rules,
        'effects': {
            'soul_per_hit_base': base_soul, 'soul_per_hit_charms': bonuses,
            'fragile_heart_masks': heart, 'nail_multipliers': nails,
            'stalwart_shell_ticks': shell, 'grubsong': grub,
        },
        'charms': rows,
        'implemented': [r['id'] for r in rows if r['effect']],
        'unimplemented': {r['id']: r['unimplemented'] for r in rows if not r['effect']},
        'icons': icons,
        'hero': source.sid(hero),
        'assembly_sha256': hashlib.sha256(Path(assembly).read_bytes()).hexdigest(),
        'admitted_scene_count': len(scenes),
        'limitations': [
            'No charm is obtainable in the admitted scenes except Grubsong, whose Shiny sits '
            'inactive in Crossroads_38; the pause screen therefore starts from an empty '
            'collection and the cheat grant is the only way to see the rest.',
            'Fragile charms never break here. The source breaks them in Hero Death Anim and '
            'Divine repairs them, and no admitted scene contains Divine, so breaking would be '
            'an unrecoverable loss rather than a reproduced rule.',
            'Charm 36 changes name, description and binding with quest progress. The cooked '
            'row carries its first state only, and the charm is unequippable regardless.',
            'Notch acquisition is not implemented: the save carries the starting three and '
            'Salubra, the Shrooms and Fog Canyon are outside the slice.',
        ],
    }


def cook():
    from source import Source
    source = Source()
    data = Path(json.load(open(ROOT / '.hkpsx/doctor.json'))['installs'][0]['data_directory'])
    scenes = json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']
    art, values = source_charm_values(source, data / 'Managed' / 'Assembly-CSharp.dll', scenes)
    icons = values['icons']
    # The guest board draws icons, so a cook that cannot seat any of them is a
    # failure here rather than a silently text-only screen: the verdict says
    # which of the three measurements moved.
    assert art, icons['verdict']
    (ROOT / 'data/charm-icons.hk').write_bytes(art['payload'])
    (ROOT / '.hkpsx/charm-icons').mkdir(parents=True, exist_ok=True)
    for i, preview in enumerate(art['previews']):
        preview.save(ROOT / f'.hkpsx/charm-icons/palette{i}.png')
    (ROOT / 'data/charms.rs').write_text(generated_charms_rs(
        values['charms'], values['equip_rules'], {'charmSlots': values['starting_notches']},
        icons['layout'], art))
    dump(ROOT / '.hkpsx/charm-catalog.json', values)
    print(f"{values['charm_count']} charms, {values['starting_notches']} starting notches, "
          f"overcharm breaks through on attempt {values['equip_rules']['overcharm_break_attempt']}")
    print(f"effects implemented: {values['implemented']}")
    print(f"icons: {icons['largest_source_sprite']} px largest source sprite, "
          f"{icons['global_clut_headroom']} of {icons['texture_budget']} CLUT slots free in "
          f"{icons['tightest_view']['scene']} chunk {icons['tightest_view']['chunk']}: atlas out. "
          f"{icons['shade_route_slots_needed']} of {icons['animation_slots']} animation slots, "
          f"{icons['icon_px']}x{icons['icon_px']} at {len(art['payload'])} linked bytes across "
          f"{art['palettes']} resident CLUT rows, board {icons['layout']['panel'][3]} of "
          f"{icons['panel_scanlines']} scanlines")


if __name__ == '__main__':
    cook()
