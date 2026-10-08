"""Synthetic dependency graphs: no retail identifiers or extracted content."""
import copy
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from residency_plan import plan_inventory, plan_all


def fixture():
    inventory = {
        'rooms': [
            {'scene_name': 'A', 'texture_ids': ['shared', 'a', 'shared']},
            {'scene_name': 'B', 'texture_ids': ['shared', 'b']},
            {'scene_name': 'C', 'texture_ids': ['shared', 'b', 'c']},
        ],
        'textures': {key: {'cooked_ram_bytes': 10, 'cooked_vram_bytes': 20}
                     for key in ['a', 'b', 'c', 'shared']},
    }
    graph = {'rooms': [{'scene_name': 'A', 'neighbours': ['C', 'A', 'B', 'B']},
                       {'scene_name': 'B', 'neighbours': ['C']},
                       {'scene_name': 'C', 'neighbours': ['B']}],
             'edges': [{'source_scene': 'A', 'target_scene': 'B', 'source_id': 'gate2'},
                       {'source_scene': 'A', 'target_scene': 'B', 'source_id': 'gate1'}]}
    return inventory, graph


class ResidencyPlanTests(unittest.TestCase):
    def test_new_fragments_in_shared_atlas_are_not_mistaken_for_cache_hits(self):
        inventory = {
            'rooms': [{'scene_name': 'A', 'texture_ids': ['atlas', 'material'], 'sprite_ids': ['one']},
                      {'scene_name': 'B', 'texture_ids': ['atlas', 'material'], 'sprite_ids': ['two']},
                      {'scene_name': 'C', 'texture_ids': ['atlas'], 'sprite_ids': ['three']}],
            'textures': {'atlas': {}, 'material': {}},
            'sprites': {key: {'backing_texture_ids': ['atlas'], 'cooked_ram_bytes': 10,
                              'cooked_vram_bytes': 20} for key in ['one', 'two', 'three']},
        }
        graph = {'rooms': [{'scene_name': 'A', 'neighbours': ['B']},
                           {'scene_name': 'B', 'neighbours': ['A', 'C']},
                           {'scene_name': 'C', 'neighbours': ['B']}]}
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        self.assertEqual(plan['planning_unit'], 'sprite_fragments')
        self.assertEqual(plan['backing_texture_union'], ['atlas', 'material'])
        self.assertEqual(plan['resource_union'], ['one', 'texture:material', 'two'])
        target = plan['transitions'][0]
        self.assertEqual(target['fetch_window_resource_ids'], ['three'])
        # A is still B's neighbour: its sprite stays pinned after crossing.
        self.assertEqual(target['candidate_release_resource_ids'], [])
        self.assertIsNone(target['release_after_window_change_resource_ids'])
        self.assertEqual(target['source_room_only_resource_ids'], ['one'])
        self.assertEqual(plan['material_texture_dependencies'], {'A': ['material'], 'B': ['material']})
        self.assertEqual(plan['budgets']['ram']['unknown_resource_ids'], ['texture:material'])

    def test_shared_atlas_is_fetched_once_and_current_is_first(self):
        inventory, graph = fixture()
        plan = plan_inventory(inventory, graph, 'A', 40, 80)
        self.assertEqual(plan['resource_union'], ['a', 'b', 'c', 'shared'])
        self.assertEqual([(item['priority'], item['resource_id']) for item in plan['fetch_order']],
                         [(0, 'a'), (0, 'shared'), (1, 'b'), (1, 'c')])
        self.assertEqual(plan['fetch_order'][1]['required_by'], ['A', 'B', 'C'])
        target = plan['transitions'][0]
        self.assertEqual(target['keep_resource_ids'], ['shared'])
        self.assertEqual(target['fetch_resource_ids'], ['b'])
        self.assertEqual(target['source_room_only_resource_ids'], ['a'])
        self.assertEqual(plan['budgets']['ram']['exact_bytes'], 40)
        self.assertEqual(plan['budgets']['vram']['exact_bytes'], 80)
        self.assertFalse(plan['full_room_fit_proven'])

    def test_directed_gate_plan_preserves_gate_identity_and_safe_eviction(self):
        inventory, graph = fixture()
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        self.assertEqual(plan['version'], 2)
        edges = plan['directed_edges']
        self.assertEqual([edge['source_id'] for edge in edges], ['gate1', 'gate2'])
        for edge in edges:
            self.assertEqual(edge['target_scene'], 'B')
            self.assertEqual(edge['fetch_before_gate_crossing_resource_ids'], [])
            self.assertEqual(edge['keep_during_transition_resource_ids'], ['b', 'c', 'shared'])
            self.assertEqual(edge['evict_after_target_admission_resource_ids'], ['a'])
            self.assertEqual(edge['fetch_deadline'], 'before_gate_crossing')
            self.assertEqual(edge['publish_deadline'], 'before_target_first_draw')

    def test_directed_gate_with_incomplete_target_cannot_evict_source(self):
        inventory, graph = fixture()
        inventory['rooms'][1]['scan_failed'] = True
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        edge = plan['directed_edges'][0]
        self.assertEqual(edge['target_window_admission'], 'unadmitted')
        self.assertIsNone(edge['evict_after_target_admission_resource_ids'])
        self.assertEqual(edge['eviction_rule'], 'hold_until_target_dependencies_are_complete')

    def test_source_estimates_never_become_cooked_costs(self):
        inventory, graph = fixture()
        inventory['textures']['b'] = {'serialized_bytes': 1, 'source_size_4bpp_estimate_bytes': 1}
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        for budget in plan['budgets'].values():
            self.assertEqual(budget['status'], 'unknown')
            self.assertEqual(budget['unknown_resource_ids'], ['b'])
            self.assertIsNone(budget['exact_bytes'])
        self.assertEqual(len(plan['fetch_order']), 4)
        self.assertEqual(plan['admission'], 'unadmitted')

    def test_crossing_reconciles_the_whole_neighbour_window(self):
        inventory, graph = fixture()
        inventory['rooms'].append({'scene_name': 'D', 'texture_ids': ['d', 'shared']})
        inventory['textures']['d'] = {'cooked_ram_bytes': 10, 'cooked_vram_bytes': 20}
        graph['rooms'][1]['neighbours'].append('D')
        graph['rooms'].append({'scene_name': 'D', 'neighbours': ['B']})
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        transition = plan['transitions'][0]
        self.assertEqual(transition['target_window_rooms'], ['B', 'C', 'D'])
        self.assertEqual(transition['fetch_window_resource_ids'], ['d'])
        self.assertEqual(transition['keep_window_resource_ids'], ['b', 'c', 'shared'])
        self.assertEqual(transition['release_after_window_change_resource_ids'], ['a'])
        self.assertEqual(transition['target_window_admission'], 'admitted_for_listed_resource_costs')
        self.assertEqual(len(plan_all(inventory, graph)['plans']), 4)

    def test_known_overflow_is_reported_without_dropping_neighbors(self):
        inventory, graph = fixture()
        inventory['textures']['b']['cooked_vram_bytes'] = None
        plan = plan_inventory(inventory, graph, 'A', 30, 40)
        self.assertEqual(plan['budgets']['ram']['status'], 'over_budget')
        self.assertEqual(plan['budgets']['vram']['status'], 'over_budget')
        self.assertEqual(plan['selected_rooms'], ['A', 'B', 'C'])
        self.assertEqual(len(plan['fetch_order']), 4)

    def test_missing_inventory_and_texture_record_are_explicit(self):
        inventory, graph = fixture()
        inventory['rooms'].pop()
        del inventory['textures']['b']
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        self.assertEqual(plan['missing_room_inventories'], ['C'])
        self.assertEqual(plan['missing_resource_records'], ['b'])
        self.assertIsNone(plan['transitions'][1]['fetch_resource_ids'])
        self.assertIsNone(plan['budgets']['ram']['exact_bytes'])
        self.assertEqual(plan['budgets']['ram']['status'], 'unknown')
        self.assertIsNone(plan['transitions'][0]['release_after_window_change_resource_ids'])

    def test_plan_is_stable_under_input_order_changes(self):
        inventory, graph = fixture()
        expected = plan_inventory(inventory, graph, 'A', 1000, 1000)
        inventory['rooms'].reverse()
        for room in inventory['rooms']:
            room['texture_ids'].reverse()
        graph['rooms'][0]['neighbours'].reverse()
        graph['edges'].reverse()
        self.assertEqual(expected, plan_inventory(inventory, graph, 'A', 1000, 1000))

    def test_invalid_cost_and_ambiguous_scene_fail_clearly(self):
        inventory, graph = fixture()
        inventory['textures']['a']['cooked_ram_bytes'] = -1
        with self.assertRaisesRegex(ValueError, 'nonnegative'):
            plan_inventory(inventory, graph, 'A')
        inventory, graph = fixture()
        inventory['rooms'].append(copy.deepcopy(inventory['rooms'][0]))
        with self.assertRaisesRegex(ValueError, 'ambiguous'):
            plan_inventory(inventory, graph, 'A')

    def test_failed_scan_never_means_an_empty_known_room(self):
        inventory, graph = fixture()
        inventory['rooms'][1].update(scan_failed=True, texture_ids=[])
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        self.assertEqual(plan['incomplete_room_inventories'], ['B'])
        self.assertEqual(plan['admission'], 'unadmitted')
        self.assertIsNone(plan['transitions'][0]['release_after_window_change_resource_ids'])

    def test_unresolved_behavior_prevents_executable_eviction(self):
        inventory, graph = fixture()
        inventory['rooms'][1]['unresolved_behavior_types'] = {
            'DynamicFixture': {'count': 2, 'source_ids': ['private1', 'private2']}}
        plan = plan_inventory(inventory, graph, 'A', 1000, 1000)
        target = plan['transitions'][0]
        self.assertEqual(target['candidate_release_resource_ids'], ['a'])
        self.assertIsNone(target['release_after_window_change_resource_ids'])
        self.assertEqual(plan['unresolved_behavior_counts'], {'B': {'DynamicFixture': 2}})
        self.assertEqual(plan['admission'], 'unadmitted')


if __name__ == '__main__':
    unittest.main()
