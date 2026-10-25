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
from PIL import Image, ImageChops
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
BURST_BRIGHT_THRESHOLD = 8
# The last palette is emitted into the one row left in the Shade reservation.
FLASH_CLUT_Y = 495
EFFECT_CLIPS = (('Focus Effect', 'Lines Anim', 6629), ('Focus Effect End', 'Lines Anim', 6629),
                ('Burst Effect', 'Heal Anim', 6375))
# The Knight looking up and down while he stands (HeroAnimationController.UpdateAnimation: `LookUp`
# while `lookingUpAnim`, `LookDown` while `lookingDownAnim`; PlayIdle answers a released look with
# `LookUpEnd` / `LookDownEnd` before Idle). Appended last so no earlier clip index moves. The two
# look clips loop from frame 1 (tk2d loopStart), the two ends play once.
LOOK_CLIPS = ('LookUp', 'LookDown', 'LookUpEnd', 'LookDownEnd')
# The crouch of a hard landing (HeroAnimationController.UpdateAnimation: `HardLand` while the actor
# state is hard_landing). Ten frames of five sprites at 12.5 fps, played once: 0.8 s, which is
# HeroController.HARD_LANDING_TIME to the frame. Appended after the look clips so no index moves.
HARD_LAND_CLIP = 'HardLand'
# King's Pass's opening crash (the `Initial Fall Impact` FSM `Control`, Tutorial_01): `Super Hard Land`
# (four frames at 12 fps, held on its last) when he lands, `Collect Normal 3` (five frames) to get up.
CRASH_CLIPS = ('Super Hard Land', 'Collect Normal 3')
# The Knight's death, the bench respawn and the hit effects, appended after the crash clips so no
# index moves. `Death` is the Hero Death object's own clip (the body breaking up, 18 frames at
# 10 fps, played once); `Sit Fall Asleep` is what Bench Control's Init Resting plays for the 1.2 s
# after a death respawn and `Wake To Sit` (9 frames at 12 fps) is its Startle.
DEATH_CLIPS = ('Death', 'Sit Fall Asleep', 'Wake To Sit')
# The Knight's ability effects (packs `art_fx` and `art_fx_spell`, carried while an ability that draws one is owned): a
# child of the Knight prefab (the Dash Burst, Double J Wings and Wall Puff objects of its `Effects` child) or a prefab
# the HeroController spawns, drawn at the Knight like the Focus effects. Each is (name, source, clip, local x, local y,
# local scale x, local scale y, density). The transforms are the prefab's own (`pfx` of the Knight,
# resources.assets:3916), except the Dash Burst's position, which `HeroController.HeroDash` sets on every ordinary
# dash (4.11, -0.55), and its x scale is the prefab's (-1.509: the streaks trail behind). `source` is 'knight' (the
# Knight's own tk2d library) or a library path id of resources.assets. `density` is screen pixels per texel at the
# frame's drawn size (1.0 is the Knight's own density, one texel a pixel), so a child's scale does not coarsen it.
# The effects are light on black in the original (Dash Effect is `Sprites/Default-ColorFlash`, the wings
# `UI/BlendModes/LinearDodge`, the puff a pale dust): each frame keeps its colour premultiplied by its alpha, is cut
# to 64x64 parts (one animation slot each) and drawn adding.
FX_CLIPS = (('Dash Effect', 'knight', 'Dash Effect', 4.11, -0.55, -1.509, 1.509, 1.0),
            ('Double Jump Wings', 'knight', 'Double Jump Wings 2', 0.22, -0.064, 1.0, 1.0, 1.0),
            ('Wall Puff', 23373, 'Wall Puff', 0.91, -0.32, 1.421, 1.421, 1.0),
            # `White Flash DJ` (resources.assets:4989): the original lays `white_light` (a flat, half-clear
            # rectangle of white) 2.3 times its size at (0, -0.54); what the eye takes from it is a small blue
            # aura, which is cooked as one round glow (AURA_*) of that reach and place.
            ('Double Jump Flash', 'aura', None, 0.0, -0.54, 1.0, 1.0, 1.5),
            # Vengeful Spirit (tk2d library 23262, drawn facing right): `Fireball Top`'s `Fireball Blast`
            # (a child at (1.13, 0), scale 2) as the ball leaves, and the ball's own `Wall Impact`
            # child (1.709, 0), scale 2.5, when a wall stops it.
            ('Fireball Blast', 23262, 'Blast', 1.13, 0.0, 2.0, 2.0, 1.7),
            ('Fireball Wall Impact', 23262, 'Fireball Wall Impact', 1.709, 0.0, 2.5, 2.5, 1.35))
# A frame is cut to parts of at most this many texels a side: the animation slot.
FX_PART = 64
# The double jump's aura: its radius in world units, the colour at its centre (0..255) and how fast it falls off.
AURA_RADIUS = 1.6
AURA_RGB = (42, 70, 160)
# A frame's outer edge fades to black over this many texels, so no quad shows a rectangle's edge.
FX_EDGE_FADE = 2.5
# Brightness (0..255, premultiplied) a texel needs to be anything but transparent.
FX_THRESHOLD = 14
# Weight of the Burst's and the Soul Burst's own pixels against the effects' when the shared bright palette is fitted.
FX_PALETTE_BURST_WEIGHT = 6

# Frames the guest keeps on the disc, not in linked RAM (game/src/ability_art.rs `STREAMS`). They are the
# last frames, so the resident table ends where the first pack starts. Each pack is a carried data module
# (host/code_modules.py CARRIED, read into the room pool by the scene gate or the pool's background fetch):
# the clips that make it, in cook order, and the file it is written to. A pack's clips must be contiguous
# and the packs must follow each other in this order; to stream another group of frames, add its clips
# after the last pack's and a row here, and a module of its name to CARRIED.
STREAM_PACKS = (('art_death', 'data/art-death.hk', ('Death',)),
                ('art_wake', 'data/art-wake.hk', ('Sit Fall Asleep', 'Wake To Sit', 'Wake Up Ground Lie', 'Wake Up Ground Rise')),
                ('art_fx', 'data/art-fx.hk', ('Dash Effect', 'Double Jump Wings', 'Wall Puff', 'Double Jump Flash')),
                ('art_fx_spell', 'data/art-fx-spell.hk', ('Fireball Blast', 'Fireball Wall Impact')))
# One streamed frame's record at the head of its pack: offset into the pack's coded texels (u32), width and
# height (u16), palette (u32) and the four bounds (i32, Q16): the guest's `Frame` field for field.
STREAM_RECORD = '<IHHIiiii'
# Effects from other tk2d libraries of resources.assets, as (name, library path id, clip, scale):
# the `Slash Effect GhostDark1/2` the Knight Damage FSM flings on every hit and the Hero Death
# FSM bursts on a death (`Slash Ghost 1/2`, prefab scale 2). The `Hit Crack` and `Death Crack`
# tk2d clips are one-pixel placeholders: the visible cracks are Unity Animator sprites, not cooked.
# A trailing blank frame (sprite 77) only hides the effect and is not cooked: the clip's end does.
EFFECT_LIBRARY_CLIPS = (('Slash Ghost 1', 21298, 'Slash Ghost 1', 2.0),
                        ('Slash Ghost 2', 21298, 'Slash Ghost 2', 2.0))
BLANK_SPRITE = 77
# The respawn at the new-game spawn (`HeroController.Respawn`, respawn type 0) plays `Wake Up Ground`:
# 25 frames of one lying pose, 20 more getting up, and a one-pixel closing sprite. The lying pose is
# cooked once and held (`Wake Up Ground Lie`), the rest as `Wake Up Ground Rise`.
WAKE_GROUND = 'Wake Up Ground'
WAKE_GROUND_LIE_FRAMES = 25
# Its frames are large, so they are cooked at half texel density to fit the shared palette sheets.
GROUND_SHRINK = 2
# The slashes are soft white wisps, drawn additively like the Focus effects: cut to a third of their
# length (a slot is 64 texels wide) and kept at full height, so they stay thin.
SLASH_SHRINK = (3, 1)

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

def fit_palette(colours, count=15):
    """`count` colours for premultiplied light, by median cut: (r, g, b) in 5-bit steps."""
    rgb = Image.new('RGB', (len(colours), 1))
    rgb.putdata(colours)
    q = rgb.quantize(colors=count, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    table = q.getpalette()
    return [tuple(c >> 3 << 3 for c in table[i * 3:i * 3 + 3]) for i in sorted(set(q.tobytes()))]

def palette_words(entries):
    """A 16-entry additive palette: index 0 clear, every other entry semi-transparent."""
    words = [0] + [(r >> 3) | ((g >> 3) << 5) | ((b >> 3) << 10) | 0x8000 for r, g, b in entries]
    return struct.pack('<16H', *(words + [0] * (16 - len(words))))

def nearest_entry(colour, entries, cache):
    """The 1-based palette index nearest `colour` in RGB."""
    if colour not in cache:
        cache[colour] = 1 + min(range(len(entries)), key=lambda k: sum((a - b) ** 2 for a, b in zip(colour, entries[k])))
    return cache[colour]

def pack_indices(indices, w, h):
    packed = bytearray(((w + 1) // 2) * h)
    for i, index in enumerate(indices):
        packed[(i // w) * ((w + 1) // 2) + (i % w) // 2] |= index << ((i % w & 1) * 4)
    return bytes(packed)

def light(colour):
    """A premultiplied colour's brightness (0..255)."""
    return (colour[0] * 77 + colour[1] * 150 + colour[2] * 29) / 256

def quantize_additive_shared(image, threshold, extra, weight):
    """`quantize_additive` for a palette the effects share: it is fitted to the sheet's own light
    (counted `weight` times) and to `extra`, the effects' samples, and each texel takes the
    nearest entry."""
    w, h = image.size
    lit = []
    for r, g, b, a in image.convert('RGBA').getdata():
        r, g, b = (c * a // 255 for c in (r, g, b))
        lit.append((r, g, b) if max(r, g, b) >= threshold else None)
    own = [c for c in lit if c is not None]
    entries = fit_palette(own * weight + list(extra))
    cache = {}
    indices = [0 if c is None else nearest_entry(c, entries, cache) for c in lit]
    return w, h, palette_words(entries), pack_indices(indices, w, h)

def fx_parts(image, box, scale, density, premultiplied=False):
    """One effect frame as light on black: its colour premultiplied by its alpha, cut to `density`
    screen pixels a texel at the size `box` (world units, corners as the draw takes them) shows it
    at, faded to nothing over `FX_EDGE_FADE` texels at the frame's edge, then split into parts of
    at most FX_PART texels a side (one animation slot each; a part with no light is dropped).
    Returns [(part image, its box)]: parts abut exactly, sharing the float their edge is computed from."""
    w = max(1, round(abs(box[2] - box[0]) * scale / density))
    h = max(1, round(abs(box[3] - box[1]) * scale / density))
    if premultiplied:
        lit = image.convert('RGB').resize((w, h), Image.Resampling.LANCZOS)
    else:
        r, g, b, a = image.convert('RGBA').split()
        lit = Image.merge('RGB', [ImageChops.multiply(c, a) for c in (r, g, b)]).resize((w, h), Image.Resampling.LANCZOS)
    ramp = lambda n: [min(1.0, min(i + 0.5, n - i - 0.5) / FX_EDGE_FADE) for i in range(n)]
    fx, fy = ramp(w), ramp(h)
    mask = Image.new('L', (w, h))
    mask.putdata([round(255 * fy[y] * fx[x]) for y in range(h) for x in range(w)])
    lit = Image.merge('RGB', [ImageChops.multiply(c, mask) for c in lit.split()])
    nx, ny = -(-w // FX_PART), -(-h // FX_PART)
    tw, th = -(-w // nx), -(-h // ny)
    parts = []
    for ty in range(0, h, th):
        for tx in range(0, w, tw):
            x1, y1 = min(w, tx + tw), min(h, ty + th)
            part = lit.crop((tx, ty, x1, y1))
            # Only the texels that carry light are kept: the part is cut to their box, so the upload and the
            # GPU's texel fetches skip the clear margins.
            lit_box = part.convert('L').point(lambda v: 255 if v >= FX_THRESHOLD else 0).getbbox()
            if lit_box is None:
                continue
            tx, ty, x1, y1 = tx + lit_box[0], ty + lit_box[1], tx + lit_box[2], ty + lit_box[3]
            part = lit.crop((tx, ty, x1, y1))
            across = lambda t: box[0] + (box[2] - box[0]) * t / w
            down = lambda t: box[3] + (box[1] - box[3]) * t / h
            parts.append((part, [across(tx), down(y1), across(x1), down(ty)]))
    return parts

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
    missing = [n for n in ORDER + LOOK_CLIPS + (HARD_LAND_CLIP,) + CRASH_CLIPS + DEATH_CLIPS + (WAKE_GROUND,)
               + tuple(c for _, source_, c, *_ in FX_CLIPS if source_ == 'knight') if n not in by_name]
    assert not missing, f'the Knight animation lost {missing}'
    resources = source.file('resources.assets')
    scale = FOCAL / -CAM_Z
    textures, images, boxes, art_sources = {}, [], [], []

    def sprite(collection_ref, index, place=None, shrink=1):
        obj = source.ref(resources, collection_ref)
        image, box = tk_sprite(source, obj.assets_file, source.read(obj), index, textures)
        kx, ky = shrink if isinstance(shrink, tuple) else (shrink, shrink)
        dims = tuple(max(1, math.ceil((box[i + 2] - box[i]) * scale / k)) for i, k in ((0, kx), (1, ky)))
        images.append(image.resize(dims, Image.Resampling.LANCZOS))
        if place:
            # A child's local transform: the texture stays at the unit scale
            # and the quad grows with the child's scale. A negative scale turns the corners
            # round, which draws the frame mirrored.
            lx, ly, kx, ky = place if len(place) == 4 else (*place[:2], place[2], place[2])
            box = (lx + kx * box[0], ly + ky * box[1], lx + kx * box[2], ly + ky * box[3])
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
                          fps=int(clip['fps']), wrap=clip['wrapMode'], loop=int(clip.get('loopStart', 0))))

    effects = effect_animation(source)
    effect_loop = None
    for name, _, _ in EFFECT_CLIPS:
        clip, lx, ly, k = effects[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'], (lx, ly, k), BURST_SHRINK if name == 'Burst Effect' else 1)
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode'], loop=int(clip.get('loopStart', 0))))
        if name == 'Focus Effect':
            assert clip['wrapMode'] == 1
            effect_loop = clip['loopStart']

    focus_art = ROOT / '.hkpsx/ability-focus-rust'
    focus_frames = json.loads((focus_art / 'frames.json').read_text())
    assert len(focus_frames) == 6
    soul_start = len(images)
    for frame in focus_frames[:5]:
        with Image.open(focus_art / frame['name']) as image:
            images.append(image.copy())
        boxes.append(frame['box'])
        art_sources.append(frame['source'])
    clips.append(dict(name='Soul Burst', start=soul_start, count=5, fps=20, wrap=2, loop=0))

    flash_start = len(images)
    frame = focus_frames[5]
    with Image.open(focus_art / frame['name']) as image:
        images.append(image.copy())
    boxes.append(frame['box'])
    art_sources.append(frame['source'])
    clips.append(dict(name='Heal Flash', start=flash_start, count=1, fps=1, wrap=2, loop=0))

    for name in LOOK_CLIPS:
        clip = by_name[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'])
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode'], loop=int(clip.get('loopStart', 0))))
        if name in ('LookUp', 'LookDown'):
            assert clip['wrapMode'] == 1 and clip['loopStart'] == 1, f'{name} no longer loops from frame 1'
        else:
            assert clip['wrapMode'] == 2, f'{name} no longer plays once'

    hard_land = by_name[HARD_LAND_CLIP]
    start = len(images)
    for frame in hard_land['frames']:
        sprite(frame['spriteCollection'], frame['spriteId'])
    assert hard_land['wrapMode'] == 2, 'HardLand no longer plays once'
    assert len(hard_land['frames']) * 60 / hard_land['fps'] == 48, 'HardLand no longer lasts HARD_LANDING_TIME'
    clips.append(dict(name=HARD_LAND_CLIP, start=start, count=len(hard_land['frames']),
                      fps=int(hard_land['fps']), wrap=hard_land['wrapMode'], loop=int(hard_land.get('loopStart', 0))))

    for name in CRASH_CLIPS:
        clip = by_name[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'])
        assert clip['wrapMode'] == 2 and clip['fps'] == 12, f'{name} no longer plays once at 12 fps'
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode'], loop=int(clip.get('loopStart', 0))))

    for name, library_id, clip_name, k in EFFECT_LIBRARY_CLIPS:
        library = source.read(resources.objects[library_id])
        clip = next(c for c in library['clips'] if c['name'] == clip_name)
        frames = [f for f in clip['frames'] if f['spriteId'] != BLANK_SPRITE]
        assert clip['wrapMode'] == 2 and len(frames) == len(clip['frames']) - 1, f'{clip_name} changed'
        start = len(images)
        for frame in frames:
            sprite(frame['spriteCollection'], frame['spriteId'], (0.0, 0.0, k), SLASH_SHRINK)
        clips.append(dict(name=name, start=start, count=len(frames),
                          fps=int(clip['fps']), wrap=clip['wrapMode'], loop=0))

    for name in DEATH_CLIPS:
        clip = by_name[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'])
        assert clip['wrapMode'] == 2, f'{name} no longer plays once'
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode'], loop=int(clip.get('loopStart', 0))))
    ground = by_name[WAKE_GROUND]
    assert ground['wrapMode'] == 2 and int(ground['fps']) == 18, f'{WAKE_GROUND} changed'
    lie, rise = ground['frames'][:WAKE_GROUND_LIE_FRAMES], ground['frames'][WAKE_GROUND_LIE_FRAMES:-1]
    assert {f['spriteId'] for f in lie} == {343} and ground['frames'][-1]['spriteId'] == 7, f'{WAKE_GROUND} changed'
    for name, frames in (('Wake Up Ground Lie', lie[:1]), ('Wake Up Ground Rise', rise)):
        start = len(images)
        for frame in frames:
            sprite(frame['spriteCollection'], frame['spriteId'], None, GROUND_SHRINK)
        clips.append(dict(name=name, start=start, count=len(frames), fps=18, wrap=2, loop=0))

    # The ability effects (FX_CLIPS), last so no earlier index moves. A clip's `count` is its frames and its
    # `parts` the images they were cut into; `fx_frame_first` says where each frame's parts start.
    fx_names = {name for name, *_ in FX_CLIPS}
    fx_frame_first, fx_clip_frames, fx_samples = [], [], []
    for name, library, clip_name, lx, ly, kx, ky, density in FX_CLIPS:
        if library == 'aura':
            start = len(images)
            fx_clip_frames.append(len(fx_frame_first))
            fx_frame_first.append(len(images))
            n = max(1, round(2 * AURA_RADIUS * scale / density))
            glow = Image.new('RGB', (n, n))
            glow.putdata([tuple(round(c * max(0.0, 1 - ((x + 0.5) / n * 2 - 1) ** 2 - ((y + 0.5) / n * 2 - 1) ** 2) ** 2)
                                for c in AURA_RGB) for y in range(n) for x in range(n)])
            box = [lx - AURA_RADIUS, ly - AURA_RADIUS, lx + AURA_RADIUS, ly + AURA_RADIUS]
            for part, part_box in fx_parts(glow, box, scale, density, premultiplied=True):
                images.append(part)
                boxes.append(part_box)
                art_sources.append('resources.assets:1987')
                fx_samples += [c for c in part.getdata() if light(c) >= FX_THRESHOLD]
            clips.append(dict(name=name, start=start, count=1, parts=len(images) - start, fps=1, wrap=2, loop=0))
            continue
        clip = by_name[clip_name] if library == 'knight' else next(
            c for c in source.read(resources.objects[library])['clips'] if c['name'] == clip_name)
        assert clip['wrapMode'] == 2, f'{clip_name} no longer plays once'
        start = len(images)
        fx_clip_frames.append(len(fx_frame_first))
        for frame in clip['frames']:
            obj = source.ref(resources, frame['spriteCollection'])
            image, box = tk_sprite(source, obj.assets_file, source.read(obj), frame['spriteId'], textures)
            fx_frame_first.append(len(images))
            # A child's local transform; a negative scale turns the corners round, which draws the frame mirrored.
            placed = (lx + kx * box[0], ly + ky * box[1], lx + kx * box[2], ly + ky * box[3])
            for part, part_box in fx_parts(image, placed, scale, density):
                images.append(part)
                boxes.append(part_box)
                art_sources.append(f'{source.sid(obj)}:{frame["spriteId"]}')
                fx_samples += [c for c in part.getdata() if light(c) >= FX_THRESHOLD]
        clips.append(dict(name=name, start=start, count=len(clip['frames']), parts=len(images) - start,
                          fps=int(clip['fps']), wrap=clip['wrapMode'], loop=0))
    fx_frame_first.append(len(images))

    # One CLUT per group of clips that fits a single 256x256 quantizer sheet, so
    # every frame of one animation keeps one palette.
    effect_names = {name for name, _, _ in EFFECT_CLIPS} | {'Soul Burst', 'Heal Flash'} | {name for name, _, _, _ in EFFECT_LIBRARY_CLIPS}
    groups, current = [], []
    for clip in clips:
        if clip['name'] in effect_names or clip['name'] in fx_names:
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
    bright = list(range(burst['start'], burst['start'] + BURST_BRIGHT_FRAMES)) + list(range(soul_start, soul_start + 5))
    current = []
    for clip in clips:
        if clip['name'] not in effect_names or clip['name'] == 'Heal Flash' or clip['name'] in fx_names:
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

    # The frames from the first streamed clip on are written to their pack's file, not to the linked blob.
    by_clip = {c['name']: c for c in clips}
    pack_ranges = []
    for pack, _, names in STREAM_PACKS:
        first, last = by_clip[names[0]], by_clip[names[-1]]
        for a, b in zip(names, names[1:]):
            assert by_clip[a]['start'] + by_clip[a].get('parts', by_clip[a]['count']) == by_clip[b]['start'], f'{pack}: {a} and {b} are not contiguous'
        pack_ranges.append((first['start'], last['start'] + last.get('parts', last['count'])))
    for (_, end), (start, _) in zip(pack_ranges, pack_ranges[1:]):
        assert end == start, 'the streamed packs are not back to back'
    resident_frames = pack_ranges[0][0]
    assert pack_ranges[-1][1] == len(images), 'frames follow the last streamed pack'
    pack_blobs = [bytearray() for _ in STREAM_PACKS]
    palettes, blob, frames = [], bytearray(), [None] * len(images)
    raw_bytes = 0
    for clut_index, group in enumerate(groups):
        positions = shelf([images[i].size for i in group])
        sheet = Image.new('RGBA', (256, 256))
        for i, origin in zip(group, positions):
            sheet.paste(images[i], origin)
        if clut_index == flash_clut:
            assert group == [flash_start] and positions == [(0, 0)]
            sw = 256
            palette = (focus_art / 'flash-pal.bin').read_bytes()
            packed = (focus_art / 'flash-sheet.4bpp').read_bytes()
            assert len(palette) == 32 and len(packed) == 256 * 256 // 2
        else:
            sw, _, palette, packed = (lambda s: quantize_additive_shared(s, BURST_BRIGHT_THRESHOLD, fx_samples, FX_PALETTE_BURST_WEIGHT) if clut_index == bright_clut else quantize_additive(s) if clut_index >= additive_from else quantize_alpha_coverage(s, 128))(sheet)
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
            assert len(texels) <= SLOT_BYTES and image.width <= 64 and image.height <= 64, f'ability frame {i} ({image.width}x{image.height}) exceeds one 64x64 animation slot'
            coded = packbits(bytes(texels))
            assert unpackbits(coded) == bytes(texels)
            target = blob
            for k, (start, end) in enumerate(pack_ranges):
                if start <= i < end:
                    target = pack_blobs[k]
            frames[i] = dict(offset=len(target), width=image.width, height=image.height,
                             bounds=boxes[i], clut=clut_index)
            target.extend(coded)
            raw_bytes += len(texels)

    # The effect parts carry no palette of their own: each texel takes the nearest entry of the bright
    # palette, which was fitted to them (and to the Burst), in colour.
    entries = [tuple((word >> shift & 31) * 255 // 31 for shift in (0, 5, 10))
               for word in struct.unpack('<16H', palettes[bright_clut])[1:] if word]
    nearest_cache = {}
    for clip in clips:
        if clip['name'] not in fx_names:
            continue
        for i in range(clip['start'], clip['start'] + clip['parts']):
            image = images[i]
            w, h = image.size
            data = list(image.getdata())
            stride = (w + 3) // 4 * 2
            texels = bytearray(stride * h)
            for y in range(h):
                for x in range(w):
                    colour = data[y * w + x]
                    if light(colour) >= FX_THRESHOLD:
                        texels[y * stride + x // 2] |= nearest_entry(colour, entries, nearest_cache) << ((x & 1) * 4)
            coded = packbits(bytes(texels))
            assert unpackbits(coded) == bytes(texels)
            assert len(texels) <= SLOT_BYTES and w <= FX_PART and h <= FX_PART, f'effect part {i} ({w}x{h}) exceeds one 64x64 animation slot'
            target = next(blob for k, (start, end) in enumerate(pack_ranges) if start <= i < end for blob in (pack_blobs[k],))
            frames[i] = dict(offset=len(target), width=w, height=h, bounds=boxes[i], clut=bright_clut)
            target.extend(coded)
            raw_bytes += len(texels)

    palette_bytes = b''.join(palettes)
    payload = bytes(palette_bytes) + bytes(blob)
    (ROOT / 'data/ability-art.hk').write_bytes(payload)
    pack_payloads = []
    for (_, path, _), (start, end), coded in zip(STREAM_PACKS, pack_ranges, pack_blobs):
        table = b''.join(struct.pack(STREAM_RECORD, f['offset'], f['width'], f['height'], f['clut'],
                                     *(round(v * 65536) for v in f['bounds'])) for f in frames[start:end])
        pack_payloads.append(table + bytes(coded))
        (ROOT / path).write_bytes(pack_payloads[-1])
    lines = [
        '// Generated Knight ability art; no embedded retail source dump.',
        f'pub const CLUT_RECT:(u16,u16,u16,u16)=({CLUT[0]},{CLUT[1]},{CLUT[2]},{CLUT[3]});',
        f'pub const PALETTE_BYTES:usize={len(palette_bytes)};',
        f'pub const PALETTE_COUNT:usize={len(palettes)};',
        'pub const CLUT_Y:&[u16]=&[' + ','.join(str(FLASH_CLUT_Y if i == flash_clut else CLUT[1] + i) for i in range(len(palettes))) + '];',
        'pub const ABILITY_CLIPS:&[Clip]=&[' + ','.join(
            'Clip{' + ','.join(f'{k}:{c[k]}' for k in ('start', 'count', 'fps', 'wrap')) + '}'
            for c in clips) + '];',
        # tk2d loopStart of every clip, which only a wrap-1 (loop section) clip reads.
        'pub const ABILITY_LOOP:&[u8]=&[' + ','.join(str(c['loop']) for c in clips) + '];',
        'pub const ABILITY_FRAMES:&[Frame]=&[' + ','.join(
            'Frame{offset:%d,width:%d,height:%d,clut:%d,bounds:[%s]}' % (
                f['offset'], f['width'], f['height'], f['clut'],
                ','.join(str(round(v * 65536)) for v in f['bounds']))
            for f in frames[:resident_frames]) + '];',
        f'pub const FRAME_COUNT:usize={len(frames)};',
        '/// Each streamed pack\'s (first frame, frame count), in `STREAM_PACKS` order.',
        'pub const STREAM_PACKS:&[(usize,usize)]=&[' + ','.join(f'({start},{end - start})' for start, end in pack_ranges) + '];',
    ]
    lines.append(f'pub const FOCUS_EFFECT_LOOP_START:usize={effect_loop};')
    # The effect clips' frames: a frame is `FX_FRAMES[n]..FX_FRAMES[n + 1]` of the parts it was cut into, and clip
    # `FX_FIRST_CLIP + k` starts at frame `FX_CLIP_FRAMES[k]`.
    lines.append(f'pub const FX_FIRST_CLIP:usize={clips.index(by_clip[FX_CLIPS[0][0]])};')
    lines.append('pub const FX_CLIP_FRAMES:&[u16]=&[' + ','.join(map(str, fx_clip_frames)) + '];')
    lines.append('pub const FX_FRAMES:&[u16]=&[' + ','.join(map(str, fx_frame_first)) + '];')
    code = '\n'.join(lines) + '\n'
    (ROOT / 'data/ability-art.rs').write_text(code)
    report = {'clips': clips, 'frames': len(frames), 'palettes': len(palettes),
              'payload_bytes': len(payload), 'stream_packs': {pack: len(data) for (pack, _, _), data in zip(STREAM_PACKS, pack_payloads)}, 'texel_bytes_uncoded': raw_bytes, 'clut_rect': list(CLUT), 'clut_rows': CLUT_ROWS,
              'animation': source.sid(anim_obj), 'art_sources': art_sources,
              'payload_sha256': hashlib.sha256(payload).hexdigest(),
              'storage': 'run-length coded per frame, in linked RAM or (STREAM_PACKS) in carried pool modules read from the disc; frames are decoded into VRAM through the shared 64x64 animation slots',
              'limitations': ['The Crystal Heart has no clip here: its SD set is a separate '
                              'charge, dash and wall-hit sequence that P15 has not modelled.']}
    dump(ROOT / '.hkpsx/ability-art.json', report)
    print(f'Ability art: {len(frames)} original frames across {len(clips)} clips, '
          f'{len(palettes)} palettes, {len(payload)} linked bytes, streamed packs '
          + ', '.join(f'{pack} {len(data)}' for (pack, _, _), data in zip(STREAM_PACKS, pack_payloads)))

if __name__ == '__main__':
    cook()
