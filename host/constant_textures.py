"""Lossless constant-black texture collapse; geometry and material stay intact.

Only static, non-frame textures with the strict mask CLUT and index1 in every
sample qualify. A4x4 index1 tile covers the same complete quad at every tint.
Dynamic fades remain safe: palette index1 and the entire original CLUT are
unchanged, including the renderer's alternate fade-CLUT lookup contract. Four texels per
axis retain a usable UV span for the large-quad subdivision helper; a texture
whose draw is too large even for that span stays exact.
"""
import struct
from region_delta import layout, textures
from scenery_geometry import texture_draws_safe

STRICT_MASK_PALETTE = struct.pack('<16H', 0, 1, *([0x8000]*14))


def _constant(blob):
    """Validate the full visible sample domain, never a hash or alpha bound."""
    if len(blob)<36:return False
    width,height=struct.unpack_from('<HH',blob)
    if not 1<=width<=256 or not 1<=height<=256:return False
    stride=(width+1)//2
    if len(blob)!=36+stride*height or blob[4:36]!=STRICT_MASK_PALETTE:return False
    for y in range(height):
        for x in range(width):
            index=(blob[36+y*stride+x//2]>>((x&1)*4))&15
            if index!=1:return False
    return True


def constant_texture_replacements(raw, protected_texture_ids=()):
    """Return local textureID→replacement blob; callers add global frame guards.

    Every local frame reference is automatically protected, even if its texture
    is stored in static pages. Additional IDs should reflect scene-global frame
    usage before an integration shares source textures between region packs.
    """
    counts,_,_,_=layout(raw);blobs=textures(raw);protected=set(protected_texture_ids)
    if any(not isinstance(i,int) or not 0<=i<counts[1] for i in protected):
        raise ValueError('Invalid protected texture index')
    frame_at=40+counts[1]*16+counts[2]*44
    for n in range(counts[3]):
        ident=struct.unpack_from('<I',raw,frame_at+n*20)[0]
        if ident>=counts[1]:raise ValueError('Invalid frame texture')
        protected.add(ident)
    replacements={}
    for ident,blob in enumerate(blobs):
        if ident in protected or struct.unpack_from('<H',raw,40+ident*16)[0]==65535:continue
        # A draw too large for the native grid repair at 4x4 keeps its exact texture.
        if _constant(blob) and blob[:4]!=struct.pack('<HH',4,4) and texture_draws_safe(raw,ident,4,4):
            replacement=struct.pack('<HH',4,4)+blob[4:36]+bytes([0x11])*8
            verify_constant_replacement(blob,replacement)
            replacements[ident]=replacement
    return replacements


def verify_constant_replacement(original, replacement):
    """Independent semantic check: exact originalCLUT and full sentinel1 domain."""
    if not _constant(original) or not _constant(replacement):
        raise ValueError('Constant replacement changes sampled words or coverage')
    if original[4:36]!=replacement[4:36] or replacement!=struct.pack('<HH',4,4)+original[4:36]+bytes([0x11])*8:
        raise ValueError('Constant replacement changes palette/index or is not4x4')


def verify_constant_room(original, result, mapping, replaced_ids):
    """Check a constant-only repack, including all nontexture gameplay records.

    A combined approximate95% pass must verify its other replacements separately;
    this strict verifier intentionally rejects unrelated texture modifications.
    """
    old,old_prefix,_,_=layout(original);new,new_prefix,_,_=layout(result)
    if old[2:]!=new[2:] or len(mapping)!=old[1] or any(not isinstance(i,int) or not 0<=i<new[1]for i in mapping):
        raise ValueError('Room record count/mapping changed')
    changes=set(replaced_ids)
    if any(not isinstance(i,int) or not 0<=i<old[1]for i in changes):raise ValueError('Invalid replacement index')
    before=textures(original);after=textures(result)
    for i,blob in enumerate(before):
        if i in changes:verify_constant_replacement(blob,after[mapping[i]])
        elif blob!=after[mapping[i]]:raise ValueError('Unselected texture changed')
        old_stream=struct.unpack_from('<H',original,40+i*16)[0]==65535
        new_stream=struct.unpack_from('<H',result,40+mapping[i]*16)[0]==65535
        if old_stream!=new_stream or (old_stream and i in changes):raise ValueError('Texture storage class changed')
    oa=40+old[1]*16;na=40+new[1]*16
    for count,size,id_size in [(old[2],44,2),(old[3],20,4)]:
        for n in range(count):
            a=original[oa+n*size:oa+(n+1)*size];b=result[na+n*size:na+(n+1)*size]
            ident=int.from_bytes(a[:id_size],'little')
            if ident>=old[1] or int.from_bytes(b[:id_size],'little')!=mapping[ident] or a[id_size:]!=b[id_size:]:
                raise ValueError('Draw/frame geometry, material or ordering changed')
            if id_size==4 and ident in changes:raise ValueError('Frame-referenced texture changed')
        oa+=count*size;na+=count*size
    if original[oa:old_prefix]!=result[na:new_prefix]:raise ValueError('Clip/edge records changed')
