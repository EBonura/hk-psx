"""Streaming code modules: enemy families and bosses that travel with their rooms.

A module is the code one or more rooms need and the rest of the game does not:
a boss, or an enemy family. It is not linked to run anywhere in particular.
Module k is linked at its own address in a link-only space, LINK_BASE +
k * LINK_STRIDE (an alias of low RAM nothing ever executes), and the guest
copies it to wherever the code pool has room, then relocates it
(game/src/modules.rs). Every room's disc group starts with one code chunk: the
packages of the modules its actors need, so the scene streamer's predictive
prefetch brings a room's code in with the rest of the room, and the guest
installs it in the background before the Knight reaches the gate.

Relocation is kept to four cases, all checked here:
- J26 (`j`/`jal`) into the module: found by scanning the module's code and
  its trampoline area, so the hazard patcher's own jumps are covered;
- HI16/LO16 pairs for the module's own tables: a module is under 64 KiB and
  linked at an address whose low half is 0x8000, so every HI16 holds the same
  value, and the guest never places one across a 64 KiB `lui` window
  boundary, so it can rewrite every HI16 to the window it landed in and
  shift every LO16 by one constant;
- W32 words (switch tables) into the module.
A module may not refer to another module.

Resident code reaches a module only through import sites: every resident
`j`/`jal` (in .text or in the hazard trampolines) and every resident data word
that targets a module's link space. The build writes the site list into the
EXE and points every site at `__hk_module_trap`; installing a module binds its
sites to where it landed, evicting it points them back at the trap. The
guest's Actor accessors also refuse a boss or family whose module is not
resident, and a room's modules are installed before its actors spawn and stay
pinned while it is resident, so no path reaches an unbound site.

Membership is by mangled section-name fragment, like the SDK script's I-cache
placement list. The module sections come before .text in the script because
LLD gives an input section to the first rule that matches it.
"""

# MODULE RULES (code that lives in a room module; harden() refuses a link that breaks one):
# 1. No two functions in different modules, or in a module and resident code, may be
#    identical: LLVM's merge-functions pass folds them into one body in one place (a
#    shared empty closure or a copied arena hook is enough). Call one resident function
#    instead. Building with -Zmerge-functions=disabled would prevent the fold for every
#    function at about 4 KB of resident code, so the guard is the check, not the flag.
# 2. No `match` that becomes a jump table inside a module: the hazard scanner cannot bound
#    its `jr`. Use a lookup table or range checks.
# 3. A module is only entered through `modules::require`/`loaded` checks in resident code.

import os
import re
import struct
from bisect import bisect_right
from pathlib import Path

LINK_BASE = 0x80408000
LINK_STRIDE = 0x10000
TRAMP_WORDS = 32
MAX_IMPORTS = 256
# The pool: a fixed region below the stack reserve. A module never straddles
# a 64 KiB `lui` window boundary inside it (the guest's allocator skips them).
# 92 KiB since the 2026-10-02 merge: at 96 KiB the merged resident image
# overflowed RAM by 2,944 bytes. The worst room plus the carried Shade art
# needs 80,972 bytes, under this less the 4 KiB window fence.
POOL_BYTES = 0x17000
STACK_FLOOR = 0x801FFF00 - 0xC000

def _sim(*mods):
    return tuple(f'*6hk_sim{len(m)}{m}*' for m in mods)


def _actor(*fns):
    return tuple(f'*Actor{len(f)}{f}*' for f in fns)


MODULES = {
    'false_knight': _sim('false_knight') + _actor('apply_false_knight', 'advance_false_knight', 'strike_false_knight'),
    'mawlek': _sim('mawlek') + _actor('apply_mawlek', 'advance_mawlek', 'strike_mawlek'),
    'husks': _sim('husk_guard', 'zombie_shield') + _actor('advance_husk_guard', 'advance_zombie_shield', 'shield_senses'),
    'vengefly': _sim('vengefly') + _actor('advance_vengefly', 'vengefly_senses'),
    'gruzzer': _sim('gruzzer') + _actor('advance_gruzzer'),
    # Gruz Mother: its controller, its arena handling and its corpse and
    # burster; the seven reserve flies it releases are the Gruzzer module's.
    'gruz_mother': _sim('gruz_mother') + _actor('apply_gruz', 'advance_gruz', 'advance_gruz_dead',
                                                'strike_gruz'),
    'acid_flyer': _sim('acid_flyer') + _actor('advance_acid_flyer'),
    'mosquito': _sim('mosquito') + _actor('advance_mosquito'),
    'moss_walker': _sim('moss_walker') + _actor('advance_moss_walker'),
    'baldur': _sim('baldur') + _actor('advance_baldur'),
    'aspid': _sim('aspid') + _actor('advance_aspid'),
    'hatcher': _sim('hatcher') + _actor('advance_hatcher', 'advance_baby'),
    'blocker': _sim('blocker') + _actor('advance_blocker'),
    'pigeon': _sim('pigeon') + _actor('advance_pigeon', 'pigeon_senses'),
    # climber_ray_hit stays resident: the SDK scan cannot resolve its
    # modulo-indexed switch table once the table is in a module.
    'climber': _sim('climber') + _actor('advance_climber'),
    # Room logic: the boss arena controller (Battle Control), only in arena rooms.
    'arena': _sim('boss'),
    # Room logic behind entry-point guards (modules::loaded): Sly's shelf
    # screen, the Snail Shaman's drawing, Cornifer's conversation.
    'shop': ('*6hk_psx4shop12presentation*',),
    'shaman': ('*6hk_psx6shaman4live4draw*',),
    'cornifer': ('*6hk_psx6mapper4live4tick*',),
}
# Room logic modules by the room that runs them: (data file, constant) holding
# the room's scene id.
ROOM_MODULES = {'shop': ('shop.rs', 'SHOP_SCENE'), 'shaman': ('shaman.rs', 'SHAMAN_SCENE'),
                'cornifer': ('cornifer.rs', 'CORNIFER_SCENE')}
# Families kept resident for a measurement (HK_RESIDENT_MODULES=climber,vengefly):
# their code links into .text as before and their rooms carry nothing for them.
ALWAYS_RESIDENT = {m for m in os.environ.get('HK_RESIDENT_MODULES', '').split(',') if m}
# Room art: the prop art (host/props.py) split by prop kind, carried by the
# rooms that place that kind. Data packages: no fixups, no import sites.
DATA_MODULES = ('art_grub', 'art_stalactite', 'art_goam')
# Carried data: packages no room's static table needs, which gameplay asks for
# at run time with `modules::carry` (the Shade's art follows the scene the
# Knight died in). Each ships in its own chunk, in the disc's last group, and
# is read into the pool by the gate into the scene that carries it.
# name -> the cooked file, read whole (repo-relative).
CARRIED = {'art_shade': 'data/shade.hk'}
# hk_sim::ActorController kinds -> the module whose code they run. Runner and
# Walker are the shared actor framework (the Runner is in 10 of the port's 28
# enemy rooms) and stay resident.
KIND_MODULE = {'FalseKnight': ('false_knight', 'arena'), 'Mawlek': ('mawlek', 'arena'), 'HuskGuard': 'husks', 'ZombieShield': 'husks',
               'Vengefly': 'vengefly', 'Gruzzer': 'gruzzer', 'Baldur': 'baldur', 'Aspid': 'aspid',
               'Hatcher': 'hatcher', 'HatcherBaby': 'hatcher', 'Pigeon': 'pigeon', 'Climber': 'climber',
               'GruzMother': ('gruz_mother', 'arena', 'gruzzer'), 'GruzzerReserve': 'gruzzer', 'AcidFlyer': 'acid_flyer', 'Mosquito': 'mosquito', 'MossWalker': 'moss_walker',
               # The Elder Baldur spits Rollers, which run the Baldur's FSM
               # (blocker_roller.rs), so its rooms carry both.
               'Blocker': ('blocker', 'baldur')}
# Resident functions allowed to hold import sites (mangled fragments): the
# per-actor dispatch in enemies.rs, and the Elder Baldur's Rollers
# (blocker_roller.rs, inlined into frame::simulate), which exist only while a
# Blocker's room is resident.
CALLERS = ('7enemies', '14blocker_roller', '5frame8simulate', '5frame6render', '8dialogue', '5frame')
J26, HI16, LO16, W32 = 1, 2, 3, 4


def link_address(k):
    return LINK_BASE + k * LINK_STRIDE


def window(address):
    """Centre of the 64 KiB `lui` window holding `address`."""
    return (address + 0x8000) & ~0xffff


def pool_base():
    return STACK_FLOOR - POOL_BYTES


def room_scenes():
    """{scene id: room logic modules} from the cooked room constants."""
    out = {}
    root = Path(__file__).resolve().parents[1] / 'data'
    for name, (file, const) in ROOM_MODULES.items():
        m = re.search(rf'pub const {const}:usize=(\d+);', (root / file).read_text())
        out.setdefault(int(m[1]), set()).add(name)
    return out


def scene_modules(regions_rs):
    """{scene id: set of module names} from the cooked actor tables and the
    rooms' own logic."""
    out = {}
    for sid, mods in room_scenes().items():
        out.setdefault(sid, set()).update(mods)
    for m in re.finditer(r'SCENE_ACTORS_(\d+)\s*:\s*&\[hk_sim::ActorSpec\]\s*=\s*&\[', regions_rs):
        body = regions_rs[m.end():regions_rs.find('];', m.end())]
        kinds = set(re.findall(r'ActorController::(\w+)', body))
        mods = set()
        for k in kinds & KIND_MODULE.keys():
            need = KIND_MODULE[k]
            mods |= {need} if isinstance(need, str) else set(need)
        mods -= ALWAYS_RESIDENT
        if mods:
            out.setdefault(int(m[1]), set()).update(mods)
    return out


def all_names():
    return list(MODULES) + list(DATA_MODULES) + list(CARRIED)


def carried_blobs(root):
    """The carried data packages' bytes (harden's data_blobs)."""
    return {name: (Path(root) / path).read_bytes() for name, path in CARRIED.items()}


def scene_table(packed, regions_rs, props_rs=None):
    """Manifest index -> sorted module ids its rooms need (code and art)."""
    index = {r['scene_id']: i for i, r in enumerate(packed['scenes'])}
    names = all_names()
    need = scene_modules(regions_rs)
    if props_rs is not None:
        for sid, arts in props_scenes(props_rs).items():
            need.setdefault(sid, set()).update(arts)
    return {index[sid]: sorted(names.index(m) for m in mods) for sid, mods in need.items() if sid in index}


def _props_tables(props_rs):
    t = props_rs
    parts = [tuple(map(int, m)) for m in re.findall(r'Part\{offset:(\d+),width:(\d+),height:(\d+)', t)]
    frames = [tuple(map(int, m)) for m in re.findall(r'\((\d+),(\d+)\)', t[t.index('pub const FRAMES'):t.index('pub const CLIP_FRAMES')])]
    clip_frames = list(map(int, re.findall(r'\d+', t[t.index('pub const CLIP_FRAMES'):t.index('pub const CLIPS')].split('=', 1)[1])))
    clips = [tuple(map(int, m)) for m in re.findall(r'Clip\{first:(\d+),count:(\d+)', t)]
    goam = list(map(int, re.search(r'GOAM_CLIPS:\[u16;4\]=\[([^\]]*)', t)[1].split(',')))
    grub = list(map(int, re.search(r'GRUB_CLIPS:\[u16;3\]=\[([^\]]*)', t)[1].split(',')))
    body = lambda k: t[t.index(f'pub const {k}'):][:t[t.index(f'pub const {k}'):].index('];')]
    stal = {int(x) for x in re.findall(r'(?:frame|embedded):(\d+)', body('STALACTITES'))}
    palettes = int(re.search(r'PALETTE_COUNT:usize=(\d+)', t)[1])
    return parts, frames, clip_frames, clips, goam, grub, stal, palettes, body


def props_scenes(props_rs):
    """Scene id -> art packages its props need."""
    *_, body = _props_tables(props_rs)
    out = {}
    for art, table in (('art_goam', 'GOAMS'), ('art_stalactite', 'STALACTITES'), ('art_grub', 'GRUBS')):
        for sid in re.findall(r'scene:(\d+)', body(table)):
            out.setdefault(int(sid), set()).add(art)
    return out


def props_art(props_rs, props_hk):
    """Split the prop art by kind: ({package: bytes}, Rust for data/props_art.rs).
    Each kind's parts are one contiguous run of data/props.hk."""
    parts, frames, clip_frames, clips, goam, grub, stal, palettes, _ = _props_tables(props_rs)
    of_clips = lambda cs: {clip_frames[i] for c in cs for i in range(clips[c][0], clips[c][0] + clips[c][1])}
    of_frames = lambda fs: {p for f in fs for p in range(frames[f][0], frames[f][0] + frames[f][1])}
    kind = {}
    for name, ps in (('art_grub', of_frames(of_clips(grub))), ('art_stalactite', of_frames(stal)),
                     ('art_goam', of_frames(of_clips(goam)))):
        for p in ps:
            kind[p] = name
    for p in range(len(parts)):  # a part no frame names rides with its predecessor
        kind.setdefault(p, kind.get(p - 1, DATA_MODULES[0]))
    size = lambda p: (parts[p][1] + 3) // 4 * 2 * parts[p][2]
    base = palettes * 32
    blobs, table = {}, []
    for name in DATA_MODULES:
        mine = [p for p in range(len(parts)) if kind[p] == name]
        if not mine:
            blobs[name] = b''
            continue
        lo, hi = parts[mine[0]][0], parts[mine[-1]][0] + size(mine[-1])
        if any(not lo <= parts[p][0] < hi for p in mine) or sorted(mine) != list(range(mine[0], mine[-1] + 1)):
            raise ValueError(f'{name} parts are not one contiguous run')
        blobs[name] = props_hk[base + lo:base + hi]
    for p in range(len(parts)):
        mine = [q for q in range(len(parts)) if kind[q] == kind[p]]
        table.append((DATA_MODULES.index(kind[p]), parts[p][0] - parts[mine[0]][0]))
    rust = ('// Generated by host/code_modules.py from data/props.rs and data/props.hk; do not edit.\n'
            f'pub static PALETTES:[u8;{base}]=[{",".join(map(str, props_hk[:base]))}];\n'
            '/// Per part: its art package (0 grub, 1 stalactite, 2 goam) and its offset in it.\n'
            f'pub const PART_ART:[(u8,u32);{len(table)}]=[{",".join(f"({k},{o})" for k, o in table)}];\n')
    return blobs, rust


def package_chunks(table):
    """The rooms' package chunks in id order: every code chunk (heading its
    room's group), then every art chunk (ending it, after the scene data, so
    it never displaces data; read into the pool on its own, never by a gate).
    Each is (kind, manifest scene, sorted module ids)."""
    code = [('code', s, [k for k in m if k < len(MODULES)]) for s, m in sorted(table.items())]
    art = [('art', s, [k for k in m if k >= len(MODULES)]) for s, m in sorted(table.items())]
    carried = [('data', None, [all_names().index(n)]) for n in CARRIED]
    return [c for c in code if c[2]] + [c for c in art if c[2]] + carried


def art_reserve(raw, stored):
    """Pool bytes a compressed art chunk reserves (modules.rs `reserve_bytes`):
    the raw packages plus LZ4's in-place margin, and at least the sectors the
    stored bytes are read into, which end exactly at the reservation's end."""
    sectors = -(-stored // 2048) * 2048
    return max(-(-(raw + (stored >> 8) + 64) // 2048) * 2048, sectors)


def inplace_fits(z, raw, arena):
    """Replay the guest decoder's in-place guards (room_decode.rs) on the
    HLZC bytes `z` ending at `arena`, output from 0: True if no copy would
    overtake input that is still to be read."""
    src, out, end = arena - len(z) + 8, 0, arena
    while src < end:
        token = z[src - (arena - len(z))]; src += 1
        n = token >> 4
        if n == 15:
            while True:
                b = z[src - (arena - len(z))]; src += 1; n += b
                if b != 255: break
        if out > src or out + n > raw: return False
        out += n; src += n
        if src >= end: break
        src += 2
        m = (token & 15) + 4
        if m == 19:
            while True:
                b = z[src - (arena - len(z))]; src += 1; m += b
                if b != 255: break
        if out + m > raw or out + m > src: return False
        out += m
    return out == raw


def art_stored(blob):
    """The HLZC form an art chunk ships in when it saves a sector and decodes
    in place in its reservation; None to ship it raw."""
    import lz4.block
    z = b'HLZC' + struct.pack('<I', len(blob)) + lz4.block.compress(blob, mode='high_compression', store_size=False)
    if -(-len(z) // 2048) >= -(-len(blob) // 2048):
        return None
    reserve = art_reserve(len(blob), len(z))
    start = reserve - -(-len(z) // 2048) * 2048
    return z if inplace_fits(z, len(blob), start + len(z)) else None


def manifest_rust(packed, regions_rs, props_rs=None):
    """data/modules.rs: module ids, the per-scene module sets and package
    chunks, and the pool constants."""
    table = scene_table(packed, regions_rs, props_rs)
    n = len(packed['scenes'])
    chunks = package_chunks(table)
    codes = [c[1] for c in chunks if c[0] == 'code']
    arts = [c[1] for c in chunks if c[0] == 'art']
    carried = {c[2][0]: k for k, c in enumerate(chunks) if c[0] == 'data'}
    masks = [sum(1 << k for k in table.get(i, ())) for i in range(n)]
    code = [str(codes.index(i)) if i in codes else 'u8::MAX' for i in range(n)]
    art = [str(len(codes) + arts.index(i)) if i in arts else 'u8::MAX' for i in range(n)]
    lines = ['// Generated by host/code_modules.py; do not edit.',
             f'pub const MODULE_COUNT:usize={len(all_names())};',
             f'pub const CODE_MODULES:usize={len(MODULES)};']
    lines += [f'pub const {name.upper()}:usize={k};' for k, name in enumerate(all_names())]
    lines += [f'pub const LINK_BASE:u32={LINK_BASE:#x};', f'pub const LINK_STRIDE:u32={LINK_STRIDE:#x};',
              f'pub const POOL_BYTES:usize={POOL_BYTES:#x};',
              f'pub const MAX_IMPORTS:usize={MAX_IMPORTS};',
              '/// Modules each manifest scene needs, one bit per module.',
              f'pub const SCENE_MODULES:[u32;{n}]=[{",".join(map(str, masks))}];',
              f'pub const CODE_MASK:u32={(1 << len(MODULES)) - 1};',
              '/// Modules linked resident for this build (HK_RESIDENT_MODULES).',
              f'pub const ALWAYS:u32={sum(1 << k for k, n in enumerate(MODULES) if n in ALWAYS_RESIDENT)};',
              '/// The code chunk heading each manifest scene\'s disc group, or u8::MAX.',
              f'pub const SCENE_CODE:[u8;{n}]=[{",".join(code)}];',
              '/// The art chunk ending each manifest scene\'s disc group (a package chunk id), or u8::MAX.',
              f'pub const SCENE_ART:[u8;{n}]=[{",".join(art)}];',
              f'pub const CODE_CHUNKS:usize={len(codes)};',
              f'pub const ART_CHUNKS:usize={len(arts)};',
              f'pub const DATA_CHUNKS:usize={len(carried)};',
              f'pub const PACKAGE_CHUNKS:usize={len(chunks)};',
              f'pub const CHUNK_SCENE:[usize;{len(chunks)}]=[{",".join("usize::MAX" if c[1] is None else str(c[1]) for c in chunks)}];',
              '/// The chunk of each carried data module (modules::carry), or u8::MAX.',
              f'pub const MODULE_CHUNK:[u8;{len(all_names())}]=[{",".join(str(carried.get(k, "u8::MAX")) for k in range(len(all_names())))}];', '']
    return '\n'.join(lines)


def layer(script):
    """The SDK-derived linker script with the pool carved out and the modules
    linked in their link-only space."""

    def sub(pattern, replacement, text):
        out, count = re.subn(pattern, replacement, text, flags=re.M)
        if count != 1:
            raise ValueError('linker script changed; review the module layer: ' + pattern)
        return out
    script = sub(r'^(STACK_RESERVE = [^\n]*)$',
                 r'\1\nPOOL_BYTES = %#x; /* hk-psx: streaming code pool, host/code_modules.py */\n'
                 r'POOL_BASE = STACK_INIT - STACK_RESERVE - POOL_BYTES;' % POOL_BYTES, script)
    script = sub(r'LENGTH = EXE_HEAD_BYTES \+ STACK_INIT - LOAD_ADDR - STACK_RESERVE$',
                 'LENGTH = EXE_HEAD_BYTES + STACK_INIT - LOAD_ADDR - STACK_RESERVE - POOL_BYTES\n'
                 f'    MODLINK (rwx) : ORIGIN = {LINK_BASE:#x}, LENGTH = {LINK_STRIDE * len(MODULES):#x}', script)
    # The BIOS loads text and data only: the SDK header's size is `__image_end`, the
    # data end rounded up to a whole sector, and `split` zero-pads the file to it.
    script = sub(r'__heap_end   = STACK_INIT - STACK_RESERVE;', '__heap_end   = POOL_BASE;', script)
    block = ['    /* hk-psx streaming code modules (host/code_modules.py), linked in a',
             '       link-only space and relocated into the pool at run time. Listed',
             '       before .text so these rules see their sections first; code first',
             '       in each, so the hazard passes treat a prefix as instructions. */']
    for k, (name, patterns) in enumerate(MODULES.items()):
        if name in ALWAYS_RESIDENT:
            continue
        rules = [f'        *(.text.{p})' for p in patterns] + [f'        *(.rodata.{p})' for p in patterns]
        block += [f'    .mod_{name} {link_address(k):#x} : {{', *rules, '    } > MODLINK',
                  f'    ASSERT(SIZEOF(.mod_{name}) + {4 * (TRAMP_WORDS + 2)} <= {LINK_STRIDE:#x}, "module {name} outgrew 64 KiB")',
                  f'    ASSERT(SIZEOF(.mod_{name}) + {4 * (TRAMP_WORDS + 2)} <= POOL_BYTES, "module {name} outgrew the pool")']
    return sub(r'^(    \.text : \{)$', '\n'.join(block) + r'\n\n\1', script)


class Elf:
    """Just enough ELF32 little-endian reading for the linked guest."""
    def __init__(self, path):
        self.data = data = open(path, 'rb').read()
        if data[:4] != b'\x7fELF' or data[4] != 1 or data[5] != 1:
            raise ValueError('not a 32-bit little-endian ELF: ' + str(path))
        shoff, = struct.unpack_from('<I', data, 0x20)
        shentsize, shnum, shstrndx = struct.unpack_from('<HHH', data, 0x2e)
        raw = [struct.unpack_from('<10I', data, shoff + i * shentsize) for i in range(shnum)]
        names = raw[shstrndx]

        def string(offset, at):
            start = offset + at
            return data[start:data.index(b'\0', start)].decode()
        self.sections = [dict(name=string(names[4], s[0]), type=s[1], flags=s[2], addr=s[3], offset=s[4],
                              size=s[5], link=s[6], info=s[7], entsize=s[9]) for s in raw]
        self.by_name = {s['name']: s for s in self.sections}
        symtab = next(s for s in self.sections if s['type'] == 2)
        strtab = self.sections[symtab['link']]
        self.symbols = []
        for i in range(symtab['size'] // 16):
            name, value, size, info, _, shndx = struct.unpack_from('<IIIBBH', data, symtab['offset'] + i * 16)
            self.symbols.append((string(strtab['offset'], name) if name else '', value, size, info & 15, shndx))
        functions = sorted((v, s, n) for n, v, s, t, x in self.symbols if t == 2 and s)
        self._starts = [f[0] for f in functions]
        self._functions = functions

    def index(self, name):
        return self.sections.index(self.by_name[name])

    def code_end(self, name):
        """End of the last function in section `name`."""
        index = self.index(name)
        return max(v + size for n, v, size, t, x in self.symbols if t == 2 and x == index and size)

    def bytes_of(self, name):
        s = self.by_name[name]
        return self.data[s['offset']:s['offset'] + s['size']]

    def symbol(self, name):
        return next(v for n, v, *_ in self.symbols if n == name)

    def function_at(self, address):
        i = bisect_right(self._starts, address) - 1
        if i >= 0:
            start, size, name = self._functions[i]
            if start <= address < start + size:
                return name
        return None

    def relocations(self, target_section):
        """(offset, type, symbol's section index) of every relocation applied to `target_section`."""
        index = self.index(target_section)
        symtab = next(s for s in self.sections if s['type'] == 2)
        for s in self.sections:
            if s['type'] == 9 and s['info'] == index:  # SHT_REL
                for i in range(s['size'] // 8):
                    offset, info = struct.unpack_from('<II', self.data, s['offset'] + i * 8)
                    shndx = struct.unpack_from('<H', self.data, symtab['offset'] + (info >> 8) * 16 + 14)[0]
                    yield offset, info & 0xff, shndx


LOAD_ADDR = 0x80010000
HEADER = 2048
MAGIC = 0x48415A54          # the SDK patcher's trampoline array
PACKAGE_MAGIC = 0x444D4B48  # 'HKMD'


def fnv1a(data):
    h = 0x811c9dc5
    for b in data:
        h = ((h ^ b) * 0x01000193) & 0xffffffff
    return h


def word_hash(data):
    """FNV-1a over little-endian words: what the guest checks a chunk with."""
    h = 0x811c9dc5
    for (w,) in struct.iter_unpack('<I', data):
        h = ((h ^ w) * 0x01000193) & 0xffffffff
    return h


def split(elf, exe_path):
    """Write the PS-X EXE: header, .text and .data, what the BIOS loads."""
    header, text, data = (elf.by_name[n] for n in ('.ps_exe_head', '.text', '.data'))
    if header['addr'] + header['size'] != text['addr'] or text['addr'] + text['size'] > data['addr']:
        raise ValueError('unexpected section layout')
    exe = bytearray(elf.bytes_of('.ps_exe_head'))
    exe += elf.bytes_of('.text') + bytes(data['addr'] - text['addr'] - text['size']) + elf.bytes_of('.data')
    claimed = struct.unpack_from('<I', exe, 0x1c)[0]
    short = claimed - (len(exe) - HEADER)
    if not 0 <= short < 0x800:
        raise ValueError('EXE header payload size disagrees with .text + .data')
    exe += bytes(short)
    Path(exe_path).write_bytes(exe)


def map_with_only(map_text, keep=None):
    """The link map without the other modules' blocks (all of them when `keep`
    is None): a composite image holds one module, and the SDK tools check the
    map against the image."""
    out, skipping = [], False
    for line in map_text.splitlines(keepends=True):
        if len(line) > 33 and line[33:34] not in (' ', '') and line[:8].strip():
            name = line[33:].strip()
            skipping = name.startswith('.mod_') and name != f'.mod_{keep}'
        if not skipping:
            out.append(line)
    return ''.join(out)


def _j_target(word, pc):
    return (pc & 0xf0000000) | ((word & 0x3ffffff) << 2)


def _module_of(address):
    k, off = divmod(address - LINK_BASE, LINK_STRIDE)
    return (k, off) if 0 <= k < len(MODULES) else (None, None)


def harden(elf_path, exe, link_map, work, report_dir, scene_table, data_blobs=None):
    """From the linked ELF to the final EXE and one code chunk per scene that
    needs code: split, patch load-delay hazards (resident code, then each
    module in a composite image holding it at its link address with its own
    trampoline area), derive each module's fixups and the resident import
    sites, pre-bind every site to the trap, write the chunk tables into the
    EXE, then scan every image. Returns the report."""
    import hazards
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    exe = Path(exe)
    elf = Elf(elf_path)
    split(elf, exe)
    text_map = Path(link_map).read_text()
    names = list(MODULES)
    # A module the compiler inlined away entirely has no section: it ships as
    # an empty package, so its rooms still find it "installed".
    empty = {n for n in names if n not in ALWAYS_RESIDENT and ('.mod_' + n) not in elf.by_name}
    mod_index = {elf.index('.mod_' + n): k for k, n in enumerate(names) if n not in ALWAYS_RESIDENT | empty}
    # The EXE holds no module, so the patcher reads the map without their blocks.
    resident_map = work / 'map-resident.map'
    resident_map.write_text(map_with_only(text_map))
    resident_patch = hazards.patch(exe, resident_map)
    report = {'resident_patch': {k: v for k, v in resident_patch.items() if k != 'log'}, 'modules': {}}

    # Resident references into module space, from the relocations: only
    # jumps and data words may cross, and only from the allowed callers.
    resident_relocs = {}
    for section in ('.text', '.data'):
        for offset, kind, shndx in elf.relocations(section):
            if shndx in mod_index:
                if kind not in (2, 4, 5, 6):
                    raise ValueError(f'resident relocation type {kind} into a module at {offset:#x}')
                where = elf.function_at(offset) or f'{section}+{offset:#x}'
                if section == '.text' and not any(c in where for c in CALLERS):
                    raise ValueError(f'{where} calls into module {names[mod_index[shndx]]}')
                resident_relocs[offset] = (kind, mod_index[shndx], where)

    packages = {}
    for k, name in enumerate(names):
        if name in empty:
            packages[k] = struct.pack('<5I', PACKAGE_MAGIC, k, 0, 0, 0)
            report['modules'][name] = {'link_address': link_address(k), 'image_bytes': 0, 'code_bytes': 0,
                                       'fixups': 0, 'hazard_sites': 0, 'trampoline_words': 0, 'empty': True}
            continue
        if name in ALWAYS_RESIDENT:
            continue
        sec = elf.by_name['.mod_' + name]
        base = link_address(k)
        assert sec['addr'] == base
        image = bytearray(elf.bytes_of('.mod_' + name))
        code_end = elf.code_end('.mod_' + name) - base
        image += bytes(-len(image) % 4)
        tramp_at = len(image)
        image += struct.pack('<II', MAGIC, TRAMP_WORDS) + bytes(4 * TRAMP_WORDS)
        relocs = list(elf.relocations('.mod_' + name))
        before = {off: struct.unpack_from('<I', image, off - base)[0] for off, kind, _ in relocs if kind in (2, 5, 6)}
        # Composite: the patched EXE with its own trampoline array hidden, so
        # the SDK patcher uses the module's, then the module at its address.
        payload = bytearray(exe.read_bytes())
        first = payload.find(struct.pack('<I', MAGIC), HEADER)
        while first >= 0 and first % 4:
            first = payload.find(struct.pack('<I', MAGIC), first + 1)
        struct.pack_into('<I', payload, first, 0)
        at = base - LOAD_ADDR + HEADER
        composite = work / f'composite-{name}.exe'
        composite.write_bytes(bytes(payload) + bytes(at - len(payload)) + bytes(image))
        mod_map = work / f'map-{name}.map'
        mod_map.write_text(map_with_only(text_map, name))
        code = [(base, base + code_end), (base + tramp_at, base + len(image))]
        patched = hazards.patch(composite, mod_map, extra=code, array_at=base + tramp_at)
        image = bytearray(composite.read_bytes()[at:at + len(image)])
        for off, word in before.items():
            if struct.unpack_from('<I', image, off - base)[0] != word:
                raise ValueError(f'the hazard patch moved a relocated word in {name} at {off:#x}: '
                                 'usually a `match` compiled to a jump table the scanner cannot bound; '
                                 'write it as a table or range checks (see MODULE RULES in this file)')
        # Fixups.
        fixups = []
        for off in list(range(0, code_end, 4)) + list(range(tramp_at + 8, len(image), 4)):
            word = struct.unpack_from('<I', image, off)[0]
            if word >> 26 in (2, 3):
                tk, toff = _module_of(_j_target(word, base + off))
                if tk is None:
                    continue
                if tk != k:
                    raise ValueError(f'module {name} jumps into module {names[tk]}: LLVM folded identical '
                                     'functions (or closures) across modules; make them one resident function '
                                     'both call, or differ (see MODULE RULES in this file)')
                fixups.append((J26, off))
        for off, kind, shndx in relocs:
            if shndx in mod_index and mod_index[shndx] != k:
                raise ValueError(f'module {name} refers to module {names[mod_index[shndx]]}')
            if shndx != elf.index('.mod_' + name):
                continue
            word = struct.unpack_from('<I', image, off - base)[0]
            if kind == 5:
                if word & 0xffff != window(base) >> 16:
                    raise ValueError(f'{name}: HI16 at {off:#x} is not in the module window')
                fixups.append((HI16, off - base))
            elif kind == 6:
                fixups.append((LO16, off - base))
            elif kind == 2:
                if _module_of(word)[0] != k:
                    raise ValueError(f'{name}: word at {off:#x} does not point into the module')
                fixups.append((W32, off - base))
            elif kind not in (4, 10):  # R_MIPS_26 is rescanned above; PC16 needs nothing
                raise ValueError(f'{name}: relocation type {kind} at {off:#x}')
        fixups.sort(key=lambda f: f[1])
        header = struct.pack('<5I', PACKAGE_MAGIC, k, len(image) // 4, len(fixups), code_end)
        packages[k] = header + bytes(image) + b''.join(struct.pack('<I', t << 28 | o) for t, o in fixups)
        report['modules'][name] = {'link_address': base, 'image_bytes': len(image), 'code_bytes': code_end,
                                   'fixups': len(fixups), 'hazard_sites': patched['sites'],
                                   'trampoline_words': patched['trampoline_words']}

    # Data packages (room art): the bytes as they are, no fixups.
    for name, blob in (data_blobs or {}).items():
        k = all_names().index(name)
        blob = bytes(blob) + bytes(-len(blob) % 4)
        packages[k] = struct.pack('<5I', PACKAGE_MAGIC, k, len(blob) // 4, 0, 0) + blob
        report['modules'][name] = {'image_bytes': len(blob), 'code_bytes': 0, 'fixups': 0, 'hazard_sites': 0,
                                   'trampoline_words': 0}

    # Import sites in the final resident image: every jump (code or resident
    # trampoline) into module space, and every relocated data word into it.
    image = bytearray(exe.read_bytes())
    tramps = elf.symbol('HAZARD_TRAMPOLINES')
    text = elf.by_name['.text']
    sites = []
    scan = list(range(text['addr'], text['addr'] + text['size'], 4)) + \
        list(range(tramps + 8, tramps + 8 + 4 * struct.unpack_from('<I', image, tramps + 4 - LOAD_ADDR + HEADER)[0], 4))
    for addr in scan:
        word = struct.unpack_from('<I', image, addr - LOAD_ADDR + HEADER)[0]
        if word >> 26 in (2, 3):
            tk, toff = _module_of(_j_target(word, addr))
            if tk is not None:
                if addr < text['addr'] + text['size'] and addr not in resident_relocs:
                    raise ValueError(f'unrelocated jump into module space at {addr:#x}')
                sites.append((tk, addr, J26, toff))
    linked = {s['name']: s for s in elf.sections}
    for addr, (kind, tk, where) in resident_relocs.items():
        word = struct.unpack_from('<I', image, addr - LOAD_ADDR + HEADER)[0]
        if kind in (5, 6):
            sec = elf.by_name['.text']
            if struct.unpack_from('<I', elf.data, sec['offset'] + addr - sec['addr'])[0] != word:
                raise ValueError(f'the hazard patch moved a resident HI16/LO16 at {addr:#x}')
        if kind == 2:
            sites.append((tk, addr, W32, word - link_address(tk)))
        elif kind == 5:
            if word & 0xffff != window(link_address(tk)) >> 16:
                raise ValueError(f'resident HI16 at {addr:#x} is not in the module window')
            sites.append((tk, addr, HI16, 0))
        elif kind == 6:
            # Module k is linked at 0x8000 past a window, so the target is
            # the sign-extended low half plus 0x8000.
            sites.append((tk, addr, LO16, ((word & 0xffff) ^ 0x8000) - 0x8000 + 0x8000))
    sites.sort()
    if len(sites) > MAX_IMPORTS:
        raise ValueError(f'{len(sites)} import sites, more than MAX_IMPORTS {MAX_IMPORTS}')
    trap = elf.symbol('__hk_module_trap')
    table = elf.symbol('HK_MODULE_IMPORTS')
    ranges = elf.symbol('HK_MODULE_IMPORT_RANGE')
    data = elf.by_name['.data']
    for sym in (table, ranges, elf.symbol('HK_CODE_LEN'), elf.symbol('HK_CODE_FNV'), elf.symbol('HK_CODE_HASH'),
                elf.symbol('HK_CODE_STORED_LEN'), elf.symbol('HK_CODE_STORED_FNV')):
        if not data['addr'] <= sym < data['addr'] + data['size']:
            raise ValueError('a post-link table is not in .data')
    put = lambda addr, value: struct.pack_into('<I', image, addr - LOAD_ADDR + HEADER, value)
    get = lambda addr: struct.unpack_from('<I', image, addr - LOAD_ADDR + HEADER)[0]
    for i, (tk, addr, kind, toff) in enumerate(sites):
        put(table + 8 * i, addr)
        put(table + 8 * i + 4, kind << 28 | tk << 20 | toff)
        # Ship every site pointing at the trap.
        hi = (trap + 0x8000) >> 16
        put(addr, {J26: (get(addr) & 0xfc000000) | ((trap >> 2) & 0x3ffffff), W32: trap,
                   HI16: (get(addr) & 0xffff0000) | hi, LO16: (get(addr) & 0xffff0000) | ((trap - (hi << 16)) & 0xffff)}[kind])
    for k in range(len(all_names())):
        mine = [i for i, s in enumerate(sites) if s[0] == k]
        put(ranges + 4 * k, (min(mine) if mine else 0) | (len(mine) << 16))
        report['modules'].setdefault(all_names()[k], {'resident': True})['import_sites'] = [hex(s[1]) for s in sites if s[0] == k]
    # The rooms' package chunks: code chunks, then art chunks.
    chunk_dir = work / 'chunks'
    chunk_dir.mkdir(exist_ok=True)
    chunks = []
    for c, (kind, scene, mods) in enumerate(package_chunks(scene_table)):
        blob = b''.join(packages[k] for k in mods)
        path = chunk_dir / (f'{kind}_{scene}.hkmd' if scene is not None else f'{kind}_{all_names()[mods[0]]}.hkmd')
        stored = art_stored(blob) if kind in ('art', 'data') else None
        path.write_bytes(stored or blob)
        put(elf.symbol('HK_CODE_LEN') + 4 * c, len(blob))
        put(elf.symbol('HK_CODE_FNV') + 4 * c, fnv1a(blob))
        put(elf.symbol('HK_CODE_HASH') + 4 * c, word_hash(blob))
        put(elf.symbol('HK_CODE_STORED_LEN') + 4 * c, len(stored) if stored else 0)
        put(elf.symbol('HK_CODE_STORED_FNV') + 4 * c, fnv1a(stored) if stored else 0)
        chunks.append({'kind': kind, 'scene_index': scene, 'modules': [all_names()[k] for k in mods],
                       'bytes': len(blob), 'stored_bytes': len(stored or blob), 'path': str(path)})
    exe.write_bytes(bytes(image))
    # Every room's own code and art must fit the pool (less one window fence).
    for scene, mods in scene_table.items():
        need = sum(report['modules'][all_names()[k]]['image_bytes'] for k in mods)
        if need > POOL_BYTES - 0x1000:
            raise ValueError(f"scene {scene} needs {need} pool bytes, more than the pool allows")
    # A carried package rides on top of any room's own (the Shade's scene can be any).
    carried = [report['modules'][n]['image_bytes'] for n in CARRIED if n in report['modules']]
    if carried:
        worst = max(sum(report['modules'][all_names()[k]]['image_bytes'] for k in mods) for mods in scene_table.values())
        if worst + max(carried) > POOL_BYTES - 0x1000:
            raise ValueError(f'a room ({worst} pool bytes) plus a carried package ({max(carried)}) exceeds the pool')
    report['chunks'] = chunks
    report['pool'] = {'base': pool_base(), 'bytes': POOL_BYTES}
    # Scans: each module in its composite (rebuilt on the final EXE), then
    # the resident image with a map that lists no module.
    for k, name in enumerate(names):
        if name in ALWAYS_RESIDENT or name in empty:
            continue
        base = link_address(k)
        img = packages[k][20:20 + 4 * struct.unpack_from('<I', packages[k], 8)[0]]
        payload = bytearray(image)
        at = base - LOAD_ADDR + HEADER
        composite = work / f'composite-{name}.exe'
        composite.write_bytes(bytes(payload) + bytes(at - len(payload)) + img)
        code_end = report['modules'][name]['code_bytes']
        tramp_at = len(img) - 4 * (TRAMP_WORDS + 2)
        hazards.scan(composite, work / f'map-{name}.map', report_path=Path(report_dir) / f'hazards-{name}.json',
                     extra=[(base, base + code_end), (base + tramp_at + 8, base + len(img))])
        hazards.stack_guard(composite, work / f'map-{name}.map')
    report['resident_map'] = str(resident_map)
    report['import_sites'] = len(sites)
    return report
