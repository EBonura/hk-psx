"""Admit original opaque-black tk2d tilemap meshes as exact merged cell unions.

Decorative SpriteRenderers do not replace these meshes. Admission is based on
an enabled tk2dTileMap's renderData hierarchy, source mesh triangles, opaque
sample support, and the source shader; object names never authorize a fill.
"""
import hashlib,math,struct
from pathlib import Path
from UnityPy.helpers.MeshHelper import MeshHandler
from breakables import _components,_descendants

BLACK_PALETTE=struct.pack('<16H',0,1,*([0x8000]*14))
BLACK_PIXELS=bytes([0x11]*8)


def mesh_cells(vertices,triangles,colors):
    """Prove each pair of source triangles covers precisely one unit cell."""
    if not vertices or len(colors)!=len(vertices) or any(tuple(c)!=(1.,1.,1.,1.)for c in colors):raise ValueError('tilemap vertex color is not constant white')
    if any(len(v)!=3 or v[2]!=0 or any(not math.isfinite(c)or c!=int(c)for c in v[:2])for v in vertices):raise ValueError('tilemap vertices are not planar integer coordinates')
    cells={};used=set()
    for tri in triangles:
        if len(tri)!=3 or len(set(tri))!=3 or any(type(i)is not int or not 0<=i<len(vertices)for i in tri):raise ValueError('invalid tilemap triangle index')
        used.update(tri);p=[tuple(int(v)for v in vertices[i][:2])for i in tri];xs=[v[0]for v in p];ys=[v[1]for v in p]
        if max(xs)-min(xs)!=1 or max(ys)-min(ys)!=1:raise ValueError('tilemap triangle is not a unit-cell half')
        area=(p[1][0]-p[0][0])*(p[2][1]-p[0][1])-(p[1][1]-p[0][1])*(p[2][0]-p[0][0])
        if abs(area)!=1:raise ValueError('tilemap half-cell area')
        cells.setdefault((min(xs),min(ys)),[]).append(set(p))
    if used!=set(range(len(vertices))):raise ValueError('unreferenced tilemap vertices')
    for (x,y),halves in cells.items():
        if len(halves)!=2:raise ValueError('missing or duplicate tilemap cell triangle')
        common=halves[0]&halves[1]
        if len(common)!=2 or halves[0]|halves[1]!={(x,y),(x+1,y),(x,y+1),(x+1,y+1)}:raise ValueError('tilemap cell coverage')
        a,b=tuple(common)
        if abs(a[0]-b[0])!=1 or abs(a[1]-b[1])!=1:raise ValueError('tilemap triangles overlap instead of sharing a diagonal')
    return set(cells)


def merge_cells(cells):
    """Lossless disjoint rectangle cover; never fill an absent source cell."""
    left=set(cells);rects=[]
    while left:
        x,y=min(left,key=lambda p:(p[1],p[0]));w=1
        while(x+w,y)in left:w+=1
        h=1
        while all((xx,y+h)in left for xx in range(x,x+w)):h+=1
        rects.append((x,y,x+w,y+h));left.difference_update((xx,yy)for yy in range(y,y+h)for xx in range(x,x+w))
    restored=set()
    for x0,y0,x1,y1 in rects:
        block={(x,y)for y in range(y0,y1)for x in range(x0,x1)}
        if restored&block:raise ValueError('overlapping merged tilemap rectangles')
        restored|=block
    if restored!=set(cells):raise ValueError('merged tilemap coverage differs from source')
    return rects


def opaque_sample_support(image,uvs):
    """Conservatively include every bilinear tap in the complete source UV box."""
    if not uvs or any(len(uv)!=2 or not all(math.isfinite(v)and 0<=v<=1 for v in uv)for uv in uvs):raise ValueError('tilemap UV range')
    w,h=image.size;u0=min(uv[0]for uv in uvs);u1=max(uv[0]for uv in uvs);v0=min(uv[1]for uv in uvs);v1=max(uv[1]for uv in uvs)
    # Unity image rows run downward; UV V runs upward. Include a full extra
    # texel on either side, stronger than the half-texel bilinear footprint.
    bounds=(max(0,math.floor(u0*w)-1),max(0,h-math.ceil(v1*h)-1),min(w,math.ceil(u1*w)+1),min(h,h-math.floor(v0*h)+1))
    crop=image.convert('RGBA').crop(bounds)
    if not crop.width or not crop.height or any(p!=(0,0,0,255)for p in crop.get_flattened_data()):raise ValueError('tilemap sample support is not fully opaque black')
    return bounds


def black_material(source,file,ref,uvs):
    material=source.ref(file,ref);m=source.read(material);shader=source.ref(material.assets_file,m['m_Shader']);sh=source.read(shader);parsed=sh.get('m_ParsedForm',{})
    if parsed.get('m_Name')!='tk2d/BlendVertexColor':raise ValueError('unsupported tilemap shader')
    passes=[p for sub in parsed.get('m_SubShaders',[])for p in sub.get('m_Passes',[])]
    expected={'srcBlend':5,'destBlend':10,'srcBlendAlpha':5,'destBlendAlpha':10,'blendOp':0,'blendOpAlpha':0,'colMask':15}
    if not passes or any(any(p['m_State']['rtBlend0'][k]['val']!=v for k,v in expected.items())for p in passes):raise ValueError('unsupported tilemap blend state')
    saved=m['m_SavedProperties'];envs=dict(saved['m_TexEnvs'])
    if set(envs)!={'_MainTex'} or saved['m_Colors'] or saved['m_Floats']:raise ValueError('unsupported tilemap material properties')
    env=envs['_MainTex']
    if env['m_Scale']!={'x':1.,'y':1.}or env['m_Offset']!={'x':0.,'y':0.}:raise ValueError('tilemap texture transform')
    tex=source.ref(material.assets_file,env['m_Texture']);im=tex.read().image.convert('RGBA');bounds=opaque_sample_support(im,uvs)
    return {'source':source.sid(material),'shader':source.sid(shader),'shader_name':parsed['m_Name'],'texture_source':source.sid(tex),'sample_bounds':bounds,
            'sample_rgba':[0,0,0,255],'texture_rgba_sha256':hashlib.sha256(im.tobytes()).hexdigest(),'mode':1,'supported':True,
            'approximation':'Exact constant opaque black; PS1 sentinel palette index1 with renderer red127 correction.'}


def tilemap_fill_sources(sc):
    """Scene-wide extraction cached by caller; unsupported active meshes fail."""
    source=sc.source;roots={};meshes=[]
    for ident,(kind,t)in sc.objects.items():
        if kind!='tk2dTileMap' or not t['m_Enabled']:continue
        owner=t['m_GameObject']['m_PathID']
        if not sc.active(owner):continue
        ref=t['renderData'];obj=source.ref(sc.file,ref)
        if obj.type.name!='GameObject':raise ValueError('tk2d renderData is not a scene GameObject')
        for gid in _descendants(sc,obj.path_id):
            # Two tilemaps may share a render root (Godhome arenas); each mesh
            # renderer is still extracted once, attributed to the first owner.
            roots.setdefault(gid,f'{Path(sc.file.name).name}:{ident}')
    for ident,(kind,t)in sc.objects.items():
        if kind!='MeshRenderer' or not t['m_Enabled']:continue
        gid=t['m_GameObject']['m_PathID']
        if gid not in roots or not sc.active(gid):continue
        filters=[c for _,k,c in _components(sc,gid)if k=='MeshFilter']
        if len(filters)!=1 or len(t['m_Materials'])!=1:raise ValueError('tilemap mesh/material count')
        obj=source.ref(sc.file,filters[0]['m_Mesh']);mesh=obj.read();reader=MeshHandler(mesh);reader.process();groups=reader.get_triangles()
        if len(groups)!=1:raise ValueError('tilemap mesh submesh count')
        cells=mesh_cells(reader.m_Vertices,groups[0],reader.m_Colors)
        if len(reader.m_UV0)!=len(reader.m_Vertices):raise ValueError('tilemap UV count')
        material=black_material(source,sc.file,t['m_Materials'][0],reader.m_UV0);rects=merge_cells(cells)
        points=[[sc.point(gid,x,y)for x,y in [(x0,y1),(x1,y1),(x0,y0),(x1,y0)]]for x0,y0,x1,y1 in rects]
        if any(any(p[2]!=ps[0][2]for p in ps)for ps in points):raise ValueError('nonplanar transformed tilemap fill')
        meshes.append({'source':f'{Path(sc.file.name).name}:{ident}','tilemap_source':roots[gid],'mesh_source':source.sid(obj),
            'mesh_sha256':hashlib.sha256(obj.get_raw_data()).hexdigest(),'name':sc.gos[gid]['m_Name'],'material':material,'cells':sorted(cells),
            'rectangles':rects,'points':points,'layer':t['m_SortingLayer'],'order':t['m_SortingOrder'],
            'source_triangle_count':len(groups[0]),'cell_count':len(cells),'rectangle_count':len(rects)})
    return meshes


def append_tilemap_fills(sc,atlas,draws,region,cam_x,cam_y,focal,cam_z):
    if not hasattr(sc,'_tilemap_fills'):sc._tilemap_fills=tilemap_fill_sources(sc)
    texture=None;admitted=[]
    for mesh in sc._tilemap_fills:
        for index,points in enumerate(mesh['points']):
            z=points[0][2]
            if z<=cam_z+2:raise ValueError('tilemap fill crosses camera near plane')
            scale=focal/(z-cam_z);xs=[p[0]for p in points];ys=[p[1]for p in points]
            if max(xs)<cam_x[0]-160/scale or min(xs)>cam_x[1]+160/scale or max(ys)<cam_y[0]-120/scale or min(ys)>cam_y[1]+120/scale:continue
            if texture is None:texture=atlas.add_quantized(4,4,BLACK_PALETTE,BLACK_PIXELS)
            draws.append({'source':mesh['source'],'sprite':mesh['material']['texture_source'],'name':mesh['name'],'texture':texture,
                'points':points,'scale':scale,'tint':[128,128,128],'z':z,'layer':mesh['layer'],'order':mesh['order'],
                'material':mesh['material'],'tilemap_rect':index,'tilemap_mesh':mesh['mesh_source']})
            admitted.append((mesh['source'],index))
    return {'mesh_count':len(sc._tilemap_fills),'source_cells':sum(m['cell_count']for m in sc._tilemap_fills),
            'source_triangles':sum(m['source_triangle_count']for m in sc._tilemap_fills),'merged_rectangles':sum(m['rectangle_count']for m in sc._tilemap_fills),
            'region_draws':len(admitted),'admitted':admitted,'meshes':sc._tilemap_fills}
