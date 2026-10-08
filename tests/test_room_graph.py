"""Synthetic topology tests; no extracted room data is included."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'host'))
from room_graph import finalize


def room(index, name, transitions=(), errors=()):
    return {'index': index, 'file': f'level{index}', 'path': f'Assets/Scenes/{name}.unity',
            'graph': {'transitions': list(transitions), 'errors': list(errors)}}


def gate(sid, name, target, entry, enabled=True):
    return {'source_id': sid, 'gate_name': name, 'target_scene': target,
            'entry_point': entry, 'component_enabled': enabled,
            'active_in_hierarchy': enabled}


class RoomGraphTests(unittest.TestCase):
    def test_exact_scene_and_gate_without_assuming_reciprocity(self):
        report = finalize([
            room(1, 'Room_A', [gate('level1:10', 'exit', 'Room_B', 'entry', False)]),
            room(2, 'Room_B', [gate('level2:20', 'entry', '', '')]),
        ])
        self.assertEqual(report['rooms'][0]['neighbours'], ['Room_B'])
        self.assertEqual(report['rooms'][1]['neighbours'], [])
        edge = report['edges'][0]
        self.assertEqual(edge['resolution'], 'mapped_gate')
        self.assertEqual(edge['target_gate_source_ids'], ['level2:20'])
        self.assertFalse(edge['active_in_hierarchy'])

    def test_no_fuzzy_or_invented_adjacency(self):
        report = finalize([
            room(1, 'Room_A', [gate('a:1', 'exit', 'room_b', 'entry')]),
            room(2, 'Room_B'),
        ])
        self.assertEqual(report['rooms'][0]['neighbours'], [])
        self.assertEqual(report['edges'][0]['resolution'], 'unresolved_scene')
        self.assertEqual(len(report['unresolved']), 1)

    def test_ambiguous_scene_and_missing_gate_are_explicit(self):
        report = finalize([
            room(1, 'Room_A', [gate('a:1', 'exit', 'Room_B', 'entry'),
                               gate('a:2', 'exit2', 'Room_C', 'entry')]),
            room(2, 'Room_B'), room(3, 'Room_B'),
            room(4, 'Room_C', errors=[{'source_id': 'c:1', 'error': 'schema unresolved'}]),
        ])
        self.assertEqual(report['edges'][0]['resolution'], 'ambiguous_scene')
        self.assertEqual(report['edges'][1]['resolution'], 'mapped_scene_unresolved_gate')
        self.assertEqual(report['rooms'][0]['neighbours'], ['Room_C'])
        self.assertEqual(len(report['unresolved']), 3)

    def test_ambiguous_destination_gate_is_not_chosen_arbitrarily(self):
        report = finalize([
            room(1, 'Room_A', [gate('a:1', 'exit', 'Room_B', 'entry')]),
            room(2, 'Room_B', [gate('b:1', 'entry', '', ''), gate('b:2', 'entry', '', '')]),
        ])
        edge = report['edges'][0]
        self.assertEqual(edge['resolution'], 'mapped_scene_unresolved_gate')
        self.assertEqual(edge['gate_resolution_reason'], 'ambiguous gate name')
        self.assertEqual(edge['target_gate_source_ids'], ['b:1', 'b:2'])
        self.assertEqual(report['rooms'][0]['neighbours'], ['Room_B'])


if __name__ == '__main__':
    unittest.main()
