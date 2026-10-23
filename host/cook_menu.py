"""Cook a bounded static title screen from the user's Windows retail UI assets."""
import hashlib
import io
import json
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageEnhance, ImageFont, ImageOps
from source import ROOT, Source, dump


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def named(source, filename, kind, name):
    matches = []
    for obj in source.file(filename).objects.values():
        if obj.type.name == kind:
            value = obj.read()
            if value.m_Name == name:
                matches.append((obj, value))
    if len(matches) != 1:
        raise ValueError(f'Expected one {filename}/{kind}/{name}, found {len(matches)}')
    return matches[0]


# The title styles the guest carries (hk-cook title-style: `title_style::SHIPPED`, in the order the
# options list them): the composed background of each, and the blob it becomes.
STYLES = [('Classic_Style', 'menu-bg-0.hk'), ('HD_Style', 'menu-bg-1.hk'), ('Grimm_Style', 'menu-bg-2.hk'),
          ('GG_Style', 'menu-bg-3.hk'), ('Infected_Style', 'menu-bg-4.hk'), ('Shade_Style', 'menu-bg-5.hk'),
          ('Steel_Style', 'menu-bg-6.hk'), ('Brilliant_Style', 'menu-bg-7.hk'),
          ('VHeart_menu_Style', 'menu-bg-8.hk'), ('Modern_Style', 'menu.hk')]


def style_blob(canvas, font, advances):
    """One style's HKMENU03 blob: its palette, its 320x240 background as two 8bpp pages and the
    loading/retry prompts cut from it. Returns (blob, palette-applied indexed preview)."""
    clean = canvas.copy()
    # Smooth glows band badly in 15-bit colour, so the palette is chosen first and then the picture is
    # error-diffused onto it as the console will show it (each entry cut to 5 bits a channel): the
    # steps become fine noise instead of contour lines.
    rough = canvas.convert('RGB').quantize(colors=256, method=Image.Quantize.MEDIANCUT,
                                         dither=Image.Dither.NONE)
    rough_palette = rough.getpalette('RGB')
    rough_palette += [0] * (768 - len(rough_palette))
    shown = [((c >> 3) * 255 // 31) for c in rough_palette]
    reference = Image.new('P', (1, 1))
    reference.putpalette(shown)
    indexed = canvas.convert('RGB').quantize(palette=reference, dither=Image.Dither.FLOYDSTEINBERG)
    palette = indexed.getpalette('RGB')
    palette += [0] * (768 - len(palette))
    words = []
    for i in range(256):
        r, g, b = palette[i * 3:i * 3 + 3]
        word = (r >> 3) | ((g >> 3) << 5) | ((b >> 3) << 10)
        words.append(word or 0x8000)  # Opaque black, never transparent texel 0000.
    # Two 8bpp texture pages: 256x240 and 64x240. Pixels remain one-to-one.
    tiles = indexed.crop((0, 0, 256, 240)).tobytes() + indexed.crop((256, 0, 320, 240)).tobytes()
    for label, size in [('LOADING...', 17), ('DISC ERROR - START TO RETRY', 12)]:
        patch = clean.crop((64, 177, 256, 209)).convert('RGB')
        patch_face = ImageFont.truetype(io.BytesIO(bytes(font.m_FontData)), size)
        pd = ImageDraw.Draw(patch)
        box = pd.textbbox((0, 0), label, font=patch_face)
        pd.text(((192 - box[2] + box[0]) // 2 - box[0], (32 - box[3] + box[1]) // 2 - box[1]),
                label, font=patch_face, fill=(230, 234, 240))
        tiles += patch.quantize(palette=indexed, dither=Image.Dither.NONE).tobytes()
    blob = struct.pack('<8sHHI', b'HKMENU03', 320, 240, len(tiles))
    blob += struct.pack('<256H', *words) + tiles + bytes(advances) + b'\0'
    return blob


def cook():
    source = Source()
    font_obj, font = named(source, 'resources.assets', 'Font', 'Perpetua')
    # The glyph advances come with the strip, from hk-cook dialogue-art (host/hk-cook/src/font_strip.rs).
    advances = json.loads((ROOT/'data/font-metrics.json').read_text())['advances']
    if len(advances)!=95 or not all(1<=a<=13 for a in advances):raise ValueError('data/font-metrics.json has no usable advances')
    blobs = []
    for style, filename in STYLES:
        # hk-cook title-style composes the style's own sprites (glows, light shafts, picture and vignette)
        # through the title camera; the logo is a separate piece that the guest draws and fades.
        composed = ROOT / '.hkpsx/title-styles' / f'{style}.png'
        if not composed.is_file():
            raise ValueError(f'{composed} is missing: run `hk-cook title-style` first')
        canvas = Image.open(composed).convert('RGBA')
        if canvas.size != (320, 240):
            raise ValueError(f'{composed} is {canvas.size}, not 320x240')
        blob = style_blob(canvas, font, advances)
        (ROOT / 'data').mkdir(exist_ok=True)
        (ROOT / 'data' / filename).write_bytes(blob)
        blobs.append((style, filename, blob))
    # The default style (the last, `title_style::DEFAULT_STYLE`) is data/menu.hk: it rides the boot chunk's
    # first part, the others are read when the options page picks them.
    blob = blobs[-1][2]
    if any(len(b) != len(blob) for _, _, b in blobs):raise ValueError('every style blob must have the same size')
    # The blobs are disc chunks read before the title, not linked RAM; the guest keeps only their
    # size and checksum (to verify the read) and the glyph advances, which the in-game text layout
    # needs after the title is gone.
    fnv = 0x811c9dc5
    for byte in blob:
        fnv = ((fnv ^ byte) * 0x01000193) & 0xffffffff
    (ROOT / 'data/menu.rs').write_text(
        '// Generated by host/cook_menu.py.\n'
        f'pub const MENU_BYTES:usize={len(blob)};\n'
        f'pub const MENU_CHECKSUM:u32={fnv};\n'
        f'pub const MENU_METRICS:[u8;96]=[{",".join(str(b) for b in blob[-96:])}];\n')
    inputs = {}
    for filename in sorted(set(source.files) | {Path(k).name for k in source.env.files}):
        path = source.directory / filename
        if path.is_file():
            inputs[filename] = {'bytes': path.stat().st_size, 'sha256': sha(path)}
    dump(ROOT / '.hkpsx/menu-provenance.json', {
        'source': str(source.directory), 'inputs': inputs,
        'objects': [{'id': source.sid(font_obj), 'name': font.m_Name}],
        'output': {'path': 'data/menu.hk', 'bytes': len(blob), 'sha256': sha(ROOT / 'data/menu.hk')},
        'styles': [{'style': style, 'path': f'data/{filename}', 'sha256': sha(ROOT / 'data' / filename),
                    'composition': sha(ROOT / '.hkpsx/title-styles' / f'{style}.png')}
                   for style, filename, _ in blobs],
        'composition': 'Each style is the original title scene\'s own sprites seen through its camera '
                       '(hk-cook title-style), shared original Perpetua glyphs. Functional title choices; '
                       'not an emulation of the retail menu transitions or animated effects.',
        'format': 'HKMENU03: 16-byte <8sHHI header, 512-byte RGB555 palette, '
                  '61440-byte left 256x240 tile, 15360-byte right 64x240 tile, '
                  'two 6144-byte 192x32 loading/retry prompt tiles, 95 glyph advances and one zero byte.',
        'shared_font': {'path':'data/read_font.hk','bytes':12832,'source':source.sid(font_obj),
                        'glyphs':'ASCII32..126, source Perpetua 12 px, hk-cook dialogue-art (font_strip.rs)'},
        'settings':{'sfx':[0,10],'ambience':[0,10],'default':10,'persistence':'current session only'},
        'vram_bytes': 89600,
        'shared_glyph_vram_bytes':12832,
    })
    print(f'Cooked title menu: {len(STYLES)} styles of {len(blob)} bytes, 89600 title VRAM bytes plus the 12832-byte font image.')


if __name__ == '__main__':
    cook()
