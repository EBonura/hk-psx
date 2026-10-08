"""Offline, lossless HKROOM02 transition prototype; never writes discs.

Canonical texture identity includes dimensions, all 16 PSX CLUT words and
every 4bpp texel. Atlas positions and padding are reconstructed independently.
The prototype keeps the old room immutable while constructing the next room.
Its transfer sizes exclude a future disc index and do not measure PS1 timing.
"""
import argparse
import csv
import hashlib
import json
import statistics
import struct
from pathlib import Path

import lz4.block

ROOT = Path(__file__).resolve().parents[1]
MAGIC = b'HKDLTA01'


def compressed(data):
    packed = b'HLZC'+struct.pack('<I', len(data))+lz4.block.compress(data, mode='high_compression', store_size=False)
    return packed if len(packed) < len(data) else data


def layout(raw):
    if len(raw) < 40 or raw[:8] != b'HKROOM02':
        raise ValueError('Invalid room header')
    if struct.unpack_from('<I',raw,36)[0]&~1:raise ValueError('Unknown room features')
    counts = struct.unpack_from('<6I', raw, 8)
    pages, textures, draws, frames, clips, edges = counts
    prefix = 40+textures*16+draws*44+frames*20+clips*16+edges*16
    page_start = prefix+textures*32
    stream_start = page_start+pages*32768
    if stream_start+struct.unpack_from('<I', raw, 32)[0] != len(raw):
        raise ValueError('Invalid room length')
    return counts, prefix, page_start, stream_start


def read_row(raw, at, odd, width):
    pairs = width//2
    if odd:
        row = bytes((raw[at+i] >> 4) | ((raw[at+i+1] & 15) << 4) for i in range(pairs))
        return row+bytes([raw[at+pairs] >> 4]) if width & 1 else row
    row = raw[at:at+pairs]
    return row+bytes([raw[at+pairs] & 15]) if width & 1 else row


def write_row(raw, at, odd, width, row):
    if odd:
        raw[at] = (raw[at] & 15) | ((row[0] & 15) << 4)
        middle = bytes((row[i-1] >> 4) | ((row[i] & 15) << 4) for i in range(1,len(row)))
        raw[at+1:at+len(row)] = middle
        if not width & 1:
            raw[at+len(row)] = (raw[at+len(row)] & 240) | (row[-1] >> 4)
    else:
        pairs = width//2
        raw[at:at+pairs] = row[:pairs]
        if width & 1:
            raw[at+pairs] = (raw[at+pairs] & 240) | row[-1]


def textures(raw):
    """Extract compact texels even when static images begin on odd nibbles."""
    counts, prefix, page_start, stream_start = layout(raw)
    result = []
    for i in range(counts[1]):
        page, u, v, width, height, palette, offset = struct.unpack_from('<6HI', raw, 40+i*16)
        if not width or not height or palette >= counts[1]:
            raise ValueError('Invalid texture')
        if page == 65535:
            stride = ((width+3)&~3)//2
            base = stream_start+offset
            x = 0
        else:
            if page >= counts[0] or u+width > 256 or v+height > 256:
                raise ValueError('Invalid atlas texture')
            stride, base, x = 128, page_start+page*32768+v*128, u
        pixels = b''.join(read_row(raw,base+yy*stride+x//2,x&1,width) for yy in range(height))
        result.append(struct.pack('<HH', width, height)+raw[prefix+palette*32:prefix+(palette+1)*32]+pixels)
    return result


def reconstruct(prefix, raw_size, blobs):
    raw = bytearray(raw_size)
    raw[:len(prefix)] = prefix
    counts, actual_prefix, page_start, stream_start = layout(raw)
    if actual_prefix != len(prefix) or len(blobs) != counts[1]:
        raise ValueError('Invalid reconstruction recipe')
    for i, blob in enumerate(blobs):
        page, u, v, width, height, palette, offset = struct.unpack_from('<6HI', raw, 40+i*16)
        if len(blob) != 36+((width+1)//2)*height or struct.unpack_from('<HH', blob) != (width, height):
            raise ValueError('Texture geometry mismatch')
        raw[actual_prefix+palette*32:actual_prefix+(palette+1)*32] = blob[4:36]
        if page == 65535:
            stride, base, x = ((width+3)&~3)//2, stream_start+offset, 0
        else:
            stride, base, x = 128, page_start+page*32768+v*128, u
        row_bytes = (width+1)//2
        for yy in range(height):
            write_row(raw,base+yy*stride+x//2,x&1,width,blob[36+yy*row_bytes:36+(yy+1)*row_bytes])
        if page!=65535 and struct.unpack_from('<I',raw,36)[0]&1:
            from alpha_covers import record
            cover=record(width,height,blob[4:36],blob[36:])
            if offset&3 or stream_start+offset+len(cover)>len(raw):raise ValueError('Invalid alpha record offset')
            raw[stream_start+offset:stream_start+offset+len(cover)]=cover
    return bytes(raw)


def encode_delta(old, new, old_blobs=None, new_blobs=None):
    old_blobs = (textures(old) if old else []) if old_blobs is None else old_blobs
    new_blobs = textures(new) if new_blobs is None else new_blobs
    donors = {blob:i for i, blob in enumerate(old_blobs)}
    missing, references, local = [], [], {}
    for blob in new_blobs:
        if blob in donors:
            references.append(donors[blob])
        else:
            if blob not in local:
                local[blob] = len(missing)
                missing.append(blob)
            references.append(0x80000000 | local[blob])
    prefix_len = layout(new)[1]
    delta = MAGIC+struct.pack('<4I', len(new), prefix_len, len(references), len(missing))
    delta += hashlib.sha256(old).digest()+hashlib.sha256(new).digest()+new[:prefix_len]
    delta += struct.pack('<'+'I'*len(references), *references)
    delta += b''.join(struct.pack('<I', len(blob))+blob for blob in missing)
    return delta


def apply_delta(old, delta, old_blobs=None):
    if delta[:4] == b'HLZC':
        delta = lz4.block.decompress(delta[8:], uncompressed_size=struct.unpack_from('<I', delta, 4)[0])
    if len(delta) < 88 or delta[:8] != MAGIC or hashlib.sha256(old).digest() != delta[24:56]:
        raise ValueError('Invalid delta/base digest')
    size, prefix_len, count, missing_count = struct.unpack_from('<4I', delta, 8)
    at = 88+prefix_len
    refs = struct.unpack_from('<'+'I'*count, delta, at)
    at += count*4
    missing = []
    for _ in range(missing_count):
        length = struct.unpack_from('<I', delta, at)[0]
        at += 4
        missing.append(delta[at:at+length])
        at += length
    if at != len(delta):
        raise ValueError('Truncated/trailing delta bytes')
    donors = (textures(old) if old else []) if old_blobs is None else old_blobs
    blobs = [missing[ref & 0x7fffffff] if ref & 0x80000000 else donors[ref] for ref in refs]
    result = reconstruct(delta[88:88+prefix_len], size, blobs)
    if hashlib.sha256(result).digest() != delta[56:88]:
        raise ValueError('Reconstructed target digest mismatch')
    return result


def cd_reads(path, regions, header_sectors):
    """Decode exact BCD Setloc starts; only whole-chunk starts are classified."""
    sector = 1024+header_sectors
    by_lba = {}
    for region in regions:
        by_lba[sector] = region
        sector += (region['stored_bytes']+2047)//2048
    reads = []
    for row in csv.DictReader(path.open()):
        if row['command'] != '0x02':
            continue
        bcd = [int(value, 16) for value in row['params'].split()]
        minute, second, frame = [(value >> 4)*10+(value & 15) for value in bcd]
        lba = (minute*60+second)*75+frame-150
        if lba == 1024:
            continue
        region = by_lba.get(lba)
        reads.append({'cycle':int(row['cycle']), 'lba':lba, 'chunk_id':region['chunk_id'] if region else None,
                      'stored_bytes':region['stored_bytes'] if region else None})
    return reads


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--metadata', type=Path, default=ROOT/'.hkpsx/selected-regions.json')
    parser.add_argument('--packed', type=Path, default=ROOT/'.hkpsx/packed-rooms.json')
    parser.add_argument('--output', type=Path, default=ROOT/'.hkpsx/region-delta-analysis.json')
    parser.add_argument('--cd-log', action='append', type=Path, default=[])
    args = parser.parse_args()
    metadata, packed = json.loads(args.metadata.read_text()), json.loads(args.packed.read_text())
    rows = metadata['regions']
    raw, blobs, inventory, unique = {}, {}, [], {}
    for row in rows:
        ident = row['chunk_id']
        raw[ident] = (ROOT/row['path']).read_bytes()
        if hashlib.sha256(raw[ident]).hexdigest() != row['sha256']:
            raise ValueError('Pack changed since selected metadata')
        blobs[ident] = textures(raw[ident])
        prefix = layout(raw[ident])[1]
        if reconstruct(raw[ident][:prefix], len(raw[ident]), blobs[ident]) != raw[ident]:
            raise ValueError(f'Atlas/padding reconstruction failed for chunk {ident}')
        for blob in blobs[ident]:
            unique[blob] = unique.get(blob, 0)+1
        delta = compressed(encode_delta(b'', raw[ident], [], blobs[ident]))
        assert apply_delta(b'', delta, []) == raw[ident]
        inventory.append({'chunk_id':ident, 'texture_references':len(blobs[ident]), 'unique_textures':len(set(blobs[ident])),
                          'canonical_bytes':sum(map(len, blobs[ident])), 'metadata_bytes':prefix,
                          'self_contained_recipe_stored_bytes':len(delta), 'raw_sha256':row['sha256']})
    print('Verified complete reconstruction for', len(rows), 'rooms', flush=True)
    transitions = []
    for row in rows:
        origin = row['chunk_id']
        for target in row['neighbour_chunks']:
            raw_delta = encode_delta(raw[origin], raw[target], blobs[origin], blobs[target])
            delta = compressed(raw_delta)
            assert apply_delta(raw[origin], delta, blobs[origin]) == raw[target]
            target_unique = set(blobs[target]); shared = target_unique & set(blobs[origin])
            full = packed['regions'][target-1]['stored_bytes']
            transitions.append({'from':origin, 'to':target, 'full_stored_bytes':full, 'delta_stored_bytes':len(delta),
                                'shared_textures':len(shared), 'target_unique_textures':len(target_unique),
                                'shared_canonical_bytes':sum(map(len, shared)), 'new_canonical_bytes':sum(map(len,target_unique-shared)),
                                'raw_delta_bytes':len(raw_delta), 'target_plus_raw_delta_bytes':len(raw[target])+len(raw_delta),
                                'full_sectors':(full+2047)//2048, 'delta_sectors':(len(delta)+2047)//2048})
    # These bank counts are storage bounds, not a proposed all-resident PS1 bank.
    report = {'scope':__doc__, 'metadata_sha256':hashlib.sha256(args.metadata.read_bytes()).hexdigest(),
              'packed_report_sha256':hashlib.sha256(args.packed.read_bytes()).hexdigest(),
              'prototype_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'room_count':len(rows), 'verified_neighbor_transitions':len(transitions), 'current_stored_bytes':packed['stored_bytes'],
              'current_raw_bytes':packed['raw_bytes'], 'texture_references':sum(len(b) for b in blobs.values()),
              'unique_palette_texel_textures':len(unique), 'all_canonical_bytes':sum(len(b)*n for b,n in unique.items()),
              'unique_canonical_bytes':sum(map(len,unique)), 'unique_individually_compressed_bytes':sum(len(compressed(b)) for b in unique),
              'maximum_canonical_texture_bytes':max(map(len,unique)),
              'maximum_individually_compressed_texture_bytes':max(len(compressed(b)) for b in unique),
              'individually_sector_aligned_texture_bank_sectors':sum((len(compressed(b))+2047)//2048 for b in unique),
              'all_metadata_bytes':sum(r['metadata_bytes'] for r in inventory), 'regions':inventory, 'transitions':transitions,
              'cd_traces':{str(path):cd_reads(path, packed['regions'], packed['header_sectors']) for path in args.cd_log},
              'cd_trace_sha256':{str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in args.cd_log},
              'summary':{'full_transition_bytes':sum(t['full_stored_bytes'] for t in transitions),
                         'delta_transition_bytes':sum(t['delta_stored_bytes'] for t in transitions),
                         'median_transfer_saving_fraction':statistics.median(1-t['delta_stored_bytes']/t['full_stored_bytes'] for t in transitions),
                         'full_transition_sectors':sum(t['full_sectors'] for t in transitions),
                         'delta_transition_sectors':sum(t['delta_sectors'] for t in transitions),
                         'maximum_uncompressed_delta_bytes':max(t['raw_delta_bytes'] for t in transitions),
                         'maximum_target_plus_delta_bytes':max(t['target_plus_raw_delta_bytes'] for t in transitions),
                         'target_plus_delta_exceeds_768k':sum(t['target_plus_raw_delta_bytes']>768*1024 for t in transitions)},
              'limitations':['Offline prototype only; no guest decoder or PS1 measurements.',
                             'Transition files assume the named donor is still resident; full fallback remains necessary.',
                             'Does not remove complete static VRAM upload on region changes.',
                             'A bounded guest scratch/overlap schedule is unimplemented; two existing room slots alone do not prove reconstruction fits.',
                             'Texture bank storage excludes directory/index overhead and seek/read scheduling.',
                             'Repeated CD loads are observable; prefetch usefulness requires selected-region history, not CD commands alone.']}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print('Verified', len(transitions), 'transition patches;',len(unique),'distinct palette+texel textures; report', args.output)


if __name__ == '__main__':
    main()
