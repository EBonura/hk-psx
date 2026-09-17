"""Deterministic disjoint covers of visible PSX 4bpp texels; no pixel edits."""
import struct

HAS_ALPHA_COVERS=1
RECORD_BYTES=20
MAX_RECTS=4

def support(width,height,palette,pixels):
    if not 1<=width<=256 or not 1<=height<=256 or len(palette)!=32 or len(pixels)!=((width+1)//2)*height:
        raise ValueError('invalid alpha cover texture')
    colours=struct.unpack('<16H',palette);stride=(width+1)//2
    # Actual PSX colour word determines transparency, even at index zero.
    # Opaque black 0x8000 remains visible; padding nibbles are excluded.
    return [[colours[(pixels[y*stride+x//2]>>((x&1)*4))&15]!=0 for x in range(width)] for y in range(height)]

def area(rect):return (rect[2]-rect[0])*(rect[3]-rect[1])

def cover(mask):
    height=len(mask);width=len(mask[0]) if height else 0
    if not 1<=height<=256 or not 1<=width<=256 or any(len(row)!=width for row in mask):
        raise ValueError('invalid alpha support mask')
    rows=[sum(1<<x for x,visible in enumerate(row) if visible) for row in mask]
    def bounds(left,top,right,bottom):
        bits=((1<<right)-1)^((1<<left)-1)
        union=0;first=bottom;last=top
        for y in range(top,bottom):
            row=rows[y]&bits
            if row:
                union|=row;first=min(first,y);last=y+1
        if not union:return None
        return ((union&-union).bit_length()-1,first,union.bit_length(),last)
    first=bounds(0,0,width,height)
    if first is None:return []
    rects=[first]
    while len(rects)<MAX_RECTS:
        best=None
        for index,(left,top,right,bottom) in enumerate(rects):
            for axis in (0,1):
                for cut in range((left if axis==0 else top)+1,right if axis==0 else bottom):
                    ranges=[(left,top,cut,bottom),(cut,top,right,bottom)] if axis==0 else [(left,top,right,cut),(left,cut,right,bottom)]
                    children=[bounds(*r) for r in ranges]
                    children=[child for child in children if child is not None]
                    saved=area(rects[index])-sum(map(area,children))
                    # Stable ties match the reference's first rectangle/axis/cut.
                    if saved>0 and (best is None or saved>best[0]):best=(saved,index,children)
        if best is None:break
        _,index,children=best;rects[index:index+1]=children
    covered=[0]*height
    for left,top,right,bottom in rects:
        bits=((1<<right)-1)^((1<<left)-1)
        for y in range(top,bottom):
            assert not covered[y]&bits,'alpha cover rectangles overlap'
            covered[y]|=bits
    assert all(not (row&~seen) for row,seen in zip(rows,covered)),'alpha cover omitted a visible PSX texel'
    return rects

# The cover is a pure function of the texel words, and the same textures recur
# across regions and postpasses (72k calls per warm cook, two thirds of its
# time in this file), so finished records persist in ignored storage.
_CACHE_PATH=None
_CACHE={}
_DIRTY=False

def _cache():
    global _CACHE_PATH
    if _CACHE_PATH is None:
        import atexit,json
        from pathlib import Path
        _CACHE_PATH=Path(__file__).resolve().parents[1]/'.hkpsx'/'alpha-covers-cache.json'
        try:_CACHE.update(json.loads(_CACHE_PATH.read_text()))
        except (OSError,ValueError):pass
        atexit.register(_flush)
    return _CACHE

def _flush():
    if not _DIRTY:return
    import json,os
    _CACHE_PATH.parent.mkdir(parents=True,exist_ok=True)
    # Parallel cook workers flush concurrently: merge what others wrote since
    # this process loaded the cache, and rename from a per-process temp file.
    merged={}
    try:merged.update(json.loads(_CACHE_PATH.read_text()))
    except (OSError,ValueError):pass
    merged.update(_CACHE)
    temp=_CACHE_PATH.with_suffix(f'.{os.getpid()}.tmp');temp.write_text(json.dumps(merged,separators=(',',':')));temp.replace(_CACHE_PATH)

def compute_record(width,height,palette,pixels):
    rects=cover(support(width,height,palette,pixels))
    output=bytearray(RECORD_BYTES);output[0]=len(rects)
    for index,(left,top,right,bottom) in enumerate(rects):
        struct.pack_into('4B',output,4+index*4,left,top,right-left-1,bottom-top-1)
    return bytes(output)

def record(width,height,palette,pixels):
    global _DIRTY
    import hashlib
    key=hashlib.sha256(struct.pack('<HH',width,height)+bytes(palette)+bytes(pixels)).hexdigest()[:32]
    cache=_cache();hit=cache.get(key)
    if hit is not None:return bytes.fromhex(hit)
    output=compute_record(width,height,palette,pixels)
    cache[key]=output.hex();_DIRTY=True
    return output
