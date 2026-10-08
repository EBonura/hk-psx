"""Production guest orchestration with the native simulation and mocked SPU."""
import json,os,subprocess,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

class FocusAudioRuntimeTests(unittest.TestCase):
    def test_native_spu_contract(self):
        for name in ('focus-audio.rs','focus-audio.adpcm'):
            self.assertTrue((ROOT/'data'/name).is_file(),f'Missing data/{name}; run host/focus_audio.py before native audio tests')
        output=ROOT/'.hkpsx/focus-audio-runtime-tests';output.mkdir(parents=True,exist_ok=True)
        env=dict(os.environ,CARGO_TARGET_DIR=str(output/'cargo'))
        result=subprocess.run(['cargo','build','--locked','--manifest-path',str(ROOT/'shared/hk-sim/Cargo.toml'),'--message-format=json'],cwd=ROOT,env=env,check=True,capture_output=True,text=True)
        libraries={}
        for line in result.stdout.splitlines():
            record=json.loads(line)
            if record.get('reason')!='compiler-artifact':continue
            name=record['target']['name']
            if name in ('hk_sim','hk_format'):
                files=[Path(p) for p in record['filenames'] if p.endswith('.rlib')]
                self.assertEqual(len(files),1);libraries[name]=files[0]
        self.assertEqual(set(libraries),{'hk_sim','hk_format'})
        executable=output/'focus-audio-runtime-tests'
        command=['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/focus_audio_runtime.rs')]
        for name,path in sorted(libraries.items()):command+=['--extern',f'{name}={path}']
        for folder in sorted({path.parent for path in libraries.values()}):command += ['-L',f'dependency={folder}']
        command += ['-o',str(executable)]
        subprocess.run(command,cwd=ROOT,env=dict(env,CARGO_MANIFEST_DIR=str(ROOT/'game')),check=True)
        subprocess.run([str(executable)],cwd=ROOT,check=True)

if __name__=='__main__':unittest.main()
