"""Synthetic generated type-tree regressions; no retail asset bytes."""
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from source import Source, repair_string_arrays
from UnityPy.helpers import TypeTreeHelper
from UnityPy.streams import EndianBinaryReader


def node(kind, name, children=(), flags=0):
    return SimpleNamespace(m_Type=kind, m_Name=name, m_Children=list(children), m_MetaFlag=flags)


def scalar_string(name):
    return node('string', name, [node('Array', 'Array', [node('int', 'size'), node('char', 'data')], 0x4000)])


def string_array(name):
    return node('string', name, [node('Array', 'Array', [node('int', 'size'), scalar_string('data')], 0x4000)])


def unity_string(value):
    raw=value.encode(); data=struct.pack('<I',len(raw))+raw
    return data+bytes((-len(data))&3)


class StringArrayTests(unittest.TestCase):
    def test_generated_array_decodes_all_strings_and_exact_length(self):
        fields=node('Fixture','Base',[string_array('actions'),scalar_string('title')])
        blob=struct.pack('<I',2)+unity_string('first')+unity_string('second')+unity_string('scalar')
        changed=repair_string_arrays(fields)
        self.assertEqual(len(changed),1)
        reader=EndianBinaryReader(blob,endian="<")
        result=TypeTreeHelper.read_value(fields,reader,TypeTreeHelper.TypeTreeConfig(True,None,False))
        self.assertEqual(result,{'actions':['first','second'],'title':'scalar'})
        self.assertEqual(reader.Position,len(blob))
        self.assertEqual(fields.m_Children[1].m_Type,'string')

    def test_empty_generated_array_is_not_an_empty_scalar_string(self):
        root=string_array('names');repair_string_arrays(root)
        result=TypeTreeHelper.read_value(root,EndianBinaryReader(bytes(4),endian="<"),TypeTreeHelper.TypeTreeConfig(True,None,False))
        self.assertEqual(result,[])
        ordinary=scalar_string('name')
        self.assertEqual(repair_string_arrays(ordinary),[])

    def test_source_restores_tree_and_reader_after_failed_parse(self):
        root=string_array('names');before=TypeTreeHelper.read_typetree_boost
        def fail(**kwargs):
            self.assertEqual(root.m_Type,'vector')
            raise ValueError('synthetic malformed source')
        obj=SimpleNamespace(type=SimpleNamespace(name='MonoBehaviour'),
                            _get_typetree_node=lambda:root,read_typetree=fail)
        with self.assertRaisesRegex(ValueError,'synthetic malformed'):
            Source.__new__(Source).read(obj)
        self.assertEqual(root.m_Type,'string')
        self.assertIs(TypeTreeHelper.read_typetree_boost,before)


if __name__ == '__main__':
    unittest.main()
