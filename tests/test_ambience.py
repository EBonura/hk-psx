"""Synthetic ambience bank/loop checks; no retail fixture samples."""
import copy,hashlib,json,pathlib,sys,tempfile,unittest
from unittest.mock import patch
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/'host'))
import ambience

class AmbienceTests(unittest.TestCase):
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
            clips.append({'source':str(ch),'name':str(ch),'rate':8000,'channels':1,'frames':27,'padding_samples':1,'spu_pitch':743,
                'planes':[{'path':str(path.relative_to(root)),'bytes':16,'sha256':hashlib.sha256(data).hexdigest()}]})
            entries[ch]={'channel':ch,'clip':str(ch),'loop':True,'pitch':1,'volume':1,
                'snapshot':{'internal_volume_db':0,'effects':[],'chain':[{'mute':False,'solo':False,'pitch':1}]}}
        for file,enabled in [('level6',[0,4,5,6]),('level7',[1,3,5,6]),('level37',[0,4,5,6])]:
            snapshots[file]=[{'internal_volume_db':0 if ch in enabled else -80}for ch in ambience.CHANNELS]
            scenes.append({'scene_file':file,'managers':[{'source':file+':1','atmos_cue':{'name':file},
                'ambience':[entries[ch]for ch in enabled]}]})
        return {'clips':clips,'scenes':scenes},snapshots

    def test_bank_preserves_source_channel_masks_gains_and_disjoint_addresses(self):
        with tempfile.TemporaryDirectory()as temp:
            root=pathlib.Path(temp);report,snapshots=self.fixture(root)
            clips,cues=ambience.assemble(report,root,{'level6':.5,'level7':.5,'level37':.5},snapshots,ambience.SFX_END)
            self.assertEqual([c['mask']for c in cues],[57,54,57])
            self.assertEqual(cues[0]['gains'],[16383,2,2,16383,16383,16383])
            self.assertEqual([c['fade_ticks']for c in cues],[30,30,30])
            self.assertEqual([c['voice']for c in clips],list(range(6,12)))
            for i,c in enumerate(clips):
                self.assertEqual(c['spu_address'],ambience.SPU_START+i*16)
                self.assertEqual(c['checksum'],ambience.fnv(c['payload']))
                self.assertEqual(c['byte_len'],16)
                self.assertEqual(c['valid_frames'],27)
            with patch.object(ambience,'SPU_END',ambience.SPU_START+80):
                with self.assertRaisesRegex(ValueError,'capacity overflow'):
                    ambience.assemble(report,root,{'level6':.5,'level7':.5,'level37':.5},snapshots,ambience.SFX_END)
            with self.assertRaisesRegex(ValueError,'overlaps'):
                ambience.assemble(report,root,{'level6':.5,'level7':.5,'level37':.5},snapshots,ambience.SFX_END+16)
            (root/report['clips'][0]['planes'][0]['path']).write_bytes(bytes(16))
            with self.assertRaisesRegex(ValueError,'hash mismatch'):
                ambience.assemble(report,root,{'level6':.5,'level7':.5,'level37':.5},snapshots,ambience.SFX_END)

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

    def test_long_cave_loop_keeps_payload_but_reserves_only_ring_in_spu(self):
        with tempfile.TemporaryDirectory() as temp:
            root=pathlib.Path(temp);report,snapshots=self.fixture(root)
            clip=report['clips'][3]
            payload=bytes([12,0]+[0x45]*14)*12492
            path=root/clip['planes'][0]['path'];path.write_bytes(payload)
            clip.update(frames=len(payload)//16*28,padding_samples=0)
            clip['planes'][0].update(bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest())
            clips,_=ambience.assemble(report,root,{'level6':.5,'level7':.5,'level37':.5},snapshots,ambience.SFX_END)
            self.assertEqual(clips[3]['byte_len'],199872)
            self.assertEqual(clips[3]['spu_bytes'],16384)
            self.assertEqual(clips[3]['payload'],ambience.loop_payload(payload))
            self.assertEqual(clips[4]['spu_address'],clips[3]['spu_address']+16384)
            self.assertEqual(sum(c['byte_len']-c['spu_bytes'] for c in clips),183488)
            with patch.object(ambience,'STREAM_RAM_LIMIT',199856):
                with self.assertRaisesRegex(ValueError,'RAM cache'):
                    ambience.assemble(report,root,{'level6':.5,'level7':.5,'level37':.5},snapshots,ambience.SFX_END)

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
