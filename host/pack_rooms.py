"""Bind exact cooked regions to compressed CD chunks and checked guest metadata."""
import argparse,hashlib,json,struct,subprocess,os
from pathlib import Path
import lz4.block
from source import ROOT,rel
from world import generate
from quality import ROOM_BYTE_BUDGET,STATIC_PAGE_BUDGET,TEXTURE_BUDGET

def fnv(data):
 v=0x811c9dc5
 for b in data:v=((v^b)*0x01000193)&0xffffffff
 return v

def main():
 p=argparse.ArgumentParser();p.add_argument('metadata',type=Path);a=p.parse_args()
 report=json.loads(a.metadata.read_text());generate(report)
 out=ROOT/'build/room-chunks';out.mkdir(parents=True,exist_ok=True)
 records=[]
 for r in report['regions']:
  raw=(ROOT/r['path']).read_bytes()
  assert len(raw)==r['bytes'] and hashlib.sha256(raw).hexdigest()==r['sha256']
  if len(raw)>ROOM_BYTE_BUDGET:raise ValueError(f'{r["path"]}: room exceeds {ROOM_BYTE_BUDGET}-byte arena')
  if raw[:8]!=b'HKROOM02' or len(raw)<40:raise ValueError(f'{r["path"]}: invalid room header')
  if struct.unpack_from('<I',raw,8)[0]>STATIC_PAGE_BUDGET:raise ValueError(f'{r["path"]}: scenery exceeds {STATIC_PAGE_BUDGET}-page bank')
  if struct.unpack_from('<I',raw,12)[0]>TEXTURE_BUDGET:raise ValueError(f'{r["path"]}: texture table exceeds {TEXTURE_BUDGET}-CLUT bank')
  compressed=b'HLZC'+struct.pack('<I',len(raw))+lz4.block.compress(raw,mode='high_compression',store_size=False)
  stored=compressed if len(compressed)<len(raw) else raw
  if ((len(stored)+2047)//2048)*2048>ROOM_BYTE_BUDGET:raise ValueError(f'{r["path"]}: sector-rounded chunk exceeds room arena')
  path=out/f'chunk_{r["chunk_id"]}.hk';path.write_bytes(stored)
  records.append({'chunk_id':r['chunk_id'],'path':rel(path),'raw_bytes':len(raw),'raw_fnv':fnv(raw),'stored_bytes':len(stored),'stored_fnv':fnv(stored)})
 # Select the smallest64KiB-aligned arena that passes both the pinned SDK
 # decoder and the exact resumable guest decoder for every shipped chunk.
 # Raw size alone cannot prove safe in-place LZ4 source/output overlap.
 minimum=max(max(r['raw_bytes'],((r['stored_bytes']+2047)//2048)*2048) for r in records)
 arena_bytes=((minimum+65535)//65536)*65536
 checks=[]
 while arena_bytes<=ROOM_BYTE_BUDGET:
  env=dict(os.environ,HK_ROOM_ARENA_BYTES=str(arena_bytes),HK_ROOM_MAX_PAGES=str(STATIC_PAGE_BUDGET),HK_ROOM_MAX_TEXTURES=str(TEXTURE_BUDGET))
  success=True
  for example in ('check_compressed','check_incremental'):
   check=['cargo','run','--quiet','--locked','--offline','--manifest-path',str(ROOT/'shared/hk-format/Cargo.toml'),'--example',example,'--']+[r['path'] for r in records]
   result=subprocess.run(check,text=True,cwd=ROOT,env=env,capture_output=True)
   checks.append({'arena_bytes':arena_bytes,'example':example,'returncode':result.returncode})
   if result.returncode:
    success=False;break
   lines=result.stdout.splitlines()
   if len(lines)!=len(records):raise ValueError('Incomplete arena verification: '+example)
   for line,r in zip(lines,records):
    fields=line.rsplit(' ',2 if example=='check_compressed' else 1)
    size=fields[-2] if example=='check_compressed' else fields[-1]
    if int(size)!=r['raw_bytes']:raise ValueError('Arena verification length mismatch')
    if example=='check_compressed' and int(fields[-1])!=r['raw_fnv']:raise ValueError('Arena verification checksum mismatch')
  if success:break
  arena_bytes+=65536
 else:raise ValueError('No permitted arena fits all chunks; checks: '+str(checks))
 source=f'pub const ROOM_ARENA_BYTES:usize={arena_bytes};\n'
 source+='pub const ROOM_MANIFEST: &[(usize,u32,usize,u32)] = &[\n'+''.join(f'({r["stored_bytes"]},{r["stored_fnv"]},{r["raw_bytes"]},{r["raw_fnv"]}),\n' for r in records)+'];\n'
 (ROOT/'data/room_manifest.rs').write_text(source)
 (ROOT/'.hkpsx/packed-rooms.json').write_text(json.dumps({'room_arena_bytes':arena_bytes,'arena_verification':checks,'room_cook_byte_budget':ROOM_BYTE_BUDGET,'static_page_budget':STATIC_PAGE_BUDGET,'texture_budget':TEXTURE_BUDGET,'regions':records,'header_sectors':(28+len(records)*24+2047)//2048,'metadata':str(a.metadata),'stored_bytes':sum(r['stored_bytes'] for r in records),'raw_bytes':sum(r['raw_bytes'] for r in records)},indent=2))
 print('Packed',len(records),'regions,',sum(r['stored_bytes'] for r in records),'stored bytes')
if __name__=='__main__':main()
