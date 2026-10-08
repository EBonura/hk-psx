"""Synthetic music groundwork checks; no retail data in tests."""
import array,hashlib,json,math,pathlib,shutil,struct,subprocess,sys,tempfile,unittest
from types import SimpleNamespace
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/'host'))
from cook_music import source_audio_ref,decoded_source,compile_encoder

class MusicSourceTests(unittest.TestCase):
    def test_unity6_resource_precedes_empty_legacy_clip(self):
        ref={'m_FileID':0,'m_PathID':1151}
        self.assertEqual(source_audio_ref({'m_Resource':ref,'m_audioClip':{'m_PathID':0}}),ref)
        self.assertEqual(source_audio_ref({'m_audioClip':ref}),ref)
        with self.assertRaisesRegex(ValueError,'no direct audio'):source_audio_ref({'m_Resource':{'m_PathID':0}})

    def test_source_resource_and_wav_cache_are_digest_guarded(self):
        with tempfile.TemporaryDirectory()as directory:
            root=pathlib.Path(directory);asset=root/'sound.resource';asset.write_bytes(b'xx1234yy')
            folder=root/'output';folder.mkdir();calls=[]
            def read():
                calls.append(1);return SimpleNamespace(samples={'sample.wav':b'wave'+bytes([len(calls)])})
            obj=SimpleNamespace(read=read)
            tree={'m_Name':'Synthetic','m_Resource':{'m_Source':'sound.resource','m_Offset':2,'m_Size':4}}
            source=SimpleNamespace(directory=root,read=lambda _:tree)
            wav,identity=decoded_source(source,obj,folder)
            self.assertEqual(identity['encoded_resource_sha256'],hashlib.sha256(b'1234').hexdigest())
            decoded_source(source,obj,folder);self.assertEqual(len(calls),1)
            asset.write_bytes(b'xx4321yy');decoded_source(source,obj,folder);self.assertEqual(len(calls),2)
            wav.write_bytes(b'corrupted');decoded_source(source,obj,folder);self.assertEqual(len(calls),3)
            asset.write_bytes(b'x')
            with self.assertRaisesRegex(ValueError,'truncated source'):decoded_source(source,obj,folder)
            tree['m_Resource']['m_Source']='../outside.resource'
            with self.assertRaisesRegex(ValueError,'escapes Windows'):decoded_source(source,obj,folder)

@unittest.skipUnless(shutil.which('cargo')and shutil.which('ffmpeg'),'host audio tools unavailable')
class MusicEncoderTests(unittest.TestCase):
    def test_predictive_payload_has_defined_start_exact_length_and_external_quality(self):
        with tempfile.TemporaryDirectory()as directory:
            root=pathlib.Path(directory);encoder=compile_encoder(root)
            samples=[round(22000*math.sin(i*.14))for i in range(10000)]
            pcm=root/'input.s16le';pcm.write_bytes(struct.pack('<10000h',*samples));encoded=root/'audio.adpcm'
            metrics=json.loads(subprocess.check_output([str(encoder),str(pcm),str(encoded)],text=True))
            payload=encoded.read_bytes();self.assertEqual(len(payload),math.ceil(len(samples)/28)*16)
            self.assertEqual(payload[0]>>4,0);self.assertTrue(all(payload[i+1]==0 for i in range(0,len(payload),16)))
            self.assertTrue(any(payload[i]>>4!=0 for i in range(16,len(payload),16)))
            self.assertEqual(metrics['samples'],len(samples))
            subprocess.run([str(encoder),str(pcm),str(encoded)],check=True,capture_output=True)
            self.assertEqual(encoded.read_bytes(),payload)
            vag=root/'test.vag';vag.write_bytes(b'VAGp'+struct.pack('>4I',0x20,0,len(payload),22050)+bytes(28)+payload)
            decoded=root/'decoded.s16le';subprocess.run(['ffmpeg','-v','error','-y','-i',str(vag),'-f','s16le',str(decoded)],check=True)
            a=array.array('h');a.frombytes(decoded.read_bytes())
            if sys.byteorder!='little':a.byteswap()
            self.assertEqual(len(a),math.ceil(len(samples)/28)*28)
            noise=sum((x-y)**2 for x,y in zip(samples,a));signal=sum(x*x for x in samples)
            self.assertGreater(10*math.log10(signal/noise),40)
            pcm.write_bytes(b'\0')
            self.assertNotEqual(subprocess.run([str(encoder),str(pcm),str(encoded)],capture_output=True).returncode,0)

if __name__=='__main__':unittest.main()
