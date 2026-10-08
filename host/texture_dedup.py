"""Lossless within-region canonical texture packing, with stable source mappings.

Fixed HKROOM02 sections and source texels are preserved; optional covers are regenerated. Only exact dimensions, all CLUT words and indexed pixels
can share storage, and static and streamed textures remain separate classes.
"""
import argparse
import hashlib
import json
import struct
from pathlib import Path

from region_delta import layout, textures

ROOT = Path(__file__).resolve().parents[1]


def clut_count(raw):
    """VRAM CLUT slots a cooked region really needs: its distinct palette words.

    HKROOM02 stores one 32-byte entry per texture, but nothing requires those
    entries to differ and nothing uploads the block as it stands: the scene bank
    pools palettes by value and a scene uploads that pool. So a region's slot
    cost is the number of distinct words among the entries its textures name,
    not the size of the table. The tiles of one tiled frame always name the same
    words, which is where the difference is largest.
    """
    counts, prefix, _, _ = layout(raw)
    textures = counts[1]
    palettes = [raw[prefix+i*32:prefix+(i+1)*32] for i in range(textures)]
    return len({palettes[struct.unpack_from('<H', raw, 40+i*16+10)[0]] for i in range(textures)})


_CODE_KEY=None
def _code_key():
    """Hash of the code that decides a dedup result, so cached packs never outlive it."""
    global _CODE_KEY
    if _CODE_KEY is None:
        h=hashlib.sha256()
        for name in ('host/texture_dedup.py','host/cook.py','host/alpha_covers.py','host/region_delta.py','tools/audit_resident_bank.py'):
            h.update((ROOT/name).read_bytes())
        _CODE_KEY=h.hexdigest()[:16]
    return _CODE_KEY

def deduplicate_room(raw, replacements=None):
    """Cached: the result is a pure function of the pack, the replacements and the code."""
    key=hashlib.sha256(_code_key().encode()+raw+json.dumps(sorted((int(k),v.hex()) for k,v in (replacements or {}).items())).encode()).hexdigest()
    folder=ROOT/'.hkpsx'/'dedup-cache';packed=folder/f'{key}.hk';record=folder/f'{key}.json'
    if packed.is_file() and record.is_file():
        try:return packed.read_bytes(),json.loads(record.read_text())
        except (OSError,ValueError):pass
    result,report=_deduplicate_room(raw,replacements)
    folder.mkdir(parents=True,exist_ok=True)
    import os
    temp=packed.with_suffix(f'.{os.getpid()}.tmp');temp.write_bytes(result);temp.replace(packed)
    temp=record.with_suffix(f'.{os.getpid()}.tmp');temp.write_text(json.dumps(report));temp.replace(record)
    return result,report

def _deduplicate_room(raw, replacements=None):
    from cook import Atlas
    counts, prefix, _, _ = layout(raw)
    pages, texture_count, draws, frames, clips, edges = counts
    flags=struct.unpack_from('<I',raw,36)[0]
    if flags&~1:raise ValueError('Unknown room features')
    atlas = Atlas(alpha_covers=bool(flags&1))
    blobs = textures(raw)
    replacements = replacements or {}
    if any(not isinstance(i,int) or not 0 <= i < texture_count for i in replacements):
        raise ValueError('Invalid replacement texture index')
    expected = [replacements.get(i,blob) for i,blob in enumerate(blobs)]
    for i in replacements:
        if struct.unpack_from('<H',raw,40+i*16)[0] == 65535 and expected[i] != blobs[i]:
            raise ValueError('Static texture trial cannot replace animation')
    # A frame split across several animation slots is a run of consecutive
    # streamed texture IDs whose first record carries the grid. Canonical
    # sharing would renumber or merge those tiles, so the run is carried
    # through whole, grid and all.
    tile_owner = {}
    for i in range(texture_count):
        page, cols, rows = struct.unpack_from('<3H', raw, 40+i*16)
        if page != 65535 or not cols:
            continue
        if i in tile_owner or i+cols*rows > texture_count:
            raise ValueError('Overlapping or truncated animation tile grid')
        for tile in range(cols*rows):
            tile_owner[i+tile] = i
    mapping = []
    for i, blob in enumerate(expected):
        width, height = struct.unpack_from('<HH', blob)
        streamed = struct.unpack_from('<H', raw, 40+i*16)[0] == 65535
        index = atlas.add_quantized(width,height,blob[4:36],blob[36:],streamed,
                                    unique=i in tile_owner)
        if i in tile_owner:
            owner = mapping[tile_owner[i]] if tile_owner[i] != i else index
            if tile_owner[i] == i:
                atlas.grids[index] = struct.unpack_from('<2H', raw, 42+i*16)
            atlas.tile_owner[index] = owner
        mapping.append(index)
    atlas.pack()
    at = 40+texture_count*16
    draw_data = bytearray(raw[at:at+draws*44]);at += draws*44
    frame_data = bytearray(raw[at:at+frames*20]);at += frames*20
    for i in range(draws):
        old = struct.unpack_from('<H',draw_data,i*44)[0]
        struct.pack_into('<H',draw_data,i*44,mapping[old])
    for i in range(frames):
        old = struct.unpack_from('<I',frame_data,i*20)[0]
        struct.pack_into('<I',frame_data,i*20,mapping[old])
    result = bytearray(b'HKROOM02'+struct.pack('<8I',len(atlas.pages),len(atlas.entries),draws,frames,clips,edges,len(atlas.stream),atlas.flags))
    for entry in atlas.entries:result.extend(struct.pack('<6HI',*entry))
    result.extend(draw_data);result.extend(frame_data);result.extend(raw[at:prefix])
    result.extend(b''.join(atlas.palettes));result.extend(b''.join(atlas.pages));result.extend(atlas.stream)
    result = bytes(result)
    new_blobs = textures(result)
    if any(blob != new_blobs[mapping[i]] for i,blob in enumerate(expected)):
        raise ValueError('Canonical texture remapping changed palette or texels')
    return result, {'replaced_textures':sorted(replacements),'old_to_canonical':mapping,'textures_before':texture_count,'textures_after':len(atlas.entries),
                    'pages_before':pages,'pages_after':len(atlas.pages),
                    'stream_bytes_before':struct.unpack_from('<I',raw,32)[0],'stream_bytes_after':len(atlas.stream),
                    'raw_bytes_before':len(raw),'raw_bytes_after':len(result),'alpha_cover_bytes':atlas.alpha_cover_bytes,'animation_bytes':atlas.animation_bytes}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--metadata',type=Path,default=ROOT/'.hkpsx/selected-regions.json')
    parser.add_argument('--output',type=Path,default=ROOT/'.hkpsx/texture-dedup')
    args=parser.parse_args();metadata=json.loads(args.metadata.read_text())
    args.output.mkdir(parents=True,exist_ok=True);records=[]
    for row in metadata['regions']:
        raw=(ROOT/row['path']).read_bytes()
        if hashlib.sha256(raw).hexdigest()!=row['sha256']:raise ValueError('Baseline pack differs from metadata')
        packed,record=deduplicate_room(raw)
        record.update(chunk_id=row['chunk_id'],source_sha256=row['sha256'],
                      output_sha256=hashlib.sha256(packed).hexdigest())
        (args.output/f'chunk-{row["chunk_id"]:03}.hk').write_bytes(packed)
        records.append(record)
    summary={field:sum(r[field]for r in records) for field in records[0]
             if field.endswith('_before')or field.endswith('_after')}
    report={'metadata_sha256':hashlib.sha256(args.metadata.read_bytes()).hexdigest(),
            'code_sha256':{name:hashlib.sha256((ROOT/'host'/name).read_bytes()).hexdigest()
                           for name in ['texture_dedup.py','cook.py','alpha_covers.py']},
            'regions':records,'summary':summary,'room_count':len(records),
            'semantics':'Exact dimensions, all CLUT words and all indexed pixels preserved; draw/frame geometry and ordering unchanged'}
    (args.output/'report.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(summary,indent=2))


if __name__=='__main__':main()
