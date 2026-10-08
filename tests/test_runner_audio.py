"""Source-free Runner audio category, integrity, flags and capacity contracts."""
import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'host'))
from runner_audio import SOURCES, SPU_BASE, SPU_END, GAIN, assemble, validate_bank, verify_outputs
from ambience import fnv


class RunnerAudioTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name); (self.root/'.hkpsx').mkdir()
        self.profiles=[]; self.contract={'clips':[]}
        for i,(event,sid,rate,loop) in enumerate(SOURCES):
            raw=bytes([12,0])+bytes([0x21+i])*14+bytes([28,0])+bytes([0x12+i])*14
            path=self.root/'.hkpsx'/f'{i}.adpcm';path.write_bytes(raw)
            identity={'clip_metadata_sha256':'metadata'+str(i),'encoded_resource_sha256':'source'+str(i)}
            self.contract['clips'].append(dict(event=event,source=sid,rate=rate,loop=loop,name='fixture'+str(i),
                source_identity=identity,source_length_seconds=53/rate,source_volume=1.,gain=GAIN,pitch_multipliers=[0.9973541498184204]*2 if loop else [.8500000238418579,1.149999976158142]))
            self.profiles.append(dict(source=sid,name='fixture'+str(i),source_identity=identity,rate=rate,channels=1,
                spu_pitch=round(rate*4096/44100),frames=53,padding_samples=3,source_pcm_frames=53,source_pcm_rate=rate,
                bytes=len(raw),planes=[dict(path=str(path.relative_to(self.root)),bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())]))

    def test_full_duration_flags_offsets_and_checksum(self):
        raw=[(self.root/p['planes'][0]['path']).read_bytes() for p in self.profiles]
        bank,desc=assemble(self.profiles,self.root,self.contract)
        self.assertEqual(len(bank),128)
        self.assertEqual([r['offset'] for r in desc['clips']],[0,32,80])
        self.assertEqual([r['spu_address'] for r in desc['clips']],[SPU_BASE,SPU_BASE+32,SPU_BASE+80])
        self.assertEqual((bank[1],bank[17]),(4,3))
        for i,record in enumerate(desc['clips']):
            data=bank[record['offset']:record['offset']+record['byte_len']]
            self.assertEqual(record['valid_frames'],53)
            self.assertEqual(record['checksum'],fnv(data))
            if i:
                self.assertEqual(data[:-16],raw[i]);self.assertEqual(data[-16:],bytes([12,1])+bytes(14))
        self.assertEqual(bank[2:16],raw[0][2:16]);self.assertEqual(bank[18:32],raw[0][18:32])

    def test_wrong_rate_source_pitch_and_duration_rejected(self):
        for change in [dict(rate=8000),dict(source=SOURCES[0][1]),dict(channels=2),dict(spu_pitch=10),
                       dict(frames=28,padding_samples=0),dict(source_identity={})]:
            profiles=copy.deepcopy(self.profiles);profiles[1].update(change)
            with self.subTest(change=change),self.assertRaises(ValueError):assemble(profiles,self.root,self.contract)
        with self.assertRaises(ValueError):assemble(self.profiles[::-1],self.root,self.contract)
        with self.assertRaises(ValueError):assemble(self.profiles[:2],self.root,self.contract)

    def test_altered_input_bytes_flags_padding_and_path_rejected(self):
        profile=self.profiles[0];path=self.root/profile['planes'][0]['path'];raw=path.read_bytes()
        path.write_bytes(raw[:-1]+b'\0')
        with self.assertRaisesRegex(ValueError,'integrity'):assemble(self.profiles,self.root,self.contract)
        path.write_bytes(raw)
        for replacement in [bytes([0x5c,0])+raw[2:],bytes([12,1])+raw[2:]]:
            path.write_bytes(replacement);profile['planes'][0]['sha256']=hashlib.sha256(replacement).hexdigest()
            with self.subTest(replacement=replacement),self.assertRaises(ValueError):assemble(self.profiles,self.root,self.contract)
        path.write_bytes(raw);profile['planes'][0]['sha256']=hashlib.sha256(raw).hexdigest()
        profile['padding_samples']=0
        with self.assertRaisesRegex(ValueError,'padding'):assemble(self.profiles,self.root,self.contract)
        profile['padding_samples']=3
        outside=self.root/'outside.adpcm';outside.write_bytes(raw);profile['planes'][0]['path']=str(outside)
        with self.assertRaisesRegex(ValueError,'ignored storage'):assemble(self.profiles,self.root,self.contract)

    def test_complete_bank_overflow_fails_without_trimming(self):
        # One block more than the whole tail above the Focus bank, derived
        # rather than written down: this read 1800 blocks, which stopped being
        # an overflow the moment ambience shrank and the tail grew under it.
        blocks=(SPU_END-SPU_BASE)//16+1
        p=self.profiles[0];raw=(bytes([12,0])+bytes(14))*blocks
        path=self.root/p['planes'][0]['path'];path.write_bytes(raw)
        p.update(frames=blocks*28,source_pcm_frames=blocks*28,padding_samples=0,bytes=len(raw))
        self.contract['clips'][0]['source_length_seconds']=blocks*28/SOURCES[0][2]
        p['planes'][0].update(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
        with self.assertRaisesRegex(ValueError,'no samples trimmed'):assemble(self.profiles,self.root,self.contract)
        with self.assertRaisesRegex(ValueError,'tail changed'):assemble(self.profiles,self.root,self.contract,SPU_BASE+16)

    def test_output_corruption_and_layout_rejected(self):
        bank,desc=assemble(self.profiles,self.root,self.contract)
        with self.assertRaisesRegex(ValueError,'checksum'):validate_bank(bank[:-1]+b'\1',desc)
        for field,value in [('offset',16),('spu_address',0),('rate',8000),('pitch',1),('valid_frames',80),('gain',1),('pitch_bounds',[1,2])]:
            broken=copy.deepcopy(desc);broken['clips'][1][field]=value
            with self.subTest(field=field),self.assertRaises(ValueError):validate_bank(bank,broken)
        # Even if integrity stamps are recomputed, bad transport flags fail.
        bad=bytearray(bank);bad[1]=0;broken=copy.deepcopy(desc)
        broken.update(checksum=fnv(bad),sha256=hashlib.sha256(bad).hexdigest())
        broken['clips'][0].update(checksum=fnv(bad[:32]),sha256=hashlib.sha256(bad[:32]).hexdigest())
        with self.assertRaisesRegex(ValueError,'loop'):validate_bank(bytes(bad),broken)

    def test_verify_rejects_stale_input_and_output_without_rewriting(self):
        bank,desc=assemble(self.profiles,self.root,self.contract)
        payload=self.root/'.hkpsx/bank.adpcm';payload.write_bytes(bank)
        source=self.root/'input';source.write_bytes(b'original')
        report=self.root/'.hkpsx/report.json';report.write_text(json.dumps(dict(path=str(payload),bank=desc,
            identity={str(source):hashlib.sha256(source.read_bytes()).hexdigest()})))
        before=report.read_bytes();verify_outputs(report)
        source.write_bytes(b'stale')
        with self.assertRaisesRegex(ValueError,'stale'):verify_outputs(report)
        source.write_bytes(b'original');payload.write_bytes(bank[:-1]+b'\1')
        with self.assertRaisesRegex(ValueError,'checksum'):verify_outputs(report)
        self.assertEqual(report.read_bytes(),before)


if __name__=='__main__':unittest.main()
