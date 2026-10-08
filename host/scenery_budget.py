"""Measure static resolution caps on fixed existing regions, without disc writes.

Reuses exact source instance geometry and every original streamed animation
texture. Source SpriteRenderer alpha and original sprite images feed the same
Lanczos/15-colour quantizer as the cooker. All output stays in a separate ignored
experiment directory; current packs and metadata are read-only.
"""
import argparse
import hashlib
import json
import math
import shutil
import struct
from pathlib import Path

from PIL import Image
from source import Source, ROOT
from cook import Atlas
from region_delta import layout, textures, compressed


def digest(data):return hashlib.sha256(data).hexdigest()


def recook(raw, draws, source, cap, pixels, prepared):
    counts, prefix, page_start, stream_start = layout(raw)
    pages, count, draw_count, frame_count, clip_count, edge_count = counts
    if len(draws) != draw_count:raise ValueError('draw provenance count mismatch')
    blobs = textures(raw)
    atlas = Atlas(); draw_textures = {}; old_streams = {}
    original_draw_offset = 40 + count*16
    # Source serialized object order is ascending path ID in the observed scene.
    # Preserve the cooker's first-admission choice for shared quantized images.
    instance_cache = {}
    for index, draw in sorted(enumerate(draws), key=lambda item:int(item[1]['source'].split(':')[-1])):
        encoded = raw[original_draw_offset+index*44:original_draw_offset+(index+1)*44]
        expected = struct.pack('<HHI8i4B',struct.unpack_from('<H',encoded)[0],int(draw['z']<0),round(draw['scale']*4096),
            *[round(p[k]*draw['scale']*256) for p in draw['points'] for k in (0,1)],*draw['tint'],draw.get('material',{}).get('mode',0))
        if expected != encoded:raise ValueError('draw provenance differs from exact original pack')
        w = math.dist(draw['points'][0],draw['points'][1])*draw['scale']
        h = math.dist(draw['points'][0],draw['points'][2])*draw['scale']
        file, path = draw['source'].split(':');renderer = source.read(source.file(file).objects[int(path)])
        alpha = renderer['m_Color']['a']
        key = (draw['sprite'],math.ceil(w),math.ceil(h),round(alpha*255))
        if key not in instance_cache:
            reduction = min(1.0,cap/max(w,h));size = (max(1,math.ceil(w*reduction)),max(1,math.ceil(h*reduction)))
            prepared_key = (cap,key)
            if prepared_key not in prepared:
                if draw['sprite'] not in pixels:
                    sprite_file,sprite_path = draw['sprite'].split(':')
                    pixels[draw['sprite']] = source.file(sprite_file).objects[int(sprite_path)].read().image.convert('RGBA')
                image = pixels[draw['sprite']].copy()
                image.putalpha(image.getchannel('A').point(lambda a:round(a*alpha)))
                prepared[prepared_key] = image.resize(size,Image.Resampling.LANCZOS)
            instance_cache[key] = atlas.add(prepared[prepared_key],w*reduction,h*reduction)
        draw_textures[index] = instance_cache[key]
    for index, blob in enumerate(blobs):
        if struct.unpack_from('<H',raw,40+index*16)[0] != 65535:continue
        w,h = struct.unpack_from('<HH',blob)
        old_streams[index] = atlas.add_quantized(w,h,blob[4:36],blob[36:],streamed=True)
    atlas.pack()
    result = bytearray(b'HKROOM02'+struct.pack('<8I',len(atlas.pages),len(atlas.entries),draw_count,frame_count,clip_count,edge_count,len(atlas.stream),atlas.flags))
    for entry in atlas.entries:result.extend(struct.pack('<6HI',*entry))
    for index in range(draw_count):
        record = bytearray(raw[original_draw_offset+index*44:original_draw_offset+(index+1)*44])
        struct.pack_into('<H',record,0,draw_textures[index]);result.extend(record)
    frame_offset = original_draw_offset+draw_count*44
    for index in range(frame_count):
        record = bytearray(raw[frame_offset+index*20:frame_offset+(index+1)*20])
        old = struct.unpack_from('<I',record)[0]
        if old not in old_streams:raise ValueError('animation frame references static scenery')
        struct.pack_into('<I',record,0,old_streams[old]);result.extend(record)
    result.extend(raw[frame_offset+frame_count*20:prefix])
    result.extend(b''.join(atlas.palettes));result.extend(b''.join(atlas.pages));result.extend(atlas.stream)
    result = bytes(result);new_blobs = textures(result)
    for old,new in old_streams.items():
        if blobs[old] != new_blobs[new]:raise ValueError('animation texels or palette changed')
    static_mismatches = sum(blobs[struct.unpack_from('<H',raw,original_draw_offset+i*44)[0]] != new_blobs[t]
                            for i,t in draw_textures.items())
    return result, {'pages':len(atlas.pages),'textures':len(atlas.entries),'draws':draw_count,
        'stream_bytes':len(atlas.stream),'raw_bytes':len(result),'stored_bytes':len(compressed(result)),
        'draws_with_different_texels':static_mismatches,'animations_preserved':len(old_streams),
        'sha256':digest(result)}


def summarize(rows, regions):
    by_id = {r['chunk_id']:r for r in rows}
    windows = []
    for region in regions:
        current = region['chunk_id'];neighbours = region.get('neighbour_chunks',[])
        # Worst choice of any two distinct immediate neighbors, plus current.
        chosen = sorted(neighbours,key=lambda n:by_id[n]['raw_bytes'],reverse=True)[:2]
        windows.append({'chunk_id':current,'neighbors':neighbours,'worst_raw_neighbor_pair':chosen,
            'three_raw_bytes':sum(by_id[n]['raw_bytes'] for n in [current]+chosen),
            'three_pages':by_id[current]['pages']+sum(sorted((by_id[n]['pages'] for n in neighbours),reverse=True)[:2]),
            'all_neighbor_raw_bytes':sum(by_id[n]['raw_bytes'] for n in [current]+neighbours)})
    return {'regions':len(rows),'total_pages':sum(r['pages'] for r in rows),
        'max_pages':max(r['pages'] for r in rows),'regions_over_6_pages':[r['chunk_id'] for r in rows if r['pages']>6],
        'total_raw_bytes':sum(r['raw_bytes'] for r in rows),'max_raw_bytes':max(r['raw_bytes'] for r in rows),
        'total_stored_bytes':sum(r['stored_bytes'] for r in rows),'max_stored_bytes':max(r['stored_bytes'] for r in rows),
        'max_inplace_raw_plus_stored_bytes':max(r['raw_bytes']+r['stored_bytes'] for r in rows),
        'regions_over_384k_raw':[r['chunk_id'] for r in rows if r['raw_bytes']>384*1024],
        'regions_over_384k_raw_plus_stored':[r['chunk_id'] for r in rows if r['raw_bytes']+r['stored_bytes']>384*1024],
        'max_three_raw_bytes':max(w['three_raw_bytes'] for w in windows),
        'max_three_pages':max(w['three_pages'] for w in windows),'windows':windows}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'.hkpsx/scenery-budget')
    parser.add_argument('--caps',type=int,nargs='+',default=[128,80,64])
    args=parser.parse_args();out=args.output.resolve()
    if not out.is_relative_to(ROOT/'.hkpsx'):raise ValueError('experiment output must stay under .hkpsx')
    out.mkdir(parents=True,exist_ok=True)
    baseline=out/'baseline';baseline.mkdir(exist_ok=True)
    report_path=baseline/'regions.json'
    if not report_path.exists():
        shutil.copy2(ROOT/'data/regions.json',report_path)
        shutil.copy2(ROOT/'data/regions.rs',baseline/'regions.rs')
        for name in ('regions-cook-cache.json','regions-provenance.json'):
            shutil.copy2(ROOT/'.hkpsx'/name,baseline/name)
        report=json.loads(report_path.read_text())
        for region in report['regions']:
            chunk=region['chunk_id'];shutil.copy2(ROOT/region['path'],baseline/f'chunk-{chunk}.hk')
            shutil.copy2(ROOT/f'data/regions/region-{chunk:03}/scene.json',baseline/f'scene-{chunk}.json')
    report=json.loads(report_path.read_text());source=Source();pixels={};prepared={};summaries={}
    for cap in args.caps:
        if not 16<=cap<=128:raise ValueError('supported experimental cap16..128')
        directory=out/f'cap-{cap}';directory.mkdir(exist_ok=True);rows=[]
        for region in report['regions']:
            chunk=region['chunk_id'];raw=(baseline/f'chunk-{chunk}.hk').read_bytes()
            if digest(raw)!=region['sha256']:raise ValueError('baseline pack hash mismatch')
            draws=json.loads((baseline/f'scene-{chunk}.json').read_text())['draws']
            packed,row=recook(raw,draws,source,cap,pixels,prepared)
            row['chunk_id']=chunk;rows.append(row);(directory/f'chunk-{chunk}.hk').write_bytes(packed)
            stored=directory/'compressed';stored.mkdir(exist_ok=True)
            (stored/f'chunk-{chunk}.hk').write_bytes(compressed(packed))
            if cap==128 and row['draws_with_different_texels']:
                raise ValueError(f'128 source replay differs from baseline at chunk{chunk}: {row["draws_with_different_texels"]} draws')
            if chunk%10==0:print(cap,chunk,'pages',row['pages'],'raw',row['raw_bytes'],flush=True)
        summary=summarize(rows,report['regions']);summary['rows']=rows
        (directory/'report.json').write_text(json.dumps(summary,indent=2));summaries[str(cap)]=summary
        print('CAP',cap,{k:v for k,v in summary.items() if k not in ('rows','windows')},flush=True)
    summaries={p.parent.name.removeprefix('cap-'):json.loads(p.read_text()) for p in out.glob('cap-*/report.json')}
    (out/'report.json').write_text(json.dumps(summaries,indent=2))
if __name__=='__main__':main()
