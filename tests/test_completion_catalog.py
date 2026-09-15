"""Pure completion-rule extraction tests."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from completion_catalog import extract_conditional_fields, extract_direct_fields


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


if __name__=='__main__':
    unittest.main()
