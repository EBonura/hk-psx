"""Tests for source-order-independent world identities."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from world_identity import (Registry, asset_key, builtin_asset_key, object_key,
                            scene_key, spawned_key, stable_id)


class WorldIdentityTests(unittest.TestCase):
    def test_scene_and_object_ignore_build_file_order(self):
        path='Assets/Scenes/Tutorial_01.unity'
        self.assertEqual(scene_key(path),scene_key(path.replace('/','\\')))
        self.assertNotIn('level6',object_key(path,12837,'TransitionPoint'))
        self.assertEqual(stable_id(object_key(path,12837,'TransitionPoint')),
                         stable_id(object_key(path,12837,'TransitionPoint')))

    def test_external_asset_uses_content_file_hash(self):
        self.assertEqual(asset_key('abc',7),'asset|sha256:abc|7')
        self.assertEqual(builtin_asset_key('unity default resources',2),
                         'asset|builtin:unity default resources|2')

    def test_spawned_instance_needs_prefab_owner_and_stable_slot(self):
        prefab=stable_id('asset|a');owner=stable_id('object|b')
        self.assertNotEqual(stable_id(spawned_key(prefab,owner,0)),
                            stable_id(spawned_key(prefab,owner,1)))

    def test_legacy_sparse_id_cannot_alias_two_owners(self):
        with tempfile.TemporaryDirectory() as directory:
            registry=Registry(Path(directory)/'ids.sqlite')
            one=registry.add('component','object|a','a','owner','active_scene')
            two=registry.add('component','object|b','b','owner','active_scene')
            registry.add_alias('breakable_scene_x128',7,one,'level1:1')
            registry.add_alias('breakable_scene_x128',7,one,'level1:1')
            with self.assertRaisesRegex(ValueError,'aliases multiple owners'):
                registry.add_alias('breakable_scene_x128',7,two,'level1:2')
            registry.connection.close()


if __name__=='__main__':
    unittest.main()
