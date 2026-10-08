"""Synthetic original fixtures: no retail data in committed tests."""
import unittest,sys,struct
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from PIL import Image
from cook import Atlas, unsupported_sprite_behavior
from scene import Scene
from combat import ticks, transformed_box, generated_params

class CookTests(unittest.TestCase):
    def test_native_draw_range_rejects_near_camera_and_far_coordinates(self):
        from cook import native_draw_range
        quad=[(0,0),(1,0),(0,1),(1,1)]
        self.assertIsNone(native_draw_range(1.0,quad))
        self.assertIn('scale',native_draw_range(85.5,quad))
        self.assertIn('coordinate',native_draw_range(1.0,[(40000,0)]+quad[1:]))

    def test_configured_static_page_budget_fails_without_resampling_animation(self):
        atlas=Atlas(max_pages=1)
        for index in range(5):
            atlas.add(Image.new('RGBA',(128,128),(index*32,100,200,255)),128,128)
        with self.assertRaisesRegex(ValueError,'VRAM page budget'):atlas.pack()
        atlas=Atlas(max_pages=0)
        atlas.add(Image.new('RGBA',(61,62),(200,100,30,255)),61,62,streamed=True)
        atlas.pack()
        self.assertEqual(atlas.entries[0][3:5],(61,62))
        self.assertEqual(len(atlas.pages),0)

    def test_palette_transparency_and_vram_bounds(self):
        a=Atlas();im=Image.new('RGBA',(8,4),(0,0,0,255));im.putpixel((0,0),(0,0,0,0));a.add(im,8,4);a.pack()
        page,x,y,w,h,cl,_=a.entries[0]
        data=a.pages[page];self.assertEqual(data[y*128+x//2]&15,0)
        index=data[y*128+x//2]>>4
        self.assertNotEqual(int.from_bytes(a.palettes[cl][index*2:index*2+2],'little'),0)
        self.assertLessEqual(x+w,256);self.assertLessEqual(y+h,256)
    def test_stream_preserves_quantized_pixels_palette_and_dimensions(self):
        image=Image.new('RGBA',(5,3),(31,127,235,255))
        image.putpixel((0,0),(0,0,0,0));image.putpixel((4,2),(245,32,19,100))
        a=Atlas();a.add(image,5,3);a.pack()
        b=Atlas();b.add(image,5,3,streamed=True);b.pack()
        page,x,y,w,h,cl,_=a.entries[0]
        self.assertEqual(b.entries[0],(65535,0,0,5,3,cl,0))
        self.assertEqual(a.palettes,b.palettes);self.assertFalse(b.pages)
        self.assertEqual(len(b.stream),12)
        for yy in range(h):
            for xx in range(w):
                original=(a.pages[page][(y+yy)*128+(x+xx)//2]>>(((x+xx)&1)*4))&15
                streamed=(b.stream[yy*4+xx//2]>>((xx&1)*4))&15
                self.assertEqual(original,streamed)
            self.assertEqual(b.stream[yy*4+2]&240,0);self.assertEqual(b.stream[yy*4+3],0)
    def test_stream_limits_and_image_alignment(self):
        a=Atlas()
        with self.assertRaisesRegex(ValueError,'64x64'):
            a.add(Image.new('RGBA',(65,1)),65,1,streamed=True)
        a.add(Image.new('RGBA',(1,1)),1,1,streamed=True)
        a.add(Image.new('RGBA',(1,1),(255,255,255,255)),1,1,streamed=True);a.pack()
        self.assertEqual([e[6] for e in a.entries],[0,4]);self.assertEqual(len(a.stream),8)
        # Packing is reproducible when called again and does not append pages.
        before=(a.entries.copy(),bytes(a.stream),a.palettes.copy());a.pack()
        self.assertEqual(before,(a.entries,bytes(a.stream),a.palettes))
    def test_an_oversized_frame_binds_a_rectangle_of_slots(self):
        # Crossroads_47's Stag, whose Idle frames measure 91x89 through
        # host/cook.py's own actor art path, is the smallest real case.
        a=Atlas(max_pages=0)
        im=Image.new('RGBA',(91,89),(200,100,30,255))
        for x in range(91):
            for y in range(89):im.putpixel((x,y),((x*2)%256,(y*2)%256,60,255))
        base=a.add_tiled(im,91,89)
        self.assertEqual(base,0)
        self.assertEqual(a.grids,{0:(2,2)})
        a.pack()
        # Row-major tiles: 64+27 across by 64+25 down, each one slot at most.
        self.assertEqual([e[3:5] for e in a.entries],[(64,64),(27,64),(64,25),(27,25)])
        # The grid rides in the two fields a streamed texture never uses for a
        # VRAM origin, and only on the frame's first tile.
        self.assertEqual([e[1:3] for e in a.entries],[(2,2),(0,0),(0,0),(0,0)])
        self.assertEqual([e[0] for e in a.entries],[65535]*4)
        # One quantization for the whole frame, so the tiles hold identical
        # colours and the seams do not shift. HKROOM02 still sizes its palette
        # block at one entry per texture and the guest uploads it whole, so the
        # frame costs four CLUT slots, not one: a shared palette cannot reduce
        # that count without a format change.
        self.assertEqual([e[5] for e in a.entries],[0,1,2,3])
        self.assertEqual(len(a.palettes),4)
        self.assertEqual(len(set(a.palettes)),1)

    def test_a_sub_slot_frame_cooks_identically_through_add_and_add_tiled(self):
        # host/cook.py's actor path moved from add(streamed=True) to add_tiled,
        # so every actor whose art already fitted one slot has to cook byte for
        # byte or the change would silently rewrite cooked packs.
        im=Image.new('RGBA',(61,62))
        for x in range(61):
            for y in range(62):im.putpixel((x,y),((x*7)%256,(y*11)%256,40,255))
        one=Atlas(max_pages=0);one.add(im,61,62,streamed=True);one.pack()
        tiled=Atlas(max_pages=0);tiled.add_tiled(im,61,62);tiled.pack()
        self.assertEqual(one.entries,tiled.entries)
        self.assertEqual(one.palettes,tiled.palettes)
        self.assertEqual(bytes(one.stream),bytes(tiled.stream))
        # A one-slot frame carries no grid, so frame_grid reads (1,1) for it.
        self.assertEqual(tiled.grids,{})
        self.assertEqual(tiled.tile_owner,{})

    def test_the_refusal_boundary_is_the_slot_budget_not_sixty_four_pixels(self):
        from cook import MAX_FRAME_TILES
        self.assertEqual(MAX_FRAME_TILES,20)
        a=Atlas(max_pages=0)
        # What used to be refused outright now costs two slots.
        self.assertEqual(a.add_tiled(Image.new('RGBA',(65,1)),65,1),0)
        self.assertEqual(a.grids[0],(2,1))
        # A frame inside one slot still costs one texture and no grid.
        one=a.add_tiled(Image.new('RGBA',(64,64)),64,64)
        self.assertNotIn(one,a.grids)
        # The False Knight's largest frame, 198x169, needs twelve, and twelve
        # now fits: the cache is 24 slots since static scenery page 19 became
        # the animation cache's second region.
        base=a.add_tiled(Image.new('RGBA',(198,169)),198,169)
        self.assertEqual(a.grids[base],(4,3))
        # add() still refuses on its own: a single streamed texture is one slot.
        with self.assertRaisesRegex(ValueError,'64x64'):
            a.add(Image.new('RGBA',(65,1)),65,1,streamed=True)

    def test_the_texture_clamp_now_binds_before_the_tile_budget(self):
        # add() and add_tiled() clamp a frame to 252 pixels on each axis, so
        # the largest frame the format can express is four tiles by four. That
        # is 16 of the 20 a frame may bind, which is why the slot count stopped
        # being the thing that refuses art.
        from cook import MAX_FRAME_TILES, SLOT_PIXELS
        per_axis=-(-252//SLOT_PIXELS)
        self.assertEqual(per_axis,4)
        self.assertLess(per_axis*per_axis,MAX_FRAME_TILES)

    def test_tiles_survive_the_canonical_dedup_postpass_consecutive(self):
        from texture_dedup import _deduplicate_room
        from regions import append_room_bank
        a=Atlas(max_pages=0)
        im=Image.new('RGBA',(91,89))
        for x in range(91):
            for y in range(89):im.putpixel((x,y),((x*3)%256,(y*5)%256,90,255))
        base=a.add_tiled(im,91,89)
        # Two frames of the same clip, so a naive dedup would merge tile runs.
        second=a.add_tiled(im,91,89)
        a.pack()
        empty=bytearray(b'HKROOM02'+struct.pack('<8I',0,1,0,0,0,0,4,0))
        empty+=struct.pack('<6HI',65535,0,0,1,1,0,0)+bytes(32)+bytes(4)
        frames=[{'texture':i,'box':(0,0,1,1)} for i in (base,second)]
        pack,_nf=append_room_bank(bytes(empty),a,frames,[],0,'tile test')
        result,report=_deduplicate_room(bytes(pack))
        count=struct.unpack_from('<I',result,12)[0]
        records=[struct.unpack_from('<6HI',result,40+i*16) for i in range(count)]
        grids=[(i,r[1],r[2]) for i,r in enumerate(records) if r[1] or r[2]]
        self.assertEqual(len(grids),2)
        # Header counts: no draws, two frames, no clips or edges, so the palette
        # block follows the textures and the frames.
        at=40+count*16
        clut=at+2*20
        for start,cols,rows in grids:
            self.assertEqual((cols,rows),(2,2))
            palettes={result[clut+records[start+t][5]*32:clut+(records[start+t][5]+1)*32]
                      for t in range(cols*rows)}
            self.assertEqual(len(palettes),1,'a frame\'s tiles must keep one palette')
            for t in range(cols*rows):
                self.assertEqual(records[start+t][0],65535)
                self.assertEqual(records[start+t][1:3],(cols,rows) if t==0 else (0,0))
        # Each frame still points at its own first tile.
        for k,(start,_c,_r) in enumerate(grids):
            self.assertEqual(struct.unpack_from('<I',result,at+k*20)[0],start)

    def test_exact_quantized_duplicates_share_texture_not_draw_geometry(self):
        a=Atlas();one=Image.new('RGBA',(5,3),(9,16,24,255));two=Image.new('RGBA',(5,3),(10,17,25,255))
        self.assertEqual(a.add(one,5,3),0)
        self.assertEqual(a.add(two,5,3),0)  # Different RGBA, identical final PS1 colors.
        self.assertEqual(a.add(one,5,3,streamed=True),1)
        self.assertEqual(a.request_map,[0,0,1]);a.pack()
        self.assertEqual(len(a.entries),2)
        self.assertNotEqual(a.entries[0][0],a.entries[1][0])
        self.assertEqual(a.quantized[0],a.quantized[1])

    def test_nested_rotation_scale_and_inactive_parent(self):
        s=Scene.__new__(Scene)
        def tr(go,parent,p,scale=(1,1,1),q=(0,0,0,1)):
            return {'m_GameObject':{'m_PathID':go},'m_Father':{'m_PathID':parent},'m_LocalPosition':dict(zip('xyz',p)),'m_LocalScale':dict(zip('xyz',scale)),'m_LocalRotation':dict(zip('xyzw',q))}
        s.transforms={1:tr(10,0,(5,7,0),(2,2,1)),2:tr(20,1,(3,4,0))};s.go_transform={10:1,20:2};s.gos={10:{'m_IsActive':False},20:{'m_IsActive':True}}
        self.assertEqual(s.point(20),(11,15,0));self.assertFalse(s.active(20))
    def test_palette_limit_is_not_silent(self):
        a=Atlas()
        for i in range(641):a.add_quantized(1,1,struct.pack('<16H',0,i+1,*([0]*14)),b'\x01')
        with self.assertRaisesRegex(ValueError,'CLUT budget'):a.pack()
    def test_inverse_remasker_exclusion_is_behavior_specific(self):
        self.assertEqual(unsupported_sprite_behavior('Inverse Remasker','mask_container',['PlayMakerFSM','SpriteRenderer']),'FSM-controlled inverse remasker')
        for name,parent,types in [('rock','mask_container',['PlayMakerFSM']),('Inverse Remasker','scenery',['PlayMakerFSM']),('Inverse Remasker','mask_container',['SpriteRenderer'])]:
            self.assertIsNone(unsupported_sprite_behavior(name,parent,types))
    def test_nail_timing_and_effect_source_transform(self):
        self.assertEqual(ticks(.349999994),21)
        self.assertEqual(ticks(.409999996),25)
        self.assertEqual(ticks(.02),2)
        self.assertEqual(ticks(.1),6)
        nail={'position':{'x':1,'y':-2},'scale':{'x':2,'y':3}}
        self.assertEqual(transformed_box((-2,-1,0,1),nail),(-3,-5,1,1))
    def test_grass_draw_switch_requires_both_source_renderers(self):
        constants={'ATTACK_DURATION':.35,'ATTACK_COOLDOWN_TIME':.41,'ALT_ATTACK_RESET':.5,'ATTACK_RECOVERY_TIME':.1}
        nails=[{'polygon':[(0,0),(1,0),(0,1)]}]*4
        grass=[{'off':10,'on':20,'box':(0,0,1,1)}]
        with self.assertRaisesRegex(ValueError,'renderer omitted'):
            generated_params(constants,.02,nails,grass,[{'source':'level6:10'}])
        result=generated_params(constants,.02,nails,grass,[{'source':'level6:20'},{'source':'level6:10'}])
        self.assertIn('off_draw:1,on_draw:0',result)
        self.assertEqual(grass[0]['on_draw'],0)
        nails[0]={'polygon':[(17,0),(1,0),(0,1)]}
        with self.assertRaisesRegex(ValueError,'Q16 bound'):
            generated_params(constants,.02,nails,grass,[])
        nails[0]={'polygon':[(0,0),(1,0),(0,1)]}
        grass[0]['box']=(0,0,513,1)
        with self.assertRaisesRegex(ValueError,'Q16 bound'):
            generated_params(constants,.02,nails,grass,[])
if __name__=='__main__':unittest.main()


class SharedPaletteTests(unittest.TestCase):
    """add_frames_shared: one palette for a whole group of streamed frames."""

    def test_a_group_of_frames_spends_one_palette(self):
        from PIL import Image
        import cook
        atlas = cook.Atlas()
        red = Image.new('RGBA', (20, 30), (200, 40, 30, 255))
        grey = Image.new('RGBA', (24, 24), (120, 120, 130, 255))
        firsts = atlas.add_frames_shared([(red, 20, 30), (grey, 24, 24)])
        self.assertEqual(len(firsts), 2)
        palettes = {atlas.quantized[i][2] for i in firsts}
        self.assertEqual(len(palettes), 1)
        self.assertTrue(all(i in atlas.streamed for i in firsts))
        self.assertEqual([atlas.quantized[i][:2] for i in firsts], [(20, 30), (24, 24)])

    def test_a_frame_wider_than_one_slot_keeps_consecutive_tiles(self):
        from PIL import Image
        import cook
        atlas = cook.Atlas()
        wide = Image.new('RGBA', (100, 40), (90, 90, 90, 255))
        small = Image.new('RGBA', (10, 10), (10, 200, 10, 255))
        first, second = atlas.add_frames_shared([(wide, 100, 40), (small, 10, 10)])
        self.assertEqual(atlas.grids[first], (2, 1))
        self.assertEqual(atlas.tile_owner[first + 1], first)
        self.assertEqual(second, first + 2)
