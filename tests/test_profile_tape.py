"""Continuity gates must reject blocked traversal, including recovered errors."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('profile_tape', Path(__file__).resolve().parents[1] / 'tools/profile_tape.py')
PROFILE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROFILE)


def clean_result():
    return {
        'final_ram': {'HK_GAME_MODE': 1},
        'optional_diagnostics': {
            name: {'final': value, 'maximum_observed': value}
            for name, value in [('HK_BOUNDARY_WAIT_TICKS', 0), ('HK_BOUNDARY_WAIT_MAX', 0),
                                ('HK_PAD_POLL_MAX_VBLANK_GAP', 1), ('HK_CD_STREAM_ERROR', 0),
                                ('HK_CD_DISCARDED_SECTORS', 0)]
        },
        'observed_error_maxima': {'__psx_rt_fault_count': 0, 'HK_ROOM_LOAD_ERROR': 0, 'HK_CD_STREAM_ERROR': 0},
        'observed_poll_gaps': {'max_route_ticks': 1},
        'poll_vblank_phase': {'lag_variation': 1},
        'frame_intervals': [
            {'route_ticks': 2, 'from_region': 1, 'to_region': 1, 'sectors_read': 12},
            {'route_ticks': 2, 'from_region': 1, 'to_region': 2, 'sectors_read': 2},
            {'route_ticks': 2, 'from_region': 2, 'to_region': 2, 'sectors_read': 0},
        ],
    }


class SeamlessGateTests(unittest.TestCase):
    def test_default_requires_30fps_even_if_only_one_frame_misses(self):
        result = clean_result()
        result['frame_intervals'][2]['route_ticks'] = 3
        self.assertTrue(any('Frame interval exceeded 2' in failure for failure in PROFILE.seamless_failures(result)))

    def test_actual_sampler_faults_or_missed_observations_fail(self):
        for name in ('HK_INPUT_MISSED_VBLANKS','HK_INPUT_FAULT'):
            result=clean_result()
            result['optional_diagnostics'][name]={'final':0,'maximum_observed':1}
            self.assertTrue(any(name in failure for failure in PROFILE.seamless_failures(result)))

    def test_background_cd_activity_with_continuous_polls_passes(self):
        self.assertEqual(PROFILE.seamless_failures(clean_result()), [])

    def test_absent_metrics_do_not_claim_zero_wait(self):
        result = clean_result()
        del result['optional_diagnostics']['HK_BOUNDARY_WAIT_TICKS']
        self.assertTrue(any('Missing' in failure for failure in PROFILE.seamless_failures(result)))

    def test_any_boundary_wait_fails_even_if_rendering_continues(self):
        result = clean_result()
        result['optional_diagnostics']['HK_BOUNDARY_WAIT_TICKS']['maximum_observed'] = 1
        self.assertTrue(any('HK_BOUNDARY_WAIT_TICKS' in failure for failure in PROFILE.seamless_failures(result)))

    def test_raw_poll_gap_catches_counter_reset_at_activation(self):
        result = clean_result()
        result['observed_poll_gaps']['max_route_ticks'] = 5
        self.assertTrue(any('Raw observed poll gap' in failure for failure in PROFILE.seamless_failures(result)))

    def test_two_sampled_ticks_are_not_a_missed_poll_when_vblank_phase_is_bounded(self):
        result = clean_result()
        result['observed_poll_gaps']['max_route_ticks'] = 2
        self.assertEqual(PROFILE.seamless_failures(result), [])

    def test_missing_phase_evidence_does_not_waive_raw_gap(self):
        result = clean_result()
        result['observed_poll_gaps']['max_route_ticks'] = 2
        del result['poll_vblank_phase']
        self.assertTrue(any('Missing independent' in failure for failure in PROFILE.seamless_failures(result)))

    def test_poll_lag_growth_fails_even_when_guest_counter_claims_one(self):
        result = clean_result()
        result['poll_vblank_phase']['lag_variation'] = 2
        self.assertTrue(any('lag variation' in failure for failure in PROFILE.seamless_failures(result)))

    def test_activation_extra_tick_fails_even_within_overall_budget(self):
        result = clean_result()
        result['frame_intervals'][1]['route_ticks'] = 4
        self.assertTrue(any('Activation frame' in failure for failure in PROFILE.seamless_failures(result, 4)))

    def test_ordinary_hitch_still_fails_frame_budget(self):
        result = clean_result()
        result['frame_intervals'][2]['route_ticks'] = 8
        self.assertTrue(any('Frame interval' in failure for failure in PROFILE.seamless_failures(result, 4)))

    def test_recovered_error_still_fails(self):
        result = clean_result()
        result['observed_error_maxima']['HK_ROOM_LOAD_ERROR'] = 6
        self.assertTrue(any('HK_ROOM_LOAD_ERROR' in failure for failure in PROFILE.seamless_failures(result)))

    def test_stationary_route_cannot_establish_traversal(self):
        result = clean_result()
        result['frame_intervals'][1]['to_region'] = 1
        self.assertTrue(any('No region activation' in failure for failure in PROFILE.seamless_failures(result)))

    def test_no_samples_and_missing_errors_fail_closed(self):
        self.assertGreater(len(PROFILE.seamless_failures({})), 5)

    def test_frame_budget_cannot_be_disabled(self):
        with self.assertRaises(ValueError):
            PROFILE.seamless_failures(clean_result(), 0)


if __name__ == '__main__':
    unittest.main()
