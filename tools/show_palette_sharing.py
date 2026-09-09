#!/usr/bin/env python3
"""Render verified lossless palette-sharing examples from local room packs."""
import json,struct,sys
from pathlib import Path
import numpy as np
from PIL import Image,ImageDraw
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'));sys.path.insert(0,str(ROOT/'tools'))
from region_delta import textures
from scene_bank import shared_planes
from audit_texture_similarity import inventory,font

OUT=ROOT/'.hkpsx/texture-similarity';OUT.mkdir(exist_ok=True)
metadata=json.loads((ROOT/'data/regions.json').read_bytes());items,_=inventory(metadata)
static=[t for t in items if not t['stream'] and 0 in t['rooms']]
planes,mapping,proof=shared_planes([bytes([0])+t['blob']for t in static],True)
print(str(proof)[:800])
byplane={}
for tid,(plane,palette) in mapping.items():byplane.setdefault(plane,[]).append((tid,palette))
groups=sorted([(plane,entries)for plane,entries in byplane.items()if len(entries)>1],key=lambda g:planes[g[0]][0]*planes[g[0]][1],reverse=True)[:3]
im=Image.new('RGB',(1120,120+len(groups)*340),'#12161d');d=ImageDraw.Draw(im)
d.text((24,18),'One pixel pattern, two original palettes',font=font(28),fill='white')
d.text((24,59),'Lossless: every 16-bit PS1 texel reconstructs exactly, including transparency and blend flags.',font=font(17),fill='#c4d2df')
d.text((24,86),'A / B show stored texture colors. The false-color pattern at right is a diagnostic, not game art.',font=font(15),fill='#a3b3c4')
colors=[(31,40,53),(230,70,80),(20,180,160),(220,160,40),(140,75,220),(60,130,220),(200,90,155),(120,190,70),(230,110,45),(95,75,175),(50,190,220),(195,200,65),(170,100,75),(100,165,180),(220,210,185),(185,100,215)]
for row,(plane,entries) in enumerate(groups):
 w,h,packed=planes[plane];yy,xx=np.indices((h,w));codes=np.frombuffer(packed,dtype=np.uint8).reshape(h,(w+1)//2);indices=(codes[:,xx[0]//2]>>((xx[0]&1)*4))&15
 y=120+row*340;d.line((24,y,1096,y),fill='#405061')
 for col,(tid,pal) in enumerate(entries[:2]):
  t=static[tid];palette=np.frombuffer(pal,dtype='<u2');q=palette[indices]
  if not np.array_equal(q,t['words']):raise ValueError('Palette sharing changed a source texel')
  rgb=np.stack((q&31,(q>>5)&31,(q>>10)&31),axis=-1)*255//31
  checker=np.where(((xx//6+yy//6)%2)[...,None],92,62);rgb=np.where((q==0)[...,None],checker,rgb).astype('uint8')
  x=24+col*340;d.text((x,y+12),f"Texture {'AB'[col]} + palette {'AB'[col]}",font=font(20),fill='white')
  d.text((x,y+41),f"{w} x {h} | texture #{t['id']}",font=font(15),fill='#b7c8d8')
  im.paste(Image.fromarray(rgb).resize((w*4,h*4),Image.Resampling.NEAREST),(x,y+67))
  sy=y+275
  for i,word in enumerate(palette):
   c=tuple(int(((int(word)>>shift)&31)*255//31)for shift in (0,5,10));d.rectangle((x+i*18,sy,x+i*18+16,sy+20),fill=c,outline='#728090')
  d.text((x,sy+27),' / '.join(sorted(t['names'])[:1])[:38],font=font(14),fill='#b7c8d8')
 x=704;d.text((x,y+12),'Shared 4bpp pixel pattern',font=font(20),fill='white')
 used=int(indices.max())+1;d.text((x,y+41),f'{used} of 16 indices used',font=font(15),fill='#b7c8d8')
 diag=np.array(colors,dtype=np.uint8)[indices];im.paste(Image.fromarray(diag).resize((w*4,h*4),Image.Resampling.NEAREST),(x,y+67))
 saved=((w+3)//4*4)//2*h*(len(entries)-1)
 d.text((x,y+275),f'{saved:,} pixel bytes saved',font=font(18),fill='#98e3bb')
 d.text((x,y+302),'0 changed texels',font=font(16),fill='#98e3bb')
im.save(OUT/'palette-sharing.png')
print('Saved',OUT/'palette-sharing.png')
