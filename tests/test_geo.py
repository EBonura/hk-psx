"""Source-free Geo art allocation, nibble layout and authored payout guards."""
import copy,struct,sys,unittest
from pathlib import Path
from unittest.mock import patch
from PIL import Image
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
import rustsrc
from geo import art_bank,place_rectangles,VRAM_RECTS,MAX_VRAM_BYTES,fsm_contract,q16,canonical_images,rock_state,MAX_ROCKS_PER_SCENE

class GeoTests(unittest.TestCase):
    def test_fragmented_allocations_are_disjoint_aligned_and_bounded(self):
        items=[('palette',16,1,16)]+[(i,4+(i%3),8+(i%8),1)for i in range(28)]
        placed=place_rectangles(items);self.assertEqual(placed,place_rectangles(list(reversed(items))))
        seen=set()
        for key,(x,y,w,h)in placed.items():
            self.assertTrue(any(rx<=x and ry<=y and x+w<=rx+rw and y+h<=ry+rh for rx,ry,rw,rh in VRAM_RECTS))
            cells={(xx,yy)for xx in range(x,x+w)for yy in range(y,y+h)}
            self.assertFalse(seen&cells);seen|=cells
        self.assertEqual(placed['palette'][0]%16,0);self.assertLessEqual(len(seen)*2,MAX_VRAM_BYTES)
        with self.assertRaises(ValueError):place_rectangles([('oversize',65,28,1)])
    def test_art_bank_keeps_frame_mapping_padding_and_blend_classes(self):
        im=Image.new('RGBA',(7,5));im.putdata([(255,100,0,(0,128,255)[x%3])for y in range(5)for x in range(7)])
        blob,uploads,textures,mapping,_=art_bank([im,im.copy(),Image.new('RGBA',(5,6),(0,255,0,255))])
        self.assertEqual(mapping,[0,0,1]);self.assertEqual(len(textures),2)
        self.assertEqual((blob,uploads,textures,mapping),art_bank([im,im.copy(),Image.new('RGBA',(5,6),(0,255,0,255))])[:4])
        palette=struct.unpack_from('<16H',blob)
        self.assertEqual(palette[0],0);self.assertTrue(any(p&0x8000 for p in palette));self.assertTrue(any(p and not p&0x8000 for p in palette))
        first=uploads[1];stride=first['w']*2
        for y in range(5):self.assertEqual(blob[first['offset']+y*stride+3]&0xf0,0)
        for t in textures:
            self.assertEqual(t['material'],1);self.assertLessEqual(t['u']+t['w'],256);self.assertLessEqual(t['v']+t['h'],256)
        self.assertEqual(sum(u['w']*u['h']*2 for u in uploads),len(blob))
    def test_fixed_point_invalid_values_rejected(self):
        for v in [float('nan'),float('inf'),40000]:
            with self.assertRaises(ValueError):q16(v)
        self.assertEqual(q16(-.25),-16384)
    def test_shared_source_sampling_uses_maximum_axes_independent_of_order(self):
        source=Image.new('RGBA',(18,20),(120,80,30,255));box=[-1,-2,1,2]
        def records():return [{'source':'atlas:17','original_image':source.copy(),'image':source.resize(d),'box':box[:]}for d in [(8,12),(10,7),(6,6)]]
        a=records();b=list(reversed(records()));canonical_images(a);canonical_images(b)
        self.assertTrue(all(v['image'].size==(10,12) and v['box']==box for v in a+b))
        self.assertEqual([v['image'].tobytes()for v in a],[v['image'].tobytes()for v in b])
        c=records();c[1]['original_image'].putpixel((0,0),(255,0,0,255))
        with self.assertRaises(ValueError):canonical_images(c)
    def test_allocator_rejects_overlapping_reserved_rectangles(self):
        with self.assertRaises(ValueError):place_rectangles([(0,1,1,1)],[(0,0,16,16),(15,15,16,16)])
        with self.assertRaises(ValueError):place_rectangles([(0,1,1,1)],[(1020,0,16,16)])
    def fixture(self):
        val=lambda v,name='':{'value':v,'useVariable':bool(name),'name':name}
        fling={'spawnMin':val(2,'Geo Per Hit'),'spawnMax':val(2,'Geo Per Hit'),'gameObject':{'useVariable':False,'value':{'m_FileID':2,'m_PathID':9}}, **{k:val(v)for k,v in {'speedMin':23,'speedMax':30,'angleMin':80,'angleMax':100,'originVariationX':.25,'originVariationY':.25}.items()}}
        final=copy.deepcopy(fling);final['spawnMin']=final['spawnMax']=val(5,'Final Payout')
        acts={'Hit':[('FlingObjectsFromGlobalPool',fling),('IntOperator',{'integer1':val(5,'Hits'),'integer2':val(1),'storeResult':val(5,'Hits'),'operation':1})],
              'Destroy':[('FlingObjectsFromGlobalPool',final)],'Pause Frame':[],
              'Broken':[('Tk2dPlayAnimation',{'clipName':val('Broken 1')}),('SetCollider',{'active':val(False)})]}
        states=[{'name':n,'actionData':{'actionNames':[k for k,v in a],'actionEnabled':[True]*len(a),'fields':[v for k,v in a]},'transitions':[]}for n,a in acts.items()]
        states[0]['transitions']=[{'fsmEvent':{'name':'HIT'},'toState':'Pause Frame'}]
        states[2]['transitions']=[{'fsmEvent':{'name':'FINISHED'},'toState':'Destroy'}]
        fsm={'states':states,'variables':{'v':[{'name':k,'value':v}for k,v in {'Hits':5,'Geo Per Hit':2,'Final Payout':5,'Recoil Time':.1}.items()]}}
        return fsm
    def test_final_hit_includes_both_payouts_and_rejects_changed_fsm(self):
        with patch('geo.compact_fields',lambda d,i:d['fields'][i]):
            f=self.fixture();r=fsm_contract(f);self.assertEqual(r['total'],15);self.assertEqual(r['hit_cooldown'],6)
            for modify in [lambda f:f['states'][0]['transitions'][0].update(toState='Destroy'),lambda f:f['states'][0]['actionData']['fields'][1].update(operation=0),lambda f:f['states'][1]['actionData']['fields'][0]['speedMin'].update(value=22),lambda f:f['variables']['v'][0].update(value=0)]:
                f=self.fixture();modify(f)
                with self.assertRaises(ValueError):fsm_contract(f)

    def test_rock_state_is_the_index_within_its_scene(self):
        # The guest asserts state<MAX_ROCKS_PER_SCENE on every swing in the
        # scene; a catalogue-wide index crossed it at the 17th admitted rock.
        rocks=[]
        for scene in [0,0,3,0,3,7]+[9]*16:
            rocks.append({'scene':scene,'state':rock_state(rocks,scene)})
        self.assertEqual([r['state'] for r in rocks[:6]],[0,1,0,2,1,0])
        self.assertEqual([r['state'] for r in rocks if r['scene']==9],list(range(16)))
        self.assertTrue(all(r['state']<MAX_ROCKS_PER_SCENE for r in rocks))
        with self.assertRaises(ValueError):rock_state(rocks,9)
    def test_host_limit_matches_the_guest(self):
        guest=rustsrc.const_int(Path(__file__).resolve().parents[1]/'game/src/geo.rs','MAX_ROCKS_PER_SCENE')
        self.assertEqual(guest,MAX_ROCKS_PER_SCENE)

if __name__=='__main__':unittest.main()
