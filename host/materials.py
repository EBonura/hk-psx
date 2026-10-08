"""Bounded source material admission; never infer black masks from object names."""
import struct

ADDITIVE=0
BLACK_AVERAGE=1

def binary_black_palette(palette):
    if len(palette)!=32:return False
    words=struct.unpack('<16H',palette)
    return words[0]==0 and all(w in (0,1,0x8000) for w in words) and 1 in words and 0x8000 in words

def black_scenery_palette(palette):
    # Static source-over admission includes fully soft and fully opaque black.
    # The stricter binary-mask palette contract remains separate for fade binds.
    if len(palette)!=32:return False
    words=struct.unpack('<16H',palette)
    return words[0]==0 and any(words) and all(w in (0,1,0x8000) for w in words)

def premultiplied_sprite(material,shader):
    parsed=shader.get('m_ParsedForm',{})
    if parsed.get('m_Name') not in ('Sprites/Default','Sprites/Lit'):return False
    passes=[p for sub in parsed.get('m_SubShaders',[]) for p in sub.get('m_Passes',[])]
    if not passes:return False
    expected={'srcBlend':1,'destBlend':10,'srcBlendAlpha':1,'destBlendAlpha':10,'blendOp':0,'blendOpAlpha':0,'colMask':15}
    if any(any(p.get('m_State',{}).get('rtBlend0',{}).get(k,{}).get('val')!=v for k,v in expected.items()) for p in passes):return False
    saved=material.get('m_SavedProperties',{});floats=dict(saved.get('m_Floats',[]));colors=dict(saved.get('m_Colors',[]));textures=dict(saved.get('m_TexEnvs',[]))
    if floats.get('_EnableExternalAlpha',0)!=0:return False
    if any(textures.get(key,{}).get('m_Texture',{}).get('m_PathID',0) for key in ('_AlphaTex','_MainTex')):return False
    for name in ('_Color','_RendererColor'):
        if name in colors and any(colors[name][k]!=1 for k in 'rgba'):return False
    if any(colors.get('_EmissionColor',{}).get(k,0)!=0 for k in 'rgb'):return False
    return True

def black_pixels(image,palette):
    # Check the actual cooked input, including renderer alpha and resizing.
    # The palette contract alone could also describe unrelated additive art.
    data=list(image.get_flattened_data() if hasattr(image,'get_flattened_data') else image.getdata())
    return black_scenery_palette(palette) and any(a>=16 for r,g,b,a in data) and all(a<16 or (r==g==b==0) for r,g,b,a in data)

def scenery_material(source,file,renderer,image,palette):
    if not black_pixels(image,palette):return None
    refs=renderer['m_Materials']
    if len(refs)!=1:return None
    obj=source.ref(file,refs[0]);key=source.sid(obj)
    if not hasattr(source,'_black_materials'):source._black_materials={}
    if key not in source._black_materials:
        material=source.read(obj);shader_obj=source.ref(obj.assets_file,material['m_Shader']);shader=source.read(shader_obj)
        source._black_materials[key]={'source':key,'shader':source.sid(shader_obj),'shader_name':shader.get('m_ParsedForm',{}).get('m_Name'),'supported':premultiplied_sprite(material,shader)}
    record=source._black_materials[key]
    if not record['supported']:return None
    return dict(record,mode=BLACK_AVERAGE,approximation='Opaque cores unchanged; soft black uses PS1 B/2 instead of source B*(1-alpha).')

# Ordered coverage is spatial, never a random or frame-dependent fade.
_BAYER4=((0,8,2,10),(12,4,14,6),(3,11,1,9),(15,7,13,5))

def alpha_coverage(alpha,opacity128,x,y):
    """Source alpha times lifetime alpha, quantized spatially to0,1/2,1."""
    if not 0<=alpha<=255 or not 0<=opacity128<=128:raise ValueError('alpha range')
    numerator=alpha*opacity128*2;denominator=255*128
    low,remainder=divmod(numerator,denominator)
    return min(2,low+int(remainder*32>denominator*(2*_BAYER4[y&3][x&3]+1)))

def quantize_alpha_coverage(image,opacity128):
    """4bpp source-over approximation for explicitly admitted colored sprites.

    Three opacity classes share15 colors; each class keeps straight sourceRGB.
    The GPU Average operation supplies the half-coverage premultiplication.
    Lifetime variants must be selected as complete textures, not RGB dimming.
    """
    from PIL import Image
    if not 0<=opacity128<=128:raise ValueError('alpha range')
    image=image.convert('RGBA');w,h=image.size
    if not (1<=w<=256 and 1<=h<=256):raise ValueError('4bpp texture dimensions')
    rgba=list(image.get_flattened_data() if hasattr(image,'get_flattened_data') else image.getdata())
    levels=[alpha_coverage(a,opacity128,i%w,i//w) for i,(_,_,_,a) in enumerate(rgba)]
    groups={level:[i for i,v in enumerate(levels)if v==level] for level in (1,2)}
    if groups[1] and groups[2]:
        half=max(1,min(14,round(15*len(groups[1])/(len(groups[1])+len(groups[2])))));budgets={1:half,2:15-half}
    else:budgets={1:15,2:15}
    palette=[0];indices=[0]*(w*h)
    for level in (1,2):
        positions=groups[level]
        if not positions:continue
        rgb=Image.new('RGB',(len(positions),1));rgb.putdata([rgba[i][:3]for i in positions])
        q=rgb.quantize(colors=budgets[level],method=Image.Quantize.MEDIANCUT,dither=Image.Dither.NONE)
        colors=q.getpalette();mapping={}
        for original in sorted(set(q.tobytes())):
            r,g,b=colors[original*3:original*3+3];word=(r>>3)|((g>>3)<<5)|((b>>3)<<10)
            # ExactRGB0 is transparent on PS1, so opaque black keeps the usual
            # one-bit sentinel. This generic colored path cannot suppress its
            # red bit without also altering legitimate very dark source reds.
            mapping[original]=len(palette);palette.append((word|0x8000)if level==1 else(word or 1))
        for position,index in zip(positions,q.tobytes()):indices[position]=mapping[index]
    if len(palette)>16:raise ValueError('4bpp palette budget')
    packed=bytearray(((w+1)//2)*h)
    for i,index in enumerate(indices):packed[(i//w)*((w+1)//2)+(i%w)//2]|=index<<((i%w&1)*4)
    return w,h,struct.pack('<16H',*(palette+[0]*(16-len(palette)))),bytes(packed)
