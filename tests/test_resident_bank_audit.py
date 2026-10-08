import importlib.util
import struct
import sys
import unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'));sys.path.insert(0,str(ROOT/'tests'))
from audit_resident_bank import Bank,dense_pack,adjacent
from test_region_delta import room

class ResidentBankAuditTests(unittest.TestCase):
    def test_moved_odd_atlas_and_palette_order_roundtrip_without_texture_duplication(self):
        bank=Bank();a=room();b=room(moved=True)
        ra=bank.add(a);rb=bank.add(b)
        self.assertEqual(len(bank.texture.values),2)
        self.assertEqual(bank.restore(ra),a);self.assertEqual(bank.restore(rb),b)
    def test_palette_changes_cannot_alias_and_mutation_fails_hash(self):
        bank=Bank();a=bank.add(room());bank.add(room(change=True))
        self.assertEqual(len(bank.texture.values),3)
        blob=bank.texture.values[0];bank.texture.values[0]=blob[:-1]+bytes([blob[-1]^1])
        with self.assertRaises(ValueError):bank.restore(a)
    def test_geometry_normalization_restores_local_texture_references(self):
        # Add a source-style draw+frame referencing different local slots.
        raw=room();header=bytearray(raw[:40]);struct.pack_into('<I',header,16,1);struct.pack_into('<I',header,20,1)
        draw=struct.pack('<HHI8i4B',0,0,65536,*([0]*8),128,128,128,0)
        frame=struct.pack('<I4i',1,0,0,65536,65536)
        target=bytes(header)+raw[40:72]+draw+frame+raw[72:]
        bank=Bank();recipe=bank.add(target)
        self.assertEqual(bank.restore(recipe),target)
    def test_dense_atlas_stays_in_tpage_and_uses_real_area(self):
        count,boxes=dense_pack([(128,128,i)for i in range(5)])
        self.assertEqual(count,2);self.assertEqual(len(boxes),5)
        with self.assertRaises(ValueError):dense_pack([(260,1,0)])
        with self.assertRaises(ValueError):dense_pack([(3,1,0)])
    def test_corner_contact_does_not_inflate_direct_working_set(self):
        self.assertTrue(adjacent([0,0,10,10],[10,0,20,10]))
        self.assertFalse(adjacent([0,0,10,10],[10,10,20,20]))

if __name__=='__main__':unittest.main()
