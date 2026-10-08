"""Cook a bounded static title screen from the user's Windows retail UI assets."""
import hashlib
import io
import math
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


def save_previews(background,face,advances,directory):
    def width(value):return sum(advances[ord(c)-32] for c in value)
    def text(im,x,y,value,gain=128):
        for c in value:
            glyph=Image.new('L',(12,12));ImageDraw.Draw(glyph).text((0,-1),c,font=face,fill=255)
            values=list(glyph.get_flattened_data());out=Image.new('RGBA',(12,12))
            out.putdata([(((((v+8)//17)*17>>3)*gain>>7)*8,)*3+(255 if (v+8)//17 else 0,) for v in values])
            im.paste(out,(x,y),out);x+=advances[ord(c)-32]
    def center(im,y,value,gain=128):text(im,160-width(value)//2,y,value,gain)
    main=background.copy()
    for i,label in enumerate(('Start Game','Options','Controls')):center(main,151+i*19,label,128 if i==0 else 96)
    text(main,144-width('Start Game')//2,151,'>');center(main,222,'Up/Down: choose    X/START: select')
    main.save(directory/'menu-preview.png')
    options=background.copy();center(options,143,'OPTIONS')
    for i,label in enumerate(('Sound effects','Ambience','Back')):
        text(options,86,161+i*17,label,128 if i==0 else 96)
        if i<2:text(options,215,161+i*17,'100%',128 if i==0 else 96)
    text(options,70,161,'>');center(options,221,'Left/Right: volume    O: back')
    options.save(directory/'menu-options-preview.png')
    controls=background.copy();d=ImageDraw.Draw(controls);d.rectangle((16,39,303,230),fill=(64,64,64));d.rectangle((18,41,301,228),fill=(0,0,0))
    center(controls,51,'CONTROLS')
    lines=('D-pad: move','X: jump','Square: swing nail','Hold O: Focus / heal','Up + Square: upward nail','Airborne Down + Square: downward nail','Up/Down at tablet: inspect','Start: pause / resume','Select: reset session')
    for i,line in enumerate(lines):text(controls,30,74+i*14,line)
    center(controls,214,'X / O / START: back');controls.save(directory/'menu-controls-preview.png')


def cook():
    source = Source()
    logo_obj, logo = named(source, 'sharedassets1.assets', 'Sprite', 'title')
    bg_obj, background = named(source, 'sharedassets1.assets', 'Sprite', 'Voidheart_menu_BG')
    font_obj, font = named(source, 'resources.assets', 'Font', 'Perpetua')
    # Title UI is a static composition; this does not flatten the gameplay room.
    canvas = ImageOps.fit(background.image.convert('RGB'), (320, 240),
                          method=Image.Resampling.LANCZOS)
    canvas = ImageEnhance.Brightness(canvas).enhance(0.38).convert('RGBA')
    mark = logo.image.convert('RGBA')
    mark = mark.resize((278, round(mark.height * 278 / mark.width)), Image.Resampling.LANCZOS)
    canvas.alpha_composite(mark, ((320 - mark.width) // 2, 40))
    clean = canvas.copy()
    face = ImageFont.truetype(io.BytesIO(bytes(font.m_FontData)), 12)
    advances = [max(1, math.ceil(face.getlength(chr(c)))) for c in range(32,127)]
    if max(advances)>12:raise ValueError('Menu source font exceeds shared glyph cells')
    indexed = canvas.convert('RGB').quantize(colors=256, method=Image.Quantize.MEDIANCUT,
                                           dither=Image.Dither.NONE)
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
    out = ROOT / 'data/menu.hk'
    out.parent.mkdir(exist_ok=True)
    out.write_bytes(blob)
    # The blob is a disc chunk read before the title, not linked RAM; the guest
    # keeps only its size and checksum (to verify the read) and the glyph
    # advances, which the in-game text layout needs after the title is gone.
    fnv = 0x811c9dc5
    for byte in blob:
        fnv = ((fnv ^ byte) * 0x01000193) & 0xffffffff
    (ROOT / 'data/menu.rs').write_text(
        '// Generated by host/cook_menu.py.\n'
        f'pub const MENU_BYTES:usize={len(blob)};\n'
        f'pub const MENU_CHECKSUM:u32={fnv};\n'
        f'pub const MENU_METRICS:[u8;96]=[{",".join(str(b) for b in blob[-96:])}];\n')
    # Preview includes PS1's actual 5-bit palette reduction.
    indexed.putpalette([c for word in words for c in
                        (((word & 31) * 255 // 31), (((word >> 5) & 31) * 255 // 31),
                         (((word >> 10) & 31) * 255 // 31))])
    preview=indexed.convert('RGB')
    save_previews(preview,face,advances,ROOT/'data')
    inputs = {}
    for filename in sorted(set(source.files) | {Path(k).name for k in source.env.files}):
        path = source.directory / filename
        if path.is_file():
            inputs[filename] = {'bytes': path.stat().st_size, 'sha256': sha(path)}
    dump(ROOT / '.hkpsx/menu-provenance.json', {
        'source': str(source.directory), 'inputs': inputs,
        'objects': [{'id': source.sid(obj), 'name': value.m_Name}
                    for obj, value in ((logo_obj, logo), (bg_obj, background), (font_obj, font))],
        'output': {'path': 'data/menu.hk', 'bytes': len(blob), 'sha256': sha(out)},
        'composition': 'Original title and Voidheart background, cropped to 320x240, '
                       'background brightness 0.38, shared original Perpetua glyphs. Functional title choices; '
                       'not an emulation of the retail menu transitions or animated effects.',
        'format': 'HKMENU03: 16-byte <8sHHI header, 512-byte RGB555 palette, '
                  '61440-byte left 256x240 tile, 15360-byte right 64x240 tile, '
                  'two 6144-byte 192x32 loading/retry prompt tiles, 95 glyph advances and one zero byte.',
        'shared_font': {'path':'data/read_font.hk','bytes':7712,'source':source.sid(font_obj),
                        'glyphs':'ASCII32..126, source Perpetua12, same recipe as read_points.py'},
        'settings':{'sfx':[0,10],'ambience':[0,10],'default':10,'persistence':'current session only'},
        'vram_bytes': 89600,
        'shared_glyph_vram_bytes':7712,
    })
    print(f'Cooked title menu: {len(blob)} bytes, 89600 title VRAM bytes plus shared7712-byte glyph reservation.')


if __name__ == '__main__':
    cook()
