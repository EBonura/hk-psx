"""Pure completion-rule extraction tests."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from completion_catalog import (copied_assignments, extract_conditional_fields,
                                extract_direct_fields, literal_assignments,
                                attach_player_data)


def row(op,field=None,literal=None,offset=0):
    return {'op':op,'field':field,'literal':literal,'offset':offset,'target':None}


class CompletionCatalogTests(unittest.TestCase):
    def test_boolean_weight_is_extracted_from_accumulator_block(self):
        rows=[row('ldfld','killedBoss'),row('brfalse.s'),row('ldarg.0'),
              row('ldfld','completionPercentage'),row('ldc.r4',literal=1.0),
              row('add'),row('stfld','completionPercentage')]
        result=extract_conditional_fields(rows,'completionPercentage')
        self.assertEqual(result[0]['field'],'killedBoss')
        self.assertEqual(result[0]['weight'],1.0)

    def test_integer_baseline_is_extracted(self):
        rows=[row('ldfld','completionPercentage'),row('ldfld','maxHealthBase'),
              row('ldc.i4.5',literal=5),row('sub'),row('conv.r4'),row('add'),
              row('stfld','completionPercentage')]
        result=extract_direct_fields(rows,'completionPercentage')
        self.assertEqual(result[0]['kind'],'integer_minus_baseline')
        self.assertEqual(result[0]['field'],'maxHealthBase')
        self.assertEqual(result[0]['baseline'],5)

    def test_literal_and_cap_copy_assignments_are_exact(self):
        rows=[row('ldarg.0'),row('ldc.i4',literal=9),row('stfld','maxHealthCap'),
              row('ldarg.0'),row('ldarg.0'),row('ldfld','maxHealthCap'),
              row('stfld','maxHealthBase')]
        self.assertEqual(literal_assignments(rows),{'maxHealthCap':[9]})
        self.assertEqual(copied_assignments(rows),{'maxHealthBase':'maxHealthCap'})

    def test_nested_completion_fields_are_cross_referenced(self):
        rules={'charm_count':{'flags':[]},'boolean_rules':[],
               'integer_rules':[],'nested_boolean_rules':[{'field':'tier'}],
               'soul_vessel_rule':{'field':'reserve'}}
        catalog={'player_data':{'keys':[]}}
        self.assertEqual([row['field'] for row in attach_player_data(rules,catalog)],
                         ['reserve','tier'])


if __name__=='__main__':
    unittest.main()
