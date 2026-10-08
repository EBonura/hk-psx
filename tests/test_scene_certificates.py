"""The guest build refuses joint residency before any cook or publication."""
import sys
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))


class SceneCertificates(unittest.TestCase):
    def test_build_rejects_joint_before_any_cook_or_publication(self):
        import build_guest
        with patch.object(build_guest,'scene_manifest')as pack:
            with self.assertRaisesRegex(ValueError,'scene_gate'):build_guest.build(residency='joint')
            pack.assert_not_called()

if __name__=='__main__':unittest.main()
