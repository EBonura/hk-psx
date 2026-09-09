#!/usr/bin/env python3
"""Measure lossless, palette-independent static indexplane sharing.

No asset/disc mutation. JSON records dimensions, hashes and source references;
full source pixels stay in the locally supplied cooked packs.
"""
import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from region_delta import textures
from scene_bank import shared_planes
from audit_resident_bank import dense_pack,aligned

def digest(data):return hashlib.sha256(data).hexdigest()
def analyze(metadata):
    source=metadata.read_bytes();rows=json.loads(source)['regions'];scenes=[]
    for scene in sorted({r['scene_id']for r in rows}):
        blobs=[];lookup={};refs=[]
        for r in rows:
            if r['scene_id']!=scene:continue
            raw=(ROOT/r['path']).read_bytes()
            if digest(raw)!=r['sha256']:raise ValueError('Stale source pack')
            for i,blob in enumerate(textures(raw)):
                if struct.unpack_from('<H',raw,40+i*16)[0]==65535:continue
                if blob not in lookup:lookup[blob]=len(blobs);blobs.append(blob);refs.append([])
                refs[lookup[blob]].append({'chunk_id':r['chunk_id'],'texture':i,'pack_sha256':r['sha256']})
        planes,mapping,proof=shared_planes([b'\0'+b for b in blobs],True)
        oldpages,_=dense_pack([(aligned(struct.unpack_from('<H',b)[0]),struct.unpack_from('<H',b,2)[0],i)for i,b in enumerate(blobs)])
        pages,positions=dense_pack([(aligned(w),h,i)for i,(w,h,p)in enumerate(planes)])
        for group in proof:
            group['source_refs']=[refs[i]for i in group['texture_ids']]
            group['palette_words']=[list(struct.unpack('<16H',mapping[i][1]))for i in group['texture_ids']]
        scenes.append({'scene_id':scene,'original_textures':len(blobs),'joint_planes':len(planes),
            'original_pages':oldpages,'shared_pages':pages,'aligned_plane_bytes':sum(aligned(w)*h//2 for w,h,p in planes),
            'saved_texel_bytes':sum(p['saved_texel_bytes']for p in proof),'groups':proof,
            'placement_sha256':digest(json.dumps(positions).encode())})
    return {'source_metadata_sha256':digest(source),'tool_sha256':digest(Path(__file__).read_bytes()),
        'scene_bank_sha256':digest((ROOT/'host/scene_bank.py').read_bytes()),'scenes':scenes,
        'proof':'Every member reconstructs the exact source16bit PSXword per pixel. Separate CLUTs preserve RGB,STP,word0 transparency. Strict binary masks and animations excluded. Deterministic greedy grouping is not a global optimum.'}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--metadata',type=Path,default=ROOT/'data/regions.json');p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    report=analyze(a.metadata);a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps([{k:v for k,v in s.items()if k!='groups'}for s in report['scenes']],indent=2))
if __name__=='__main__':main()
