import sys,unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from audit_texture_similarity import metrics
from texture_match_groups import score_matrix,review_groups,all_static_matches

def tex(i,words,mode=0):
    a=np.array(words,dtype=np.uint16)
    return dict(id=i,words=a,w=a.shape[1],h=a.shape[0],modes=(mode,),stream=False,black=False,sprites={str(i)})

class TextureMatchGroupsTests(unittest.TestCase):
    def test_matrix_matches_scalar_for_color_transparency_and_both_blends(self):
        rng=np.random.default_rng(6)
        for mode in (0,1):
            items=[tex(i,rng.choice([0,1,31,0x8000,0x801f,0x7fff,0xffff],(7,11)),mode)for i in range(6)]
            score=score_matrix(items)
            for i in range(6):
                for j in range(6):self.assertAlmostEqual(score[i,j],metrics(items[i],items[j])['similarity'],places=5)
    def test_cross_source_and_native_size_are_both_included(self):
        items=[tex(0,[[31,31]]),tex(1,[[30,30]]),tex(2,[[31,31,31,31]])]
        r=all_static_matches(items,80)
        self.assertEqual(r['compared_pairs'],3);self.assertEqual(len(r['candidates']),3)
        self.assertTrue(all(not p['same_source']for p in r['candidates']))
        p=next(p for p in r['candidates']if p['a']==0 and p['b']==1)
        self.assertEqual(p['comparison'],'native')
    def test_chain_does_not_accumulate_error_to_final_representative(self):
        items=[tex(i,[[31]])for i in range(4)]
        pairs=[dict(a=a,b=b,similarity=85)for a,b in [(0,1),(1,2),(2,3)]]
        groups=review_groups(pairs,items,80)
        self.assertEqual(groups[0]['representative'],1)
        self.assertEqual([m['id']for m in groups[0]['members']],[1,0,2])
        self.assertEqual(groups[1]['members'],[dict(id=3,similarity=100)])
        self.assertEqual(groups,review_groups(list(reversed(pairs)),items,80))
    def test_threshold_changes_membership_without_duplicate_assignment(self):
        items=[tex(i,[[31]])for i in range(3)]
        pairs=[dict(a=0,b=1,similarity=96),dict(a=0,b=2,similarity=85)]
        g=review_groups(pairs,items,95)
        self.assertEqual(len(g),1);self.assertEqual(len(g[0]['members']),2)
        g=review_groups(pairs,items,80)
        ids=[m['id']for group in g for m in group['members']]
        self.assertEqual(sorted(ids),[0,1,2])

if __name__=='__main__':unittest.main()
