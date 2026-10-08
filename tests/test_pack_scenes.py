"""Resident scene allocation and O(1) region mapping contracts, source-free."""
import hashlib,json,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from pack_scenes import allocate_scenes,manifest,decoder_command,pack_scenes,validate_geometry,atlas_ranges

def entry(scene,size,pages,palettes,chunks):
    return dict(scene_id=scene,bytes=size,stored_bytes=size//2,pages=pages,palettes=palettes,
                stored_fnv=13,raw_fnv=17,textures=65,stored_path=f'scene{scene}.hlzc',raw_path=f'scene{scene}.hk',source_rooms=[{'chunk_id':c,'counts':[1,65,33,1,1,1]}for c in chunks])
def atlases_for(scenes):
    out=[]
    for scene in scenes:
        for kind,base,count in ((0,scene['page_base'],scene['pages']),(1,scene['palette_base'],scene['palettes'])):
            for first in range(base,base+count,1 if kind==0 else 1024):
                n=min(1 if kind==0 else 1024,base+count-first);index=len(out)
                out.append(dict(scene_index=scene['scene_index'],stored_len=120,stored_fnv=7,
                                raw_len=n*(32768 if kind==0 else 32),raw_fnv=9,kind=kind,first=first,count=n,
                                stored_path=f'atlas{index}.hlzc',raw_path=f'atlas{index}.raw'))
    return out

class PackScenes(unittest.TestCase):
    def test_largest_scene_uses_full_arena_then_immutable_aligned_prefix(self):
        out=allocate_scenes([entry(1,116756,1,141,[2]),entry(0,929692,18,1065,[1,3])],arena_bytes=1069056)
        self.assertEqual([e['scene_id']for e in out],[0,1])
        self.assertEqual([(e['ram_offset'],e['arena_capacity'])for e in out],[(0,1069056),(929692,139364)])
        self.assertEqual([(e['page_base'],e['palette_base'])for e in out],[(0,0),(18,1065)])
        self.assertEqual([e['chunk_id']for e in out],[1,2])
        odd=allocate_scenes([entry(0,1001,1,1,[1]),entry(1,8,1,1,[2])],arena_bytes=1024)
        self.assertEqual(odd[1]['ram_offset'],1004)
    def test_the_page_reservation_is_nineteen_since_one_became_animation_slots(self):
        """The twentieth 256x256 4bpp page is the animation cache's second
        region; see hk_cache::residency::STATIC_PAGES. Measured over all 45
        cooked scenes, the largest is Tutorial_01 at 18 pages and the next is
        Crossroads_ShamanTemple at 9, so the shipped scene_gate plan is
        unaffected. What no longer fits is a joint plan that wanted twenty.
        """
        from pack_scenes import MAX_PAGES
        self.assertEqual(MAX_PAGES,19)
        gate=allocate_scenes([entry(0,929692,18,1065,[1]),entry(1,116756,9,141,[2])],
                             arena_bytes=1069056,residency='scene_gate')
        self.assertEqual([e['page_base']for e in gate],[0,0])
        with self.assertRaisesRegex(ValueError,'page'):
            allocate_scenes([entry(0,929692,18,1065,[1]),entry(1,116756,2,141,[2])],
                            arena_bytes=1069056)

    def test_compact_allocation_and_manifest_keep_full_host_provenance(self):
        a=entry(0,1000000,18,1000,[1]);a.update(resident_raw_len=300000,resident_stored_len=150000,resident_raw_fnv=31,resident_stored_fnv=41,
            resident_raw_path='compact.hk',resident_stored_path='compact.hlzc',texture_flags=[3,0,0,0])
        b=entry(1,100000,1,100,[2]);b.update(resident_raw_len=40000,resident_stored_len=20000,resident_raw_fnv=51,resident_stored_fnv=61)
        scenes=allocate_scenes([a,b],arena_bytes=350000)
        self.assertEqual(scenes[0]['raw_len'],1000000)
        self.assertEqual(scenes[1]['ram_offset'],300000)
        atlases=atlases_for(scenes)
        r=dict(scenes=scenes,arena_bytes=350000,raw_resident_bytes=340000,total_pages=19,total_palettes=1100,atlases=atlases)
        m={'regions':[{'chunk_id':1,'scene_id':0},{'chunk_id':2,'scene_id':1}]}
        text=manifest(r,m)
        self.assertIn('stored_len:150000,stored_fnv:41,raw_len:300000,raw_fnv:31',text)
        # Texture flags ride in the resident bank now, not the manifest.
        self.assertNotIn('SCENE_TEXTURE_FLAGS',text)
        self.assertIn('AtlasDesc{scene_index:0,stored_len:120,stored_fnv:7,raw_len:32768,raw_fnv:9,kind:0,first:0,count:1}',text)
        command=decoder_command(r)
        at=command.index('--atlases')
        self.assertEqual(command[at-4:at],['compact.hlzc','compact.hk','scene1.hlzc','scene1.hk'])
        self.assertEqual(command[at+1:],sum(([a['stored_path'],a['raw_path']]for a in atlases),[]))
        self.assertIn('SCENE_GATE_LOAD:bool=false;',text)

    def test_manifest_binds_checked_world_metadata_identity_and_chunk(self):
        a=entry(0,1000,1,1,[1]);a.update(resident_raw_len=900,resident_stored_len=450,
            resident_raw_fnv=31,resident_stored_fnv=41)
        scenes=allocate_scenes([a],arena_bytes=2000,residency='scene_gate')
        r=dict(scenes=scenes,arena_bytes=2000,raw_resident_bytes=900,total_pages=1,
               total_palettes=1,atlases=atlases_for(scenes),world_metadata=[dict(
                   scene_id=0,raw_len=160,raw_fnv=17,bank_fnv=19,stored_len=80,
                   stored_fnv=23,chunk_id=9,fingerprint='00'*32)])
        text=manifest(r,{'regions':[{'chunk_id':1,'scene_id':0}]})
        self.assertIn('WORLD_META_ARENA_BYTES:usize=160;',text)
        self.assertIn('WorldMetaDesc{scene_id:0,raw_len:160,raw_fnv:17,bank_fnv:19,stored_len:80,stored_fnv:23,chunk_id:9,fingerprint:[0,0,0',text)
    def test_aggregate_limits_reject_individually_valid_banks(self):
        for a,b,arena in [(entry(0,600,10,10,[1]),entry(1,600,10,10,[2]),1000),
                          (entry(0,400,11,10,[1]),entry(1,400,10,10,[2]),1000),
                          (entry(0,400,10,900,[1]),entry(1,400,10,800,[2]),1000)]:
            with self.assertRaises(ValueError):allocate_scenes([a,b],arena)
        with self.assertRaises(ValueError):allocate_scenes([])
        with self.assertRaises(ValueError):allocate_scenes([entry(0,100,1,1,[1])]*2)
    def test_scene_gate_allocation_reuses_memory_without_joint_limit(self):
        a=entry(0,320000,18,1068,[1]);b=entry(1,89000,6,408,[2])
        out=allocate_scenes([a,b],arena_bytes=369180,residency='scene_gate')
        self.assertEqual([(e['ram_offset'],e['page_base'],e['palette_base'])for e in out],[(0,0,0)]*2)
        self.assertEqual([e['arena_capacity']for e in out],[369180]*2)
        with self.assertRaises(ValueError):allocate_scenes([a,b],arena_bytes=369180)
        for bad in [dict(a,pages=21),dict(a,palettes=1249),dict(a,bytes=400000)]:
            with self.assertRaises(ValueError):allocate_scenes([bad,b],residency='scene_gate')
        with self.assertRaises(ValueError):allocate_scenes([a],residency='invented')
    def test_scene_gate_decoder_routes_only_matching_atlases(self):
        a=entry(0,320000,18,1068,[1]);b=entry(1,89000,6,408,[2])
        for e in (a,b):
            e.update(resident_stored_path='s'+str(e['scene_id']),resident_raw_path='r'+str(e['scene_id']))
        scenes=allocate_scenes([a,b],residency='scene_gate')
        atlases=[dict(scene_index=i,kind=0,first=0,count=1,stored_path='a'+str(i),raw_path='b'+str(i))for i in range(2)]
        cmd=decoder_command(dict(residency='scene_gate',arena_bytes=369180,scenes=scenes,atlases=atlases))
        self.assertIn('check_scene_gate_load',cmd)
        self.assertEqual(cmd[cmd.index('--scene'):],['--scene','0','s0','r0','--atlas','0','0','1','a0','b0',
                       '--scene','1','s1','r1','--atlas','0','0','1','a1','b1'])
    def test_scene_gate_publishes_real_manifest_and_runs_actual_decoder(self):
        sys.path.insert(0,str(Path(__file__).resolve().parent))
        from test_scene_bank import source_room
        with tempfile.TemporaryDirectory()as d:
            d=Path(d);regions=[]
            for scene in range(2):
                src=d/f'room{scene}.hk';src.write_bytes(source_room(bool(scene)))
                regions.append(dict(scene_id=scene,chunk_id=scene+1,path=str(src),
                                    sha256=hashlib.sha256(src.read_bytes()).hexdigest(),camera_bounds=[0,0,1,1]))
            meta=d/'regions.json';meta.write_text(json.dumps({'complete':True,'regions':regions}))
            with patch('world.generate')as generate:
                report=pack_scenes(meta,d/'banks',d/'report.json',d/'manifest.rs',residency='scene_gate')
                generate.assert_called_once()
            text=(d/'manifest.rs').read_text()
            self.assertNotIn('compile_error!',text)
            self.assertIn('SCENE_GATE_LOAD:bool=true;',text)
            self.assertEqual(report['sequential_decoder_validation']['status'],'PASS')
            self.assertEqual(report['total_pages'],max(e['pages']for e in report['scenes']))
            self.assertEqual(report['total_palettes'],max(e['palettes']for e in report['scenes']))
            self.assertEqual(report['required_resident_bytes'],max(e['resident_raw_len']for e in report['scenes']))
            self.assertEqual([(e['ram_offset'],e['page_base'],e['palette_base'])for e in report['scenes']],[(0,0,0)]*2)
            ranges=atlas_ranges(report['scenes'],report['atlases'])
            self.assertIn('SCENE_ATLAS_RANGES:&[(usize,usize)]=&['+','.join(f'({a},{b})'for a,b in ranges)+'];',text)
            for owner,(start,end)in enumerate(ranges):
                self.assertTrue(all(a['scene_index']==owner for a in report['atlases'][start:end]))

    def test_atlas_ownership_requires_contiguous_complete_scene_ranges(self):
        for residency in ('joint','scene_gate'):
            scenes=allocate_scenes([entry(0,600,2,1100,[1]),entry(1,400,1,100,[2])],residency=residency)
            good=atlases_for(scenes)
            self.assertEqual(atlas_ranges(scenes,good),[(0,4),(4,6)])
            cases=[good[:-1],good+[dict(good[0])],good[4:]+good[:4]]
            for field,value in [('scene_index',9),('kind',2),('raw_len',32),('count',0),('count',3),('first',1)]:
                bad=[dict(a)for a in good];bad[0][field]=value;cases.append(bad)
            for bad in cases:
                with self.subTest(residency=residency,atlases=bad),self.assertRaises(ValueError):atlas_ranges(scenes,bad)

    def test_gate_manifest_rejects_joint_totals_or_nonzero_origins(self):
        scenes=allocate_scenes([entry(0,600,2,1100,[1]),entry(1,400,1,100,[2])],residency='scene_gate')
        report=dict(residency='scene_gate',scenes=scenes,atlases=atlases_for(scenes),arena_bytes=369180,
                    total_pages=2,total_palettes=1100,raw_resident_bytes=1000)
        metadata={'regions':[{'chunk_id':1,'scene_id':0},{'chunk_id':2,'scene_id':1}]}
        self.assertIn('SCENE_TOTAL_PAGES:usize=2;',manifest(report,metadata))
        for key,value in [('total_pages',3),('total_palettes',1200)]:
            with self.subTest(key=key),self.assertRaises(ValueError):manifest(dict(report,**{key:value}),metadata)
        for field in ('ram_offset','page_base','palette_base'):
            changed=[dict(e)for e in scenes];changed[1][field]=1
            bad=dict(report,scenes=changed,atlases=atlases_for(changed))
            with self.subTest(field=field),self.assertRaises(ValueError):manifest(bad,metadata)

    def test_build_guest_explicitly_selects_exclusive_production_packing(self):
        import build_guest
        with tempfile.TemporaryDirectory()as d:
            d=Path(d);(d/'data').mkdir();(d/'.hkpsx').mkdir()
            (d/'data/regions.json').write_text(json.dumps({'complete':True}))
            (d/'.hkpsx/packed-scenes.json').write_text(json.dumps({'scenes':[]}))
            with patch.object(build_guest,'ROOT',d),patch.object(build_guest,'run')as run:
                self.assertEqual(build_guest.scene_manifest(),[])
                self.assertEqual(run.call_args.args[0][-2:],['--residency','scene_gate'])
                build_guest.scene_manifest('joint')
                self.assertEqual(run.call_args.args[0][-2:],['--residency','joint'])

    def test_region_mapping_uses_source_local_order_not_contiguous_scene_assumption(self):
        scenes=allocate_scenes([entry(1,400,1,1,[2]),entry(0,800,2,3,[1,3])])
        scenes[0]['black_cores']=[[0,0,8,8]]
        scenes[0]['opaque_cores']=[[0,0,16,8]]
        r={'scenes':scenes,'arena_bytes':1179648,'raw_resident_bytes':1200,'total_pages':3,'total_palettes':4,'atlases':atlases_for(scenes)}
        m={'regions':[{'chunk_id':1,'scene_id':0},{'chunk_id':2,'scene_id':1},{'chunk_id':3,'scene_id':0}]}
        text=manifest(r,m);self.assertIn('(0,0),\n(1,0),\n(0,1),',text);self.assertIn('SCENE_REGIONS:&[usize]=&[0,1]',text)
        self.assertIn('SCENE_TEXTURE_CAPACITY:usize=96;',text)
        self.assertIn('SCENE_DRAW_CAPACITY:usize=64;',text)
        self.assertNotIn('SCENE_BLACK_CORES',text);self.assertNotIn('SCENE_OPAQUE_CORES',text)
        command=decoder_command(r);at=command.index('--atlases');self.assertEqual(command[at-5:at],['1179648','scene0.hlzc','scene0.hk','scene1.hlzc','scene1.hk'])
        m['regions'][1]['scene_id']=0
        with self.assertRaises(ValueError):manifest(r,m)
    def test_serialized_scene_draws_receive_packet_bound_checks(self):
        sys.path.insert(0,str(Path(__file__).resolve().parent))
        from test_scene_bank import source_room
        from scene_bank import build_scene
        bank,entry=build_scene(0,[(1,source_room())],share_pixel_planes=True)
        metadata={'regions':[{'chunk_id':1,'camera_bounds':[0,0,1,1]}]}
        proof=validate_geometry(bank,entry,metadata)
        self.assertEqual(proof['regions'][0]['mandatory_packets'],1)
        # Serialize a too-large quad directly into the otherwise valid Scene.
        import struct
        damaged=bytearray(bank);at=entry['sections']['draws']['offset']
        struct.pack_into('<8i',damaged,at+8,0,0,20000*256,0,0,20000*256,20000*256,20000*256)
        with self.assertRaises(ValueError):validate_geometry(damaged,entry,metadata)
    def test_stale_source_rejected_before_output_publication(self):
        with tempfile.TemporaryDirectory()as d:
            d=Path(d);src=d/'room.hk';src.write_bytes(b'stale')
            meta=d/'regions.json';meta.write_text(json.dumps({'complete':True,'regions':[{'scene_id':0,'chunk_id':1,'path':str(src),'sha256':hashlib.sha256(b'expected').hexdigest()}]}))
            with patch('world.generate'):
                with self.assertRaises(ValueError):pack_scenes(meta,d/'banks',d/'report.json',d/'manifest.rs')
            self.assertFalse((d/'banks').exists());self.assertFalse((d/'manifest.rs').exists())
if __name__=='__main__':unittest.main()
