"""The original's per-scene colour grading and hero light, baked for the PS1.

Every frame the original renders the world camera, then runs its image
effects (docs/VISUALS.md has the evidence):

* Scenery drawn with `Sprites/Lit` is multiplied by `RenderSettings.ambientLight`,
  which `SceneManager.SetLighting` sets to `defaultColor * lerp(1, defaultIntensity, 0.5)`.
  Actors (tk2d) are unlit.
* `ColorCorrectionCurves` (the simple shader) looks each channel up in the
  scene's `redChannel`/`greenChannel`/`blueChannel` curves, then lerps from
  luminance by `saturation + 0.17` (`SceneManager.AdjustSaturationForPlatform`).
* `HeroLight`, a 3x scaled `light_effect_v02` sprite behind the Knight, is a
  Linear Light blend tinted by the scene's `heroLightColor`.

* Haze, fog and beam sprites use `UI/BlendModes/Screen` (B + F(1 - B)) or
  `LinearDodge` (B + F), lerped by alpha, and are unlit.

All of these are per-scene constants, so the palette part is applied to the
scene palette atlases when they are packed (no runtime cost): static scenery
palettes get ambient then curves and saturation, palettes only streamed
textures use (the Knight, enemies, NPCs) get curves and saturation. The
hero light becomes a per-scene additive colour for the guest's light fan
(data/scene_grading.rs).

Screen and LinearDodge scenery becomes additive (`additive_palettes`): every
visible texel of its palettes Adds, as the guest already draws semi-transparent
scenery texels, so the same pixels are drawn and only the blend changes. Its
palettes skip the ambient (the shaders are unlit) and take the camera pass as
an increment over the scene's mean background (`grade_additive_word`).
"""
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / '.hkpsx/scene-grading.json'
RUST = ROOT / 'data/scene_grading.rs'
FORMAT = 'HKSCENEGRADING03'
# SceneManager.AdjustSaturationForPlatform adds this on every platform.
PLATFORM_SATURATION = 0.17
# SceneManager.AmbientIntesityMix: ambient = colour * lerp(1, intensity, 0.5).
AMBIENT_MIX = 0.5
# UnityCG Luminance() in gamma colour space, as the curves shader uses it.
LUMA = (0.22, 0.707, 0.071)
# light_effect_v02's texels are one colour (185, 200, 246); only alpha varies.
LIGHT_TEXEL = (185 / 255, 200 / 255, 246 / 255)
# Linear Light clamps base + 2*blend - 1 at 1, hardest on the white light's
# blue (2*246/255 - 1 = 0.93): over brighter scenery it lifts toward white
# rather than toward blue. Measured before the camera pass at texture alpha 1:
# King's Pass cave (78, 97, 145), unclamped (the formula gives 71, 89, 146);
# King's Pass fog (40, 46, 61) and Crossroads_01 (70, 83, 102), clamped. The
# white light keeps this per-channel share of heroLight.a * (2*texel - 1), the
# clamped fits' mean, so it reads white-blue as on most screens; Greenpath's
# tinted light stays under the clamp (its capture matched within 10%) and
# keeps all of it. Graded as an increment over dark scenery (LIGHT_BASE).
LIGHT_CLAMP_SHARE = (0.905, 0.815, 0.605)
LIGHT_BASE = 0.05
# Scenery shaders by how their palettes may be graded. Screen and LinearDodge
# make a palette additive. The bank shares one texture between sprites with
# the same pixels, so haze art is also drawn by LinearLight lamp glows and
# Sprites/Default copies: those unlit users may share an additive palette
# (Linear Light's 2F - 1 is then approximated by F). A lit user keeps the
# palette lit.
BLEND_SHADERS = {'UI/BlendModes/Screen': 'screen', 'UI/BlendModes/LinearDodge': 'dodge',
                 'UI/BlendModes/LinearLight': 'linear_light', 'Sprites/Default': 'unlit'}
ADDITIVE = ('screen', 'dodge')
# Vignette scale the hero's Darkness Control FSM sets per darknessLevel.
VIGNETTE_SCALE = {-1: 10.5, 0: 5.5, 1: 2.2, 2: 0.8}
# The original darkens by B*(1-alpha); the PS1 can only subtract. Subtracting
# alpha times the scene's mean graded scenery colour (scaled by this gain)
# darkens a typical pixel by the right amount and keeps its hue, where a grey
# subtraction crushed red and green first on blue scenes.
VIGNETTE_GAIN = 1.25
VIGNETTE_GREY = 0.12


def evaluate(keys, t):
    """Unity AnimationCurve.Evaluate for non-weighted keys: cubic Hermite."""
    if t <= keys[0]['time']:
        return keys[0]['value']
    if t >= keys[-1]['time']:
        return keys[-1]['value']
    for a, b in zip(keys, keys[1:]):
        if a['time'] <= t <= b['time']:
            dt = b['time'] - a['time']
            if dt <= 0:
                return b['value']
            s = (t - a['time']) / dt
            m0, m1 = a['outSlope'] * dt, b['inSlope'] * dt
            return ((2 * s ** 3 - 3 * s ** 2 + 1) * a['value'] + (s ** 3 - 2 * s ** 2 + s) * m0
                    + (-2 * s ** 3 + 3 * s ** 2) * b['value'] + (s ** 3 - s ** 2) * m1)
    return keys[-1]['value']


class Grade:
    """One scene's colour pipeline in float sRGB-gamma space, 0..1."""

    def __init__(self, record):
        self.record = record
        self.saturation = record['saturation'] + (0 if record['ignorePlatformSaturationModifiers'] else PLATFORM_SATURATION)
        # ColorCorrectionCurves.UpdateParameters: texel floor(255 t) = curve(t), clamped.
        self.lut = [[min(1.0, max(0.0, evaluate(record[name]['m_Curve'], i / 255))) for i in range(256)]
                    for name in ('redChannel', 'greenChannel', 'blueChannel')]
        mix = 1 + (record['defaultIntensity'] - 1) * AMBIENT_MIX
        self.ambient = tuple(record['defaultColor'][c] * mix for c in 'rgb')

    def camera(self, rgb):
        """The camera pass: point-sampled curves, then saturation about luminance."""
        out = [self.lut[c][min(255, int(rgb[c] * 256))] for c in range(3)]
        lum = sum(w * v for w, v in zip(LUMA, out))
        return [min(1.0, max(0.0, lum + (v - lum) * self.saturation)) for v in out]

    def scenery(self, rgb):
        return self.camera([v * a for v, a in zip(rgb, self.ambient)])

    def light(self):
        """Additive hero-light colour at texture alpha 1, after the camera pass (0..1)."""
        h = self.record['heroLightColor']
        tint = (h['r'], h['g'], h['b'])
        shares = LIGHT_CLAMP_SHARE if min(tint) >= 0.999 else (1.0, 1.0, 1.0)
        add = [max(0.0, h['a'] * (2 * t * c - 1) * share) for t, c, share in zip(LIGHT_TEXEL, tint, shares)]
        base = self.camera([LIGHT_BASE] * 3)
        lit = self.camera([LIGHT_BASE + v for v in add])
        return [max(0.0, a - b) for a, b in zip(lit, base)]


def grade_word(word, grade, lit):
    """One BGR555 palette word. Zero stays transparent, STP is kept, and the
    exact black-mask words (0, 1, 0x8000) are left alone."""
    if word & 0x7FFF <= 1:
        return word
    rgb = [((word >> s) & 31) / 31 for s in (0, 5, 10)]
    out = grade.scenery(rgb) if lit else grade.camera(rgb)
    r, g, b = (min(31, int(v * 31 + 0.5)) for v in out)
    graded = r | g << 5 | b << 10
    # A visible colour must not become the transparent word.
    return (word & 0x8000) | (graded or 1)


def streamed_palettes(raw, entry):
    """Palette indices that only streamed (animation) textures use: actors."""
    s = entry['sections']
    streamed, static = set(), set()
    for i in range(entry['textures']):
        page, *_rest, palette, _tail = struct.unpack_from('<6HI', raw, s['textures']['offset'] + i * 16)
        (streamed if page == 65535 else static).add(palette)
    return streamed - static


def grade_additive_word(word, grade, kind, background):
    """One additive palette word: the light it adds to the scene's mean
    background (pre-camera, 0..1), taken through the camera pass, as the
    increment over that background. Screen adds F(1 - B), LinearDodge F."""
    if word & 0x7FFF == 0:
        return word
    add = [((word >> s) & 31) / 31 for s in (0, 5, 10)]
    if kind == 'screen':
        add = [v * (1 - b) for v, b in zip(add, background)]
    base = grade.camera(background)
    lit = grade.camera([min(1.0, b + v) for b, v in zip(background, add)])
    r, g, b = (min(31, int(max(0.0, l - o) * 31 + 0.5)) for l, o in zip(lit, base))
    return 0x8000 | r | g << 5 | b << 10


def grade_palette_chunk(chunk, first, grade, actors, additive=None, background=None):
    """Grade a bootstrap palette atlas chunk whose first record is palette `first`.
    `additive`: palette -> blend kind (`additive_palettes`)."""
    additive = additive or {}
    out = bytearray(chunk)
    for p in range(len(chunk) // 32):
        words = struct.unpack_from('<16H', chunk, p * 32)
        kind = additive.get(first + p)
        if kind:
            graded = [grade_additive_word(w, grade, kind, background) for w in words]
        else:
            lit = first + p not in actors
            graded = [grade_word(w, grade, lit) for w in words]
        struct.pack_into('<16H', out, p * 32, *graded)
    return bytes(out)


def additive_palettes(raw, entry, meta, kinds):
    """Make the bank's Screen/LinearDodge scenery palettes additive.

    `kinds`: renderer source id -> BLEND_SHADERS kind, for the scene (`cook`).
    A palette qualifies when a Screen or LinearDodge draw uses it and every
    texture using it is static and drawn only by unlit BLEND_SHADERS draws;
    one with a lit or unknown user is left alone. Its kind is screen when any
    user is Screen. Every visible word gets STP, so the texel Adds; opaque words keep
    their colour (alpha 224 or more: straight is nearly premultiplied). The
    bank's own certificates (opaque cores, coverage) then see these texels as
    the see-through ones they now are. Returns (raw, {palette: kind}).
    """
    if not any(v in ADDITIVE for v in kinds.values()):
        return raw, {}
    s = entry['sections']
    textures = [struct.unpack_from('<6HI', raw, s['textures']['offset'] + t * 16) for t in range(entry['textures'])]
    users = {}
    drawn = set()
    for local, room in enumerate(entry['source_rooms']):
        desc = struct.unpack_from('<10I', raw, s['rooms']['offset'] + 40 * local)
        draws = json.loads((ROOT / f"data/regions/region-{room['chunk_id']:03}/scene.json").read_text())['draws']
        if desc[0] != room['chunk_id'] or desc[1] != len(draws):
            raise ValueError('additive palettes: bank room reference mismatch')
        for i, d in enumerate(draws):
            gid = struct.unpack_from('<H', raw, desc[5] + i * 2)[0]
            tex = struct.unpack_from('<H', raw, s['draws']['offset'] + gid * 44)[0]
            drawn.add(tex)
            users.setdefault(textures[tex][5], set()).add(kinds.get(d['source']) if textures[tex][0] != 65535 else None)
    for t, record in enumerate(textures):
        if t not in drawn:
            users.setdefault(record[5], set()).add(None)
    chosen = {p: 'screen' if 'screen' in k else 'dodge' for p, k in users.items()
              if None not in k and any(v in ADDITIVE for v in k)}
    out = bytearray(raw)
    for p in chosen:
        at = s['palettes']['offset'] + p * 32
        words = struct.unpack_from('<16H', out, at)
        struct.pack_into('<16H', out, at, *((w | 0x8000) if w & 0x7FFF else w for w in words))
    return bytes(out), chosen


def scenery_background(raw, entry, grade, actors, additive):
    """Mean pre-camera colour (0..1) of the scene's lit static palette words:
    the background the additive palettes are graded over."""
    s = entry['sections']
    total, count = [0.0, 0.0, 0.0], 0
    for p in range(entry['palettes']):
        if p in actors or p in additive:
            continue
        for word in struct.unpack_from('<16H', raw, s['palettes']['offset'] + p * 32):
            if word & 0x7FFF <= 1:
                continue
            total = [t + ((word >> sh) & 31) / 31 * a for t, sh, a in zip(total, (0, 5, 10), grade.ambient)]
            count += 1
    return [t / count for t in total] if count else [0.0, 0.0, 0.0]


def scenery_mean(raw, entry, grade, actors, additive=()):
    """Mean graded colour (0..1) of the scene's visible static palette words:
    what the vignette subtracts in proportion to, so darkening keeps the hue."""
    s = entry['sections']
    total, count = [0.0, 0.0, 0.0], 0
    for p in range(entry['palettes']):
        if p in actors or p in additive:
            continue
        for word in struct.unpack_from('<16H', raw, s['palettes']['offset'] + p * 32):
            if word & 0x7FFF <= 1:
                continue
            out = grade.scenery([((word >> sh) & 31) / 31 for sh in (0, 5, 10)])
            total = [t + v for t, v in zip(total, out)]
            count += 1
    return [t / count for t in total] if count else [0.0, 0.0, 0.0]


def cook(meta):
    """Read each scene's SceneManager from the Steam data (host/source.py)."""
    import source as S
    src = S.Source()
    scenes = {}
    for scene in meta['scenes']:
        f = src.file(scene['file'])
        names = {}
        record = None
        for o in f.objects.values():
            if o.type.name != 'MonoBehaviour':
                continue
            head = o.parse_monobehaviour_head()
            key = (head.m_Script.m_FileID, head.m_Script.m_PathID)
            if key not in names:
                names[key] = head.m_Script.read().m_ClassName
            if names[key] == 'SceneManager':
                tree = src.read(o)
                record = {k: tree[k] for k in ('saturation', 'ignorePlatformSaturationModifiers', 'redChannel', 'greenChannel',
                                               'blueChannel', 'defaultColor', 'defaultIntensity', 'heroLightColor', 'darknessLevel',
                                               'noLantern', 'sceneType')}
                record['source'] = src.sid(o)
                break
        if record is None:
            raise ValueError('No SceneManager in ' + scene['scene_name'])
        scenes[str(scene['scene_id'])] = dict(scene_name=scene['scene_name'], file=scene['file'], scene_manager=record,
                                              blend=blend_sources(src, meta, scene['scene_id']))
    report = {'format': FORMAT, 'source': 'SceneManager components and scenery shaders of the Steam build (host/source.py)',
              'scenes': scenes}
    REPORT.write_text(json.dumps(report, indent=1) + '\n')
    return report


def blend_sources(src, meta, scene_id):
    """Renderer source id -> BLEND_SHADERS kind for the scene's cooked scenery
    draws whose material uses one of those shaders (absent: lit or other)."""
    out, shaders = {}, {}
    for row in meta['regions']:
        if row['scene_id'] != scene_id:
            continue
        for d in json.loads((ROOT / f"data/regions/region-{row['chunk_id']:03}/scene.json").read_text())['draws']:
            sid = d['source']
            if sid in shaders:
                continue
            name, pid = sid.rsplit(':', 1)
            o = src.file(name).objects.get(int(pid))
            shader = None
            if o is not None and o.type.name == 'SpriteRenderer':
                refs = src.read(o)['m_Materials']
                if refs:
                    mo = src.ref(o.assets_file, refs[0])
                    so = src.ref(mo.assets_file, src.read(mo)['m_Shader'])
                    tree = src.read(so)
                    shader = tree.get('m_ParsedForm', {}).get('m_Name', tree.get('m_Name'))
            shaders[sid] = shader
            if shader in BLEND_SHADERS:
                out[sid] = BLEND_SHADERS[shader]
    return out


def load(meta):
    """Cached gradings for the scenes in `meta`, cooking them on a miss."""
    if not meta.get('scenes'):
        return {'format': FORMAT, 'scenes': {}}
    if REPORT.is_file():
        report = json.loads(REPORT.read_text())
        if report.get('format') == FORMAT and {str(s['scene_id']) for s in meta['scenes']} <= set(report['scenes']):
            return report
    return cook(meta)


def gradings(meta):
    report = load(meta)
    return {int(k): Grade(v['scene_manager']) for k, v in report['scenes'].items()}


def write_rust(meta, means=None):
    """Per-scene hero light colour, vignette scale and vignette colour for the
    guest, by scene id. `means`: scene id -> scenery_mean, from the pack."""
    report = load(meta)
    means = means or {}
    rows = []
    for scene_id in range(max(int(k) for k in report['scenes']) + 1):
        v = report['scenes'].get(str(scene_id))
        if v is None:
            rows.append('SceneLight{rgb:[0,0,0],vignette_q8:0,vignette_rgb:[0,0,0]},')
            continue
        sm = v['scene_manager']
        light = [min(255, int(c * 255 + 0.5)) for c in Grade(sm).light()]
        scale = VIGNETTE_SCALE.get(sm['darknessLevel'], 5.5) if sm['sceneType'] == 0 else 0
        mean = means.get(scene_id, [VIGNETTE_GREY] * 3)
        vig = [min(255, int(c * 255 * VIGNETTE_GAIN + 0.5)) for c in mean]
        rows.append(f"SceneLight{{rgb:[{light[0]},{light[1]},{light[2]}],vignette_q8:{int(scale * 256 + 0.5)},"
                    f"vignette_rgb:[{vig[0]},{vig[1]},{vig[2]}]}}, // {v['scene_name']}")
    text = ('// Generated by host/scene_grading.py from each scene\'s SceneManager.\n'
            '// rgb: additive hero-light colour at full texture alpha, after the camera grade.\n'
            '// vignette_q8: the hero vignette\'s scale for the scene darkness level (0: none).\n'
            '// vignette_rgb: what full vignette alpha subtracts (the scene\'s mean graded scenery colour).\n'
            '#[derive(Clone,Copy)]\npub struct SceneLight{pub rgb:[u8;3],pub vignette_q8:u16,pub vignette_rgb:[u8;3]}\n'
            'pub const SCENE_LIGHTS:&[SceneLight]=&[\n' + '\n'.join(rows) + '\n];\n')
    if not RUST.is_file() or RUST.read_text() != text:
        RUST.write_text(text)
    return RUST
