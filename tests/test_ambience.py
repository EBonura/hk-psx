"""Synthetic ambience bank/loop checks; no retail fixture samples."""
import copy,hashlib,json,pathlib,sys,tempfile,unittest
from unittest.mock import patch
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/'host'))
import ambience

class AmbienceTests(unittest.TestCase):
    def setUp(self):
        # The fixture models three scenes regardless of the live catalog size.
        patcher=patch.object(ambience,'SCENE_FILES',('level6','level7','level37'));patcher.start();self.addCleanup(patcher.stop)
        # Three shapes cannot cover eight channels with every pair's union
        # inside five voices, so the fixture prices against six.
        pool=patch.object(ambience,'POOL_VOICES',tuple(range(7,13)));pool.start();self.addCleanup(pool.stop)
    def test_loop_flags_preserve_every_encoded_sample_nibble(self):
        for blocks in [1,2,17]:
            original=bytes([12,0]+[0x12]*14)+bytes([0x2a,0]+[0x34]*14)*(blocks-1)
            loop=ambience.loop_payload(original)
            ambience.validate_loop(loop)
            self.assertEqual(len(loop),len(original))
            for i,(a,b)in enumerate(zip(original,loop)):
                if i%16!=1:self.assertEqual(a,b)
            self.assertEqual(loop[1],7 if blocks==1 else 4)
            self.assertEqual(loop[-15],7 if blocks==1 else 3)
            for i in [1,len(loop)-15]:
                bad=bytearray(loop);bad[i]=0
                with self.assertRaises(ValueError):ambience.validate_loop(bad)
        self.assertEqual(ambience.fnv(b'hello'),0x4f9f2cab)

    def test_invalid_block_headers_and_input_flags_fail_closed(self):
        for data in [b'',bytes(15),bytes([0x10])+bytes(15),bytes([13])+bytes(15),
                     bytes(16)+bytes([0x50])+bytes(15),bytes([0,1])+bytes(14)]:
            with self.assertRaises(ValueError):ambience.loop_payload(data)

    def fixture(self,root):
        clips=[];entries={};snapshots={};scenes=[]
        for i,ch in enumerate(ambience.CHANNELS):
            data=bytes([12,0])+bytes([i]*14);path=root/'.hkpsx'/f'{i}.adpcm';path.parent.mkdir(exist_ok=True);path.write_bytes(data)
            # Each channel's own cook rate, the same way assemble asks for it.
            # This used to read "the streamed channel's rate, else RATE", which
            # was the answer rather than the rule and went wrong the moment a
            # resident channel that does not stream was cooked below RATE.
            rate=ambience.atmos_rate(ch)
            clips.append({'source':str(ch),'name':str(ch),'rate':rate,'channels':1,'frames':27,'padding_samples':1,
                'spu_pitch':round(rate*4096/44100),
                'planes':[{'path':str(path.relative_to(root)),'bytes':16,'sha256':hashlib.sha256(data).hexdigest()}]})
            entries[ch]={'channel':ch,'clip':str(ch),'loop':True,'pitch':1,'volume':1,
                'snapshot':{'internal_volume_db':0,'effects':[],'chain':[{'mute':False,'solo':False,'pitch':1}]}}
        # Pick the enabled sets out of the resident channels by position rather
        # than by number, so a swap in RESIDENT_ATMOS_CHANNELS moves the fixture
        # with it. Hardcoding the numbers made four of these raise KeyError the
        # first time the set changed, and a fixed pair of shapes priced the pool
        # at six stems the first time the set grew past the pool's width.
        #
        # Three disjoint shapes, because `assemble` refuses a report that leaves
        # a resident channel unenabled everywhere and two shapes cannot both
        # cover eight slots and keep any pair of them inside the pool. Dealt
        # round robin so each pair's union is as even as the set allows, which
        # is what `pool_pressure` measures and why this fixture has more than
        # one shape at all.
        pooled=list(range(len(ambience.CHANNELS)))
        shapes=[sorted(pooled[i::3])for i in range(3)]
        self.slots=dict(zip(('level6','level7','level37'),shapes))
        for file,slots in self.slots.items():
            enabled=[ambience.CHANNELS[s]for s in slots]
            snapshots[file]=[{'internal_volume_db':0 if ch in enabled else -80}for ch in ambience.CHANNELS]
            scenes.append({'scene_file':file,'managers':[{'source':file+':1','atmos_cue':{'name':file},
                'ambience':[entries[ch]for ch in enabled]}]})
        self.snapshots=snapshots
        # The resident clip table, which the cook reads off the AudioManager
        # rather than off the scenes: every resident channel has a row here
        # whether or not any scene enables it.
        resident=[{'channel':ch,'clip':str(ch),'loop':True,'pitch':1,'volume':1}for ch in ambience.CHANNELS]
        return {'clips':clips,'scenes':scenes,'resident_atmos':resident},snapshots

    def mask(self,file):
        """The cue mask the fixture's enabled slots have to produce."""
        return sum(1<<s for s in self.slots[file])

    def assemble(self,report,root,sfx_end=None,ceiling=ambience.SPU_END,edges=()):
        return ambience.assemble(report,root,{'level6':.5,'level7':.5,'level37':.5},
            self.snapshots,ambience.SFX_END if sfx_end is None else sfx_end,ceiling,edges)

    def assertDisjoint(self,clips,stems):
        spans=sorted((clips[s]['spu_address'],clips[s]['spu_address']+clips[s]['spu_bytes'])for s in stems)
        for (_,end),(start,_) in zip(spans,spans[1:]):self.assertLessEqual(end,start)

    def test_bank_preserves_source_channel_masks_gains_and_disjoint_addresses(self):
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);report,_=self.fixture(root)
            clips,cues=self.assemble(report,root)
            self.assertEqual([c['mask']for c in cues],[self.mask(f)for f in('level6','level7','level37')])
            self.assertEqual(cues[0]['gains'],
                [16383 if s in self.slots['level6']else 2 for s in range(len(ambience.CHANNELS))])
            self.assertEqual([c['fade_ticks']for c in cues],[30,30,30])
            # No clip owns a voice: each is handed one out of the pool when it
            # keys on, and the voice outside the pool is area music's.
            self.assertTrue(all('voice' not in c for c in clips))
            self.assertNotIn(ambience.MUSIC_VOICE,ambience.POOL_VOICES)
            for i,c in enumerate(clips):
                self.assertEqual(c['checksum'],ambience.fnv(c['payload']))
                self.assertEqual(c['byte_len'],16)
                self.assertEqual(c['valid_frames'],27)
                self.assertGreaterEqual(c['spu_address'],ambience.SPU_START)
            # Stems one cue plays together never share bytes; stems no cue
            # and no gate plays together do, which is the point.
            for file in self.slots:self.assertDisjoint(clips,self.slots[file])
            end=max(c['spu_address']+c['spu_bytes']for c in clips)
            self.assertEqual(end,ambience.SPU_START+16*max(len(s)for s in self.slots.values()))
            self.assertLess(end,ambience.SPU_START+16*len(clips))
            with self.assertRaisesRegex(ValueError,'capacity overflow'):
                self.assemble(report,root,ceiling=ambience.SPU_START+32)
            with self.assertRaisesRegex(ValueError,'overlaps'):
                self.assemble(report,root,sfx_end=ambience.SFX_END+16)
            (root/report['clips'][0]['planes'][0]['path']).write_bytes(bytes(16))
            with self.assertRaisesRegex(ValueError,'hash mismatch'):
                self.assemble(report,root)

    def test_capacity_overflow_names_the_clip_and_the_shortfall(self):
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);report,_=self.fixture(root)
            # The widest cue holds three 16-byte payloads; room for two leaves
            # its last one 16 over.
            ceiling=ambience.SPU_START+32
            with self.assertRaisesRegex(ValueError,r'ends 16 bytes past'):
                self.assemble(report,root,ceiling=ceiling)

    def test_a_gate_keeps_both_scenes_stems_apart(self):
        """The outgoing cue fades out while the incoming one rises, so two
        scenes joined by a gate must not share bytes; two that no gate joins may."""
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);report,_=self.fixture(root)
            apart,_=self.assemble(report,root)
            shared=lambda clips:any(
                clips[a]['spu_address']<clips[b]['spu_address']+16 and clips[b]['spu_address']<clips[a]['spu_address']+16
                for a in self.slots['level6'] for b in self.slots['level7'] if a!=b)
            self.assertTrue(shared(apart))
            joined,_=self.assemble(report,root,edges={(0,1)})
            self.assertFalse(shared(joined))
            self.assertDisjoint(joined,set(self.slots['level6'])|set(self.slots['level7']))

    def test_the_clips_one_gate_away_are_kept_apart_and_listed_for_prefetch(self):
        """A scene's neighbours' clips load in the background while it plays,
        so every neighbour's stems sit apart from the scene's and from each
        other's, and the manifest lists what each scene may prefetch."""
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);report,_=self.fixture(root)
            # level6 joins both others: its own stems and both neighbours' coexist.
            clips,cues=self.assemble(report,root,edges={(0,1),(0,2)})
            self.assertDisjoint(clips,set(range(len(ambience.CHANNELS))))
            by={c['scene']:c for c in cues}
            self.assertEqual(by[0]['prefetch'],(self.mask('level7')|self.mask('level37'))&~self.mask('level6'))
            self.assertEqual(by[1]['prefetch'],self.mask('level6')&~self.mask('level7'))
            text=ambience.rust_manifest(clips,cues)
            self.assertIn(f"pub const AMBIENCE_PREFETCH:[u8;3]={[by[s]['prefetch'] for s in range(3)]};",text)

    def test_a_tail_bank_above_a_gap_is_not_drift_but_an_overlap_is(self):
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);(root/'data').mkdir()
            bases=(0x40000,0x50000,0x60000)
            self.assertEqual(len(bases),len(ambience.TAIL_BANKS))
            for name,base in zip(ambience.TAIL_BANKS,bases):
                (root/name).write_text(f'pub const BANK_BYTES: usize = 4096;\npub const SPU_BASE: u32 = {base};\n')
            self.assertEqual(ambience.tail_drift(root,0x30000),{})
            self.assertEqual(ambience.tail_drift(root,0x40010),
                {ambience.TAIL_BANKS[0]:{'declared':0x40000,'required':0x40010}})

    def test_more_stems_audible_at_once_than_the_pool_holds_is_refused(self):
        """What a longer resident set costs is voices while stems are audible.

        A transition fades the outgoing cue's stems out while the incoming
        cue's rise, so the pool is sized on the union of two cues rather than
        on the largest single one or on the number of clips. Every SPU voice
        outside the pool belongs to another bank, so the cook stops here.
        """
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);report,_=self.fixture(root)
            clips,cues=self.assemble(report,root)
            live=ambience.pool_pressure(cues)
            # The fixture's cue shapes share only the stream, so the union of
            # any two is wider than either: that union is the number.
            self.assertGreater(live,max(bin(c['mask']).count('1')for c in cues))
            self.assertLessEqual(live,len(ambience.POOL_VOICES))
            with patch.object(ambience,'POOL_VOICES',ambience.POOL_VOICES[:live-1]):
                with self.assertRaisesRegex(ValueError,f'{live} stems can be audible at once.*SPU voices'):
                    self.assemble(report,root)

    def test_a_resident_set_wider_than_the_cue_mask_is_refused(self):
        """One bit per stem in a u8, on both sides of the manifest."""
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp)
            with patch.object(ambience,'CHANNELS',tuple(range(9))):
                with self.assertRaisesRegex(ValueError,'cue mask'):
                    ambience.assemble({'clips':[],'scenes':[]},root,{},{},ambience.SFX_END,ambience.SPU_END)

    def test_spu_ceiling_excludes_the_banks_stacked_above_ambience(self):
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);(root/'data').mkdir()
            sizes=(1024,2048,512)
            self.assertEqual(len(sizes),len(ambience.TAIL_BANKS))
            for name,size in zip(ambience.TAIL_BANKS,sizes):
                (root/name).write_text(f'pub const BANK_BYTES: usize = {size};\n')
            tail=ambience.spu_ceiling(root)
            self.assertEqual(tail['reserved_bytes'],3584)
            self.assertEqual(tail['ceiling'],ambience.SPU_END-3584)
            self.assertEqual(tail['banks'],dict(zip(ambience.TAIL_BANKS,sizes)))
            (root/ambience.TAIL_BANKS[0]).write_text('nothing useful\n')
            with self.assertRaisesRegex(ValueError,'no BANK_BYTES'):ambience.spu_ceiling(root)

    def test_sfx_overlap_and_manifest_drift_rejected(self):
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);(root/'data').mkdir()
            bank=root/'data/sfx.adpcm';manifest=root/'data/sfx.rs'
            bank.write_bytes(bytes(16));manifest.write_text('pub const BANK_BYTES: usize = 16;')
            self.assertEqual(ambience.sfx_reservation(root)['end'],0x1020)
            manifest.write_text('pub const BANK_BYTES: usize = 32;')
            with self.assertRaisesRegex(ValueError,'mismatch'):ambience.sfx_reservation(root)
            size=ambience.SFX_END-0x1010+16
            bank.write_bytes(bytes(size));manifest.write_text(f'pub const BANK_BYTES: usize = {size};')
            with self.assertRaisesRegex(ValueError,'overlaps'):ambience.sfx_reservation(root)

    def test_a_long_loop_is_held_whole_in_spu(self):
        """cave_noises streamed from RAM while every loop had to fit at once;
        per-area residency holds every loop whole, and area music has the ring."""
        with tempfile.TemporaryDirectory() as temp:
            root=pathlib.Path(temp);report,_=self.fixture(root)
            index=2
            clip=report['clips'][index]
            payload=bytes([12,0]+[0x45]*14)*12492
            path=root/clip['planes'][0]['path'];path.write_bytes(payload)
            clip.update(frames=len(payload)//16*28,padding_samples=0)
            clip['planes'][0].update(bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest())
            clips,_=self.assemble(report,root)
            self.assertEqual(clips[index]['byte_len'],199872)
            self.assertEqual(clips[index]['spu_bytes'],199872)
            self.assertEqual(clips[index]['payload'],ambience.loop_payload(payload))
            for file in self.slots:self.assertDisjoint(clips,self.slots[file])

    def test_source_gain_above_unity_clamps_to_full_scale_and_is_recorded(self):
        """MiscWind lifts channel 15 by 1.36 dB; 16,383 stays the ceiling."""
        self.assertEqual(ambience.volume(0),16383)
        self.assertEqual(ambience.volume(1.35545),16383)
        self.assertEqual(ambience.volume(ambience.BOOST_CEILING_DB),16383)
        with self.assertRaisesRegex(ValueError,'unsupported mixer gain'):
            ambience.volume(ambience.BOOST_CEILING_DB+0.001)
        with self.assertRaisesRegex(ValueError,'unsupported mixer gain'):
            ambience.volume(float('inf'))
        self.assertEqual(ambience.boosts((3,7),[-6.0,1.5]),{'7':1.5})
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);report,_=self.fixture(root)
            boosted=ambience.CHANNELS[0]
            self.snapshots['level6'][0]['internal_volume_db']=1.35545
            report['scenes'][0]['managers'][0]['ambience'][0]['snapshot']['internal_volume_db']=1.35545
            _,cues=self.assemble(report,root)
            self.assertEqual(cues[0]['gains'][0],16383)
            self.assertEqual(cues[0]['clamped_boost_db'],{str(boosted):1.35545})
            self.assertEqual(cues[1]['clamped_boost_db'],{})

    def test_cook_rate_is_per_channel(self):
        """A resident channel off RATE still resolves, at its own pitch."""
        exception=ambience.CHANNELS[-1]
        other=8000 if ambience.RATE!=8000 else 4000
        rates=lambda ch:other if ch==exception else ambience.RATE
        with patch.object(ambience,'atmos_rate',rates):
            with tempfile.TemporaryDirectory()as temp:
                root=pathlib.Path(temp);report,_=self.fixture(root)
                clips,_=self.assemble(report,root)
                odd=clips[-1]
                self.assertEqual(odd['source_channel'],exception)
                self.assertEqual(odd['rate'],other)
                self.assertEqual(odd['pitch'],round(other*4096/44100))
                self.assertEqual(odd['spu_bytes'],odd['byte_len'])
                self.assertEqual(clips[0]['rate'],ambience.RATE)
                self.assertNotEqual(clips[0]['pitch'],odd['pitch'])

    def test_manifest_lengths_follow_the_resident_channel_set(self):
        """The table used to say 6 whatever the set was."""
        clips=[{'byte_len':16*(i+1),'spu_bytes':16,'checksum':i,'spu_address':ambience.SPU_START+16*i,
                'pitch':743,'source_channel':ch}
               for i,ch in enumerate(ambience.CHANNELS)]
        cues=[{'mask':1,'gains':[0]*len(clips),'fade_ticks':30}]
        text=ambience.rust_manifest(clips,cues)
        self.assertIn(f'pub const AMBIENCE_CLIPS:[AmbienceClip;{len(clips)}]=[',text)
        self.assertIn(f'pub const AMBIENCE_SPU_END:u32={ambience.SPU_START+16*len(clips)};',text)
        self.assertIn(f'pub gains:[i16;{len(clips)}]',text)
        # The voice budget is stated once, as music's voice and ambience's
        # pool, rather than once per clip: that is what lets the table be
        # longer than the number of voices ambience owns.
        self.assertNotIn('voice:',text)
        self.assertIn(f'pub const MUSIC_VOICE:u8={ambience.MUSIC_VOICE};',text)
        self.assertIn(f'pub const MUSIC_RING_BYTES:usize={ambience.MUSIC_RING_BYTES};',text)
        self.assertIn(f'pub const AMBIENCE_POOL_VOICES:[u8;{len(ambience.POOL_VOICES)}]={list(ambience.POOL_VOICES)};',text)

    def test_fresh_or_stale_cache_invokes_reproducible_source_cooker(self):
        path=pathlib.Path('/ignored/music/provenance.json')
        with patch.object(ambience,'cached_report',side_effect=[FileNotFoundError(),{'ready':True}]),patch.object(ambience.subprocess,'run')as run:
            self.assertEqual(ambience.ensure_music(path),{'ready':True})
            self.assertEqual(run.call_args.args[0][-2:],['--output',str(path.parent)])
            self.assertTrue(run.call_args.kwargs['check'])
        with patch.object(ambience,'cached_report',return_value={'ready':True}),patch.object(ambience.subprocess,'run')as run:
            ambience.ensure_music(path);run.assert_not_called()

    def test_cache_cannot_silently_use_another_windows_install(self):
        with tempfile.TemporaryDirectory() as temp:
            root=pathlib.Path(temp);(root/'.hkpsx').mkdir()
            (root/'.hkpsx/doctor.json').write_text(json.dumps({'installs':[{'data_directory':str(root/'selected') }]}))
            path=root/'provenance.json';path.write_text(json.dumps({'source_directory':str(root/'old')}))
            with patch.object(ambience,'ROOT',root):
                with self.assertRaisesRegex(ValueError,'different selected Windows install'):
                    ambience.cached_report(path)

if __name__=='__main__':unittest.main()
