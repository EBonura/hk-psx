import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from effects import generated_corpse

class CorpseMetadataTests(unittest.TestCase):
    def test_legacy_actor_has_no_unverified_corpse(self):
        self.assertEqual(generated_corpse(None),'None')
    def test_cooked_source_clips_and_collider_survive_rust_generation(self):
        result=generated_corpse({'air_clip':15,'land_clip':16,'bounds':[-46080,-55296,47104,4096],'spawn_offset':[0,32768],'bounce_factor':19661})
        self.assertIn('air_clip:15,land_clip:16',result)
        self.assertIn('bounds:[-46080,-55296,47104,4096]',result)
        self.assertIn('spawn_offset:[0,32768]',result)
        self.assertIn('bounce_factor:19661',result)
    def test_partial_cook_must_not_publish_corpse(self):
        for missing in ('air_clip','land_clip','bounds','spawn_offset','bounce_factor'):
            record={'air_clip':15,'land_clip':16,'bounds':[-46080,-55296,47104,4096],'spawn_offset':[0,32768],'bounce_factor':19661}
            del record[missing]
            with self.assertRaisesRegex(ValueError,'missing cooked'):generated_corpse(record)

from effects import polygon_moment,generated_debris
class DoorMetadataTests(unittest.TestCase):
    def test_uniform_moment_rectangle_triangle_and_winding(self):
        rectangle=[[-1.,-2.],[1.,-2.],[1.,2.],[-1.,2.]]
        center,inertia=polygon_moment(rectangle)
        self.assertEqual(center,[0.,0.]);self.assertAlmostEqual(inertia,(4+16)/12)
        self.assertEqual(polygon_moment(list(reversed(rectangle))),(center,inertia))
        center,inertia=polygon_moment([[0.,0.],[3.,0.],[0.,6.]])
        self.assertEqual(center,[1.,2.]);self.assertAlmostEqual(inertia,(9+36)/18)
    def test_degenerate_geometry_rejected(self):
        with self.assertRaisesRegex(ValueError,'degenerate'):polygon_moment([[0,0],[1,1],[2,2]])
    def test_generated_id_owner_and_reflection_survive(self):
        p={'id':0,'door_state':38,'frame':105,'source':'level6:10618','origin':[1,2],'centroid':[3,4],'scale':[-65536,65536],
           'polygon':[[0,0],[1,0],[1,1]],'torque':100,'angle_offset':-60}
        result=generated_debris([p],1)
        self.assertIn('door:166',result);self.assertIn('frame:105',result);self.assertIn('scale:[-65536,65536]',result)
        with self.assertRaisesRegex(ValueError,'identity'):generated_debris([p,p],1)
        with self.assertRaisesRegex(ValueError,'pool'):generated_debris([p]*29,1)
