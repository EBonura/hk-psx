"""Tests for installed managed progression-field indexing."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools'))
from managed_field_usage import build_field_index, completion_fields


class ManagedFieldUsageTests(unittest.TestCase):
    def test_completion_fields_include_alternatives_nested_and_count_output(self):
        completion={'rules':{
            'charm_count':{'flags':['gotCharm_1'],'output':'charmsOwned'},
            'boolean_rules':[{'kind':'boolean','field':'hasDash'},
                             {'kind':'any_boolean','fields':['won','banished']}],
            'integer_rules':[{'field':'nailSmithUpgrades'}],
            'nested_boolean_rules':[{'field':'bossDoorStateTier1'}],
            'soul_vessel_rule':{'field':'MPReserveMax'}}}
        self.assertEqual(completion_fields(completion), {
            'gotCharm_1','charmsOwned','hasDash','won','banished',
            'nailSmithUpgrades','bossDoorStateTier1','MPReserveMax'})

    def test_field_index_keeps_read_write_and_address_distinct(self):
        method={'method_token':'0x06:1','declaring_type':'A','method':'M',
                'method_sha256':'abc','calls':[], 'accesses':[
                    {'field':'flag','operation':'read','offset':1},
                    {'field':'flag','operation':'read','offset':3},
                    {'field':'flag','operation':'write','offset':5},
                    {'field':'nested','operation':'address','offset':7}]}
        result={row['field']:row for row in build_field_index({'flag','nested'},[method])}
        self.assertEqual(result['flag']['readers'][0]['offsets'],[1,3])
        self.assertEqual(result['flag']['writers'][0]['offsets'],[5])
        self.assertEqual(result['nested']['address_users'][0]['offsets'],[7])


if __name__ == '__main__':
    unittest.main()
