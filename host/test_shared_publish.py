import ast, os, tempfile, unittest
from pathlib import Path
from unittest.mock import patch
import shared_publish

class SharedPublisherDriver(unittest.TestCase):
    def test_missing_tool_source_fails_closed(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(ValueError, 'HK_OVERLAY_SDK'):
                shared_publish.tool_source()
    def test_wrong_revision_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'HK_OVERLAY_SDK':directory}), patch('subprocess.check_output',return_value='wrong\n'):
            with self.assertRaisesRegex(ValueError,'revision mismatch'):
                shared_publish.tool_source()
    def test_full_publication_dispatch(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory); out=root/'build'; out.mkdir()
            link_map=root/'game.map'; link_map.write_text('map')
            with patch.dict(os.environ, {'CARGO_TARGET_DIR':str(root/'target')}), patch('shared_publish.tool_source',return_value=root/'sdk'), patch('subprocess.run') as run, patch.dict(shared_publish.cm.MODULES,{'crawler':None},clear=True), patch('shared_publish.cm.carried_blobs',return_value={}), patch('shared_publish.cm.all_names',return_value=['crawler']), patch('shared_publish.cm.package_chunks',return_value=[('code',None,[0])]), patch.object(shared_publish.cm,'CARRIED',()), patch('shared_publish.cm.map_with_only',return_value='resident'):
                def execute(command, **kwargs):
                    if len(command)>1 and command[1]=='assemble':
                        output=Path(command[-1]); (output/'chunks').mkdir(exist_ok=True)
                        for name in ('runtime.exe','modules.json','transport.json'):
                            (output/name).write_text('{}')
                run.side_effect=execute
                shared_publish.publish(root,root/'game.elf',link_map,out,{0:[0]}, {})
            commands=[call.args[0] for call in run.call_args_list]
            operations=[command[1] for command in commands if command[0].endswith('hk-module-publish')]
            self.assertEqual(operations,['pipeline','assemble','assemble'])
            pipeline=next(command for command in commands if len(command)>1 and command[1]=='pipeline')
            self.assertEqual(pipeline[-1],str((root/'target/overlay-sdk-tools/release').resolve()))
            self.assertEqual((out/'resident.map').read_text(),'resident')
            import json
            config=json.loads((out/'transport-config.json').read_text())
            self.assertEqual(config['rooms'],[[0]])
            self.assertEqual(config['chunks'][0]['stem'],'code_crawler')
            self.assertEqual(config['chunks'][0]['modules'],[0])
            self.assertTrue((out/'modules.json').exists())
            self.assertTrue((out/'chunks').is_dir())
            source=Path(__file__).with_name('build_guest.py').read_text()
            ast.parse(source)
            self.assertIn('publish(ROOT, linked, link_map, out, scene_code, art)',source)

if __name__=='__main__': unittest.main()
