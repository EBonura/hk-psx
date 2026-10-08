"""The Knight's ability clips, cooked the way the Hollow Shade's art is.

An ability is usable in any view, so its frames cannot ride in a per-region
atlas: the tightest admitted region has about 19 free texture slots against the
416-slot CLUT budget, and the ten clips need far more than that. They take the
Shade's route instead. Frames stay in linked RAM and reach VRAM through the
eight existing 64x64 animation slots, so nothing new is reserved but the CLUT
rows, which come out of the block the Shade reserved and does not use.

No retail payload is embedded here; the generated blob and table are build
outputs under data/.
"""
import hashlib, json, math
from pathlib import Path
from PIL import Image
from source import Source, ROOT, dump
from cook import tk_sprite, FOCAL, CAM_Z
from materials import quantize_alpha_coverage

# x320..335 y484..487, out of the 14 rows shade.py reserved at y482 and the two
# it uses. docs/BUDGET.md carries the split.
CLUT = (320, 484, 16, 1)
CLUT_ROWS = 4
# The animation slot caps a single upload, as it does for the Shade.
SLOT_BYTES = 2048
# Order is the guest's ABILITY_CLIP index; it is not free to move.
ORDER = ('Dash', 'Wall Slide', 'Walljump', 'Double Jump',
         'DN Start', 'DN Charge', 'DN Slash Antic', 'DN Slash',
         'Fireball Antic', 'Fireball1 Cast',
         # The Crystal Heart's body poses. Its Fx, Trail and Crys clips are
         # effects rather than the Knight, and are not cooked.
         'SD Charge Ground', 'SD Wall Charge', 'SD Dash', 'SD Hit Wall')
# Vengeful Spirit's projectile, from its own sprite collection rather than the
# Knight's, appended after the Knight clips.
BALL_CLIPS = ('Ball', 'Ball End')

def knight_animation(source):
    """The Knight's own tk2d animation table, found by the clips it carries."""
    resources = source.file('resources.assets')
    for obj in resources.objects.values():
        if obj.type.name != 'MonoBehaviour' or source.typename(obj) != 'tk2dSpriteAnimation':
            continue
        table = source.read(obj)
        names = {c['name'] for c in table['clips']}
        if {'Idle', 'Run', 'Slash', 'Airborne', 'Focus'}.issubset(names):
            return obj, table
    raise LookupError('no Knight animation table')

def fireball_animation(source):
    """The Vengeful Spirit projectile's own tk2d clips, reached through the
    Hero's Fireball Cast FSM rather than by prefab name."""
    from spells import _named_child, _fsm_on
    from superdash import _fsm
    file = source.file('resources.assets')
    control = _fsm(source, 'Spell Control')
    states = {s['name']: s for s in control['states']}
    caster = _named_child(source, file, states['Fireball 1'], 'Fireball Top')
    cast = _fsm_on(source, file, caster, 'Fireball Cast')
    cast_states = {s['name']: s for s in cast['states']}
    ball = _named_child(source, file, cast_states['Cast Right'], 'Fireball')
    animator = next(source.ref(file, c['component']) for c in ball['m_Component']
                    if source.typename(source.ref(file, c['component'])) == 'tk2dSpriteAnimator')
    library = source.read(source.ref(file, source.read(animator)['library']))
    return {c['name']: c for c in library['clips']}

def shelf(sizes, limit=256):
    """Shelf packing into one quantizer sheet; None when the set does not fit."""
    order = sorted(range(len(sizes)), key=lambda i: -sizes[i][1])
    positions = [None] * len(sizes)
    x = y = row = 0
    for i in order:
        w, h = sizes[i]
        if w > limit or h > limit:
            return None
        if x + w > limit:
            x, y, row = 0, y + row, 0
        if y + h > limit:
            return None
        positions[i] = (x, y)
        x += w
        row = max(row, h)
    return positions

def cook():
    source = Source()
    anim_obj, table = knight_animation(source)
    by_name = {c['name']: c for c in table['clips']}
    missing = [n for n in ORDER if n not in by_name]
    assert not missing, f'the Knight animation lost {missing}'
    resources = source.file('resources.assets')
    scale = FOCAL / -CAM_Z
    textures, images, boxes, art_sources = {}, [], [], []

    def sprite(collection_ref, index):
        obj = source.ref(resources, collection_ref)
        image, box = tk_sprite(source, obj.assets_file, source.read(obj), index, textures)
        dims = tuple(max(1, math.ceil((box[i + 2] - box[i]) * scale)) for i in (0, 1))
        images.append(image.resize(dims, Image.Resampling.LANCZOS))
        boxes.append(list(box))
        art_sources.append(f'{source.sid(obj)}:{index}')

    ball_by_name = fireball_animation(source)
    missing = [n for n in BALL_CLIPS if n not in ball_by_name]
    assert not missing, f'the Fireball animation lost {missing}'
    clips = []
    for name in ORDER + BALL_CLIPS:
        clip = by_name[name] if name in ORDER else ball_by_name[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'])
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode']))

    # One CLUT per group of clips that fits a single 256x256 quantizer sheet, so
    # every frame of one animation keeps one palette.
    groups, current = [], []
    for clip in clips:
        indices = list(range(clip['start'], clip['start'] + clip['count']))
        if shelf([images[i].size for i in current + indices]) is None:
            assert current, f"the {clip['name']} clip alone does not fit one sheet"
            groups.append(current)
            current = indices
        else:
            current = current + indices
    if current:
        groups.append(current)
    assert len(groups) <= CLUT_ROWS, f'{len(groups)} palettes exceed the {CLUT_ROWS} reserved rows'

    palettes, blob, frames = [], bytearray(), [None] * len(images)
    for clut_index, group in enumerate(groups):
        positions = shelf([images[i].size for i in group])
        sheet = Image.new('RGBA', (256, 256))
        for i, origin in zip(group, positions):
            sheet.paste(images[i], origin)
        sw, _, palette, packed = quantize_alpha_coverage(sheet, 128)
        palettes.append(palette)
        for i, (x0, y0) in zip(group, positions):
            image = images[i]
            stride = (image.width + 3) // 4 * 2
            texels = bytearray(stride * image.height)
            for y in range(image.height):
                for x in range(image.width):
                    byte = packed[(y + y0) * ((sw + 1) // 2) + (x + x0) // 2]
                    nibble = (byte >> (((x + x0) & 1) * 4)) & 15
                    texels[y * stride + x // 2] |= nibble << ((x & 1) * 4)
            assert len(texels) <= SLOT_BYTES, f'ability frame {i} exceeds one animation slot'
            frames[i] = dict(offset=len(blob), width=image.width, height=image.height,
                             bounds=boxes[i], clut=clut_index)
            blob.extend(texels)

    palette_bytes = b''.join(palettes)
    payload = bytes(palette_bytes) + bytes(blob)
    (ROOT / 'data/ability-art.hk').write_bytes(payload)
    lines = [
        '// Generated Knight ability art; no embedded retail source dump.',
        f'pub const CLUT_RECT:(u16,u16,u16,u16)=({CLUT[0]},{CLUT[1]},{CLUT[2]},{CLUT[3]});',
        f'pub const PALETTE_BYTES:usize={len(palette_bytes)};',
        f'pub const PALETTE_COUNT:usize={len(palettes)};',
        'pub const ABILITY_CLIPS:&[Clip]=&[' + ','.join(
            'Clip{' + ','.join(f'{k}:{c[k]}' for k in ('start', 'count', 'fps', 'wrap')) + '}'
            for c in clips) + '];',
        'pub const ABILITY_FRAMES:&[Frame]=&[' + ','.join(
            'Frame{offset:%d,width:%d,height:%d,clut:%d,bounds:[%s]}' % (
                f['offset'], f['width'], f['height'], f['clut'],
                ','.join(str(round(v * 65536)) for v in f['bounds']))
            for f in frames) + '];',
    ]
    code = '\n'.join(lines) + '\n'
    (ROOT / 'data/ability-art.rs').write_text(code)
    report = {'clips': clips, 'frames': len(frames), 'palettes': len(palettes),
              'payload_bytes': len(payload), 'clut_rect': list(CLUT), 'clut_rows': CLUT_ROWS,
              'animation': source.sid(anim_obj), 'art_sources': art_sources,
              'payload_sha256': hashlib.sha256(payload).hexdigest(),
              'storage': 'linked RAM; frames reach VRAM through the shared 64x64 animation slots',
              'limitations': ['The Crystal Heart has no clip here: its SD set is a separate '
                              'charge, dash and wall-hit sequence that P15 has not modelled.']}
    dump(ROOT / '.hkpsx/ability-art.json', report)
    print(f'Ability art: {len(frames)} original frames across {len(clips)} clips, '
          f'{len(palettes)} palettes, {len(payload)} linked bytes')

if __name__ == '__main__':
    cook()
