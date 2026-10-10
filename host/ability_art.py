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
import hashlib, json, math, struct
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
# The Focus effects, after the ball clips: the Knight's child `Focus Effects`
# carries `Lines Anim` (Focus Effect: seven frames of it appearing, then the
# loop section; Focus Effect End once) and `Heal Anim` (Burst Effect once, on
# a heal). Each child's own local position and scale are baked into the frame
# bounds, so a frame draws at the Knight like any other ability frame. Both
# animators share one tk2d library; the path ids are asserted by name.
# The Burst is drawn at three times its sprite size over a screen-wide area, so a texel
# covers three screen pixels: it is cooked at the sprite's own unit scale (no shrink).
BURST_SHRINK = 1
# The first two Burst frames (the flash and the ring) are the bright, large ones and band
# on a palette fitted to the thin lines too: they get a palette of their own, fitted down
# to the faint edge of their halo (`BURST_BRIGHT_THRESHOLD`, the lines use 24).
BURST_BRIGHT_FRAMES = 2
# The Soul Burst: the star at the Knight when the soul orb can heal (`Can Heal 2`), the Knight's
# `Effects/Soul Burst` (resources.assets:4371, a Unity Animator over a SpriteRenderer, scale 1.67
# at (0, -0.48), clip Soul_burst000: five sprites at 20 fps, once). Cooked so that its widest frame
# fits an animation slot (a texel covers about 2.2 screen pixels), into the burst's palette.
SOUL_BURST_SPRITES = (3114, 3559, 3434, 1947, 2426)
SOUL_BURST_PLACE = (0.0, -0.48, 1.67)
SOUL_BURST_SHRINK = 2.2
BURST_BRIGHT_THRESHOLD = 8
# The heal's `White Flash R` (resources.assets:5267): the `white_light` sprite (1987, a pale disc,
# 256x216) at scale 10 on the Knight, white at alpha 0.5176, faded out in a second by SimpleSpriteFade.
# At that scale the disc is 62.6 x 52.8 units, three times the screen, so only the part a screen can
# reach from the Knight is cooked: one screen's width and height either side of it. The texture holds
# the flash at its brightest as greys for the GPU's Add (the original is an alpha blend, which lifts a
# dark view by about the same amount) and the draw's tint fades it. Its palette is a row of its own
# (`FLASH_CLUT_Y`), the one row the Shade's block left unreserved.
FLASH_SPRITE = 1987
FLASH_ALPHA = 0.5176470875740051
FLASH_TEXELS = (64, 48)
# Measured on the original (hkref og, profile focus-og: the heal's first frame minus the frame before,
# over every dark pixel of the view more than eight units from the Knight, so the burst is clear):
# the disc's lift is the sprite's own profile with these three numbers fitted, mean error 1.8 grey on
# a lift of about 60. The disc is a tenth larger than the asset's scale of 10 at the 24.8 pixels a
# unit the original's view gives (the two trade off against that pixel scale), centred one unit below
# the Knight's origin, at 1.08 of the sprite's brightness (cooked at 1.17: the GPU's 15-bit Add lands
# about a tenth under the texel, measured on the port). The FSM's spawn offset was not read, so these
# are the fit, not authored values.
FLASH_SCALE = 11.0
FLASH_BELOW = 1.0
FLASH_GAIN = 1.17
FLASH_CLUT_Y = 495
EFFECT_CLIPS = (('Focus Effect', 'Lines Anim', 6629), ('Focus Effect End', 'Lines Anim', 6629),
                ('Burst Effect', 'Heal Anim', 6375))

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

def effect_animation(source):
    """(clip, local x, local y, scale) per EFFECT_CLIPS name."""
    file = source.file('resources.assets')
    out = {}
    for name, child, pid in EFFECT_CLIPS:
        go = file.objects[pid]
        data = source.read(go)
        assert data['m_Name'] == child, f'{child} moved'
        comps = {source.typename(source.ref(file, c['component'])): source.ref(file, c['component']) for c in data['m_Component']}
        transform = source.read(comps['Transform'])
        parent = source.read(source.ref(file, source.read(source.ref(file, transform['m_Father']))['m_GameObject']))
        assert parent['m_Name'] == 'Focus Effects', 'the Focus effects left their parent'
        local, k = transform['m_LocalPosition'], transform['m_LocalScale']
        assert abs(k['x'] - k['y']) < 1e-4 and abs(local['x']) < 1 and abs(local['y']) < 1.5
        library = source.read(source.ref(file, source.read(comps['tk2dSpriteAnimator'])['library']))
        clip = next(c for c in library['clips'] if c['name'] == name)
        out[name] = (clip, local['x'], local['y'], k['x'])
    return out

def packbits(data):
    """Run-length coding for a frame's texels, which are mostly a few long runs of
    index 0 and of the halo's flat steps. A token below 0x80 is a literal of token+1
    bytes; a token from 0x80 is a run of (token & 0x7f) + 2 copies of the next byte.
    The guest streams it straight into the GPU (game/src/ability_art.rs `upload`)."""
    out, i, n = bytearray(), 0, len(data)
    while i < n:
        j = i
        while j + 1 < n and data[j + 1] == data[i] and j - i < 128:
            j += 1
        if j > i:
            out += bytes([0x80 | (j - i - 1), data[i]])
            i = j + 1
            continue
        k = i
        while k < n and k - i < 128 and not (k + 1 < n and data[k] == data[k + 1]):
            k += 1
        out += bytes([k - i - 1]) + data[i:k]
        i = k
    return bytes(out)

def unpackbits(data):
    out, i = bytearray(), 0
    while i < len(data):
        token = data[i]
        if token < 0x80:
            out += data[i + 1:i + 2 + token]
            i += 2 + token
        else:
            out += bytes([data[i + 1]]) * ((token & 0x7f) + 2)
            i += 2
    return bytes(out)

def quantize_additive(image, threshold=24):
    """4bpp for an additive draw: the colour is premultiplied by the alpha, a
    texel that adds (almost) nothing is the transparent index 0 and every other
    entry has its semi-transparency bit, so the GPU's Add reads all of them."""
    w, h = image.size
    px = list(image.convert('RGBA').getdata())
    lit = []
    for r, g, b, a in px:
        r, g, b = (c * a // 255 for c in (r, g, b))
        lit.append((r, g, b) if max(r, g, b) >= threshold else None)
    colours = [c for c in lit if c is not None]
    palette, indices = [0], [0] * (w * h)
    if colours:
        rgb = Image.new('RGB', (len(colours), 1))
        rgb.putdata(colours)
        q = rgb.quantize(colors=15, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
        table = q.getpalette()
        mapping = {}
        for original in sorted(set(q.tobytes())):
            r, g, b = table[original * 3:original * 3 + 3]
            mapping[original] = len(palette)
            palette.append((r >> 3) | ((g >> 3) << 5) | ((b >> 3) << 10) | 0x8000)
        it = iter(q.tobytes())
        for i, c in enumerate(lit):
            if c is not None:
                indices[i] = mapping[next(it)]
    packed = bytearray(((w + 1) // 2) * h)
    for i, index in enumerate(indices):
        packed[(i // w) * ((w + 1) // 2) + (i % w) // 2] |= index << ((i % w & 1) * 4)
    return w, h, struct.pack('<16H', *(palette + [0] * (16 - len(palette)))), bytes(packed)

def quantize_flash(image):
    """The flash as fifteen greys for an additive draw: the sprite's brightness at the
    renderer's alpha (`FLASH_ALPHA`) with the brightest texel the top grey, index 0 clear."""
    w, h = image.size
    value = [(r + g + b) / 3 * a / 255 * FLASH_ALPHA * FLASH_GAIN for r, g, b, a in image.convert('RGBA').getdata()]
    peak = max(value)
    packed = bytearray(((w + 1) // 2) * h)
    for i, v in enumerate(value):
        packed[(i // w) * ((w + 1) // 2) + (i % w) // 2] |= round(v / peak * 15) << ((i % w & 1) * 4)
    greys = [(round(peak * i / 15) + 4) >> 3 for i in range(1, 16)]
    palette = [0] + [g * 0x0421 | 0x8000 for g in greys]
    return w, h, struct.pack('<16H', *palette), bytes(packed)

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

    def sprite(collection_ref, index, place=None, shrink=1):
        obj = source.ref(resources, collection_ref)
        image, box = tk_sprite(source, obj.assets_file, source.read(obj), index, textures)
        dims = tuple(max(1, math.ceil((box[i + 2] - box[i]) * scale / shrink)) for i in (0, 1))
        images.append(image.resize(dims, Image.Resampling.LANCZOS))
        if place:
            # A child's local transform: the texture stays at the unit scale
            # and the quad grows with the child's scale.
            lx, ly, k = place
            box = (lx + k * box[0], ly + k * box[1], lx + k * box[2], ly + k * box[3])
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

    effects = effect_animation(source)
    effect_loop = None
    for name, _, _ in EFFECT_CLIPS:
        clip, lx, ly, k = effects[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'], (lx, ly, k), BURST_SHRINK if name == 'Burst Effect' else 1)
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode']))
        if name == 'Focus Effect':
            assert clip['wrapMode'] == 1
            effect_loop = clip['loopStart']

    from cook import native_sprite
    soul_start = len(images)
    lx, ly, k = SOUL_BURST_PLACE
    for pid in SOUL_BURST_SPRITES:
        image, box = native_sprite(resources.objects[pid])
        dims = tuple(max(1, math.ceil((box[i + 2] - box[i]) * k * scale / SOUL_BURST_SHRINK)) for i in (0, 1))
        images.append(image.resize(dims, Image.Resampling.LANCZOS))
        boxes.append([lx + k * box[0], ly + k * box[1], lx + k * box[2], ly + k * box[3]])
        art_sources.append(f'resources.assets:{pid}')
    clips.append(dict(name='Soul Burst', start=soul_start, count=len(SOUL_BURST_SPRITES), fps=20, wrap=2))

    flash_start = len(images)
    image, box = native_sprite(resources.objects[FLASH_SPRITE])
    reach = (320 / scale, 240 / scale)
    half = tuple(r / FLASH_SCALE * n / (box[i + 2] - box[i]) for i, (r, n) in enumerate(zip(reach, image.size)))
    cx = image.width / 2
    cy = image.height / 2 - FLASH_BELOW / FLASH_SCALE * image.height / (box[3] - box[1])
    crop = image.convert('RGBA').crop((round(cx - half[0]), round(cy - half[1]),
                                       round(cx + half[0]), round(cy + half[1])))
    images.append(crop.resize(FLASH_TEXELS, Image.Resampling.BOX))
    boxes.append([-reach[0], -reach[1], reach[0], reach[1]])
    art_sources.append(f'resources.assets:{FLASH_SPRITE}')
    clips.append(dict(name='Heal Flash', start=flash_start, count=1, fps=1, wrap=2))

    # One CLUT per group of clips that fits a single 256x256 quantizer sheet, so
    # every frame of one animation keeps one palette.
    effect_names = {name for name, _, _ in EFFECT_CLIPS} | {'Soul Burst', 'Heal Flash'}
    groups, current = [], []
    for clip in clips:
        if clip['name'] in effect_names:
            continue
        indices = list(range(clip['start'], clip['start'] + clip['count']))
        if shelf([images[i].size for i in current + indices]) is None:
            assert current, f"the {clip['name']} clip alone does not fit one sheet"
            groups.append(current)
            current = indices
        else:
            current = current + indices
    if current:
        groups.append(current)
    # The effects are Screen blends in the original: premultiplied by their
    # alpha, black adding nothing, so they get palettes of their own that the
    # GPU's Add reads (`quantize_additive`).
    additive_from = len(groups)
    # The Burst's two bright frames get their own palette, last, fitted to their halo.
    burst = next(c for c in clips if c['name'] == 'Burst Effect')
    bright = list(range(burst['start'], burst['start'] + BURST_BRIGHT_FRAMES)) + list(range(soul_start, soul_start + len(SOUL_BURST_SPRITES)))
    current = []
    for clip in clips:
        if clip['name'] not in effect_names:
            continue
        indices = [i for i in range(clip['start'], clip['start'] + clip['count']) if i not in bright]
        if shelf([images[i].size for i in current + indices]) is None:
            assert current, f"the {clip['name']} clip alone does not fit one sheet"
            groups.append(current)
            current = indices
        else:
            current = current + indices
    if current:
        groups.append(current)
    bright_clut = len(groups)
    groups.append(bright)
    assert len(groups) <= CLUT_ROWS, f'{len(groups)} palettes exceed the {CLUT_ROWS} reserved rows'
    flash_clut = len(groups)
    groups.append([flash_start])

    palettes, blob, frames = [], bytearray(), [None] * len(images)
    raw_bytes = 0
    for clut_index, group in enumerate(groups):
        positions = shelf([images[i].size for i in group])
        sheet = Image.new('RGBA', (256, 256))
        for i, origin in zip(group, positions):
            sheet.paste(images[i], origin)
        sw, _, palette, packed = (lambda s: quantize_flash(s) if clut_index == flash_clut else quantize_additive(s, BURST_BRIGHT_THRESHOLD) if clut_index == bright_clut else quantize_additive(s) if clut_index >= additive_from else quantize_alpha_coverage(s, 128))(sheet)
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
            coded = packbits(bytes(texels))
            assert unpackbits(coded) == bytes(texels)
            frames[i] = dict(offset=len(blob), width=image.width, height=image.height,
                             bounds=boxes[i], clut=clut_index)
            blob.extend(coded)
            raw_bytes += len(texels)

    palette_bytes = b''.join(palettes)
    payload = bytes(palette_bytes) + bytes(blob)
    (ROOT / 'data/ability-art.hk').write_bytes(payload)
    lines = [
        '// Generated Knight ability art; no embedded retail source dump.',
        f'pub const CLUT_RECT:(u16,u16,u16,u16)=({CLUT[0]},{CLUT[1]},{CLUT[2]},{CLUT[3]});',
        f'pub const PALETTE_BYTES:usize={len(palette_bytes)};',
        f'pub const PALETTE_COUNT:usize={len(palettes)};',
        'pub const CLUT_Y:&[u16]=&[' + ','.join(str(CLUT[1] + i if i < CLUT_ROWS else FLASH_CLUT_Y) for i in range(len(palettes))) + '];',
        'pub const ABILITY_CLIPS:&[Clip]=&[' + ','.join(
            'Clip{' + ','.join(f'{k}:{c[k]}' for k in ('start', 'count', 'fps', 'wrap')) + '}'
            for c in clips) + '];',
        'pub const ABILITY_FRAMES:&[Frame]=&[' + ','.join(
            'Frame{offset:%d,width:%d,height:%d,clut:%d,bounds:[%s]}' % (
                f['offset'], f['width'], f['height'], f['clut'],
                ','.join(str(round(v * 65536)) for v in f['bounds']))
            for f in frames) + '];',
    ]
    lines.append(f'pub const FOCUS_EFFECT_LOOP_START:usize={effect_loop};')
    code = '\n'.join(lines) + '\n'
    (ROOT / 'data/ability-art.rs').write_text(code)
    report = {'clips': clips, 'frames': len(frames), 'palettes': len(palettes),
              'payload_bytes': len(payload), 'texel_bytes_uncoded': raw_bytes, 'clut_rect': list(CLUT), 'clut_rows': CLUT_ROWS,
              'animation': source.sid(anim_obj), 'art_sources': art_sources,
              'payload_sha256': hashlib.sha256(payload).hexdigest(),
              'storage': 'linked RAM, run-length coded per frame; frames are decoded into VRAM through the shared 64x64 animation slots',
              'limitations': ['The Crystal Heart has no clip here: its SD set is a separate '
                              'charge, dash and wall-hit sequence that P15 has not modelled.']}
    dump(ROOT / '.hkpsx/ability-art.json', report)
    print(f'Ability art: {len(frames)} original frames across {len(clips)} clips, '
          f'{len(palettes)} palettes, {len(payload)} linked bytes')

if __name__ == '__main__':
    cook()
