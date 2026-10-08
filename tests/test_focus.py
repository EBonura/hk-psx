"""Synthetic compact-FSM and fixed-step parameter checks; no retail bytes."""
import sys,struct,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from focus import action_fields,generated_focus_params

class FocusSourceTests(unittest.TestCase):
    def test_compact_values_keep_variable_references_distinct_from_defaults(self):
        raw=struct.pack('<f',.027)+b'\x01Drain'+struct.pack('<i',33)+b'\x00'+b'\x01\x00'
        data={'actionStartIndex':[0],'actionNames':['Example'],'paramName':['time','cost','enabled'],
            'paramDataType':[15,16,17],'paramDataPos':[0,10,15],'paramByteDataSize':[10,5,2],'byteData':list(raw)}
        fields=action_fields(data,0)
        self.assertEqual(fields['time']['name'],'Drain');self.assertTrue(fields['time']['useVariable'])
        self.assertEqual(fields['cost'],{'value':33,'useVariable':False,'name':''})
        self.assertEqual(fields['enabled']['value'],True)
    def test_truncated_scalar_is_rejected(self):
        data={'actionStartIndex':[0],'actionNames':['Example'],'paramName':['time'],
            'paramDataType':[15],'paramDataPos':[0],'paramByteDataSize':[4],'byteData':[0]*4}
        with self.assertRaises(ValueError):action_fields(data,0)
    def test_source_float_roundoff_does_not_add_a_tick_and_fast_unbounded_drain_is_rejected(self):
        values={'hold_seconds':.25,'start_seconds':.25,'heal_seconds':.20000000298023224,
            'cancel_seconds':.25,'finish_seconds':.23000000417232513,'first_grace_seconds':.20000000298023224,
            'repeat_grace_seconds':.44999998807907104,'attack_recovery_seconds':.10000000149011612,
            'drain_seconds':.027000000700354576,'cost':33,'heal_amount':1}
        generated=generated_focus_params(values)
        for text in ['heal_ticks:12','finish_ticks:14','repeat_grace_ticks:27','attack_recovery_ticks:6','drain_interval_us:27000']:
            self.assertIn(text,generated)
        with self.assertRaises(ValueError):generated_focus_params(values|{'drain_seconds':.001})
if __name__=='__main__':unittest.main()
