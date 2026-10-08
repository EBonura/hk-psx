import copy
import math
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from runner import axis_aligned_bounds, body_contract, clip_contract, walker_parameters, CLIPS


def walker():
    return dict(walkSpeedL=-1.5, walkSpeedR=1.5, rightScale=-1., edgeXAdjuster=0.,
                turnPause=1., turnAfterIdlePercentage=0, pauseWaitMin=4.,
                pauseWaitMax=1.5, pauseTimeMin=2.5, pauseTimeMax=1.5, pauses=1,
                idleClip='Idle', walkClip='Walk', turnClip='Turn', ambush=0,
                startInactive=0, waitForHeroX=0, preventTurn=0, ignoreHoles=0,
                preventTurningToFaceHero=0, preventScaleChange=0, m_Enabled=1)


class RunnerSourceTests(unittest.TestCase):
    def test_body_requires_source_gravity_rotation_lock_and_terrain_filters(self):
        body = dict(m_BodyType=0, m_Simulated=True, m_UseAutoMass=False,
                    m_UseFullKinematicContacts=False, m_Mass=1., m_LinearDamping=0.,
                    m_GravityScale=1., m_Interpolate=0, m_SleepingMode=1,
                    m_CollisionDetection=0, m_Constraints=4,
                    m_Material={'m_FileID': 0, 'm_PathID': 0},
                    m_IncludeLayers={'m_Bits': 0}, m_ExcludeLayers={'m_Bits': 0})
        body_contract(body)
        for key, changed in [('m_GravityScale', 0.5), ('m_Constraints', 0),
                             ('m_LinearDamping', .1), ('m_Mass', math.nan),
                             ('m_BodyType', 2), ('m_Simulated', False),
                             ('m_ExcludeLayers', {'m_Bits': 256}),
                             ('m_Material', {'m_FileID': 0, 'm_PathID': 1})]:
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, key):
                body_contract(dict(body, **{key: changed}))

    def test_supported_walker_preserves_reversed_pause_arguments(self):
        result = walker_parameters(walker())
        self.assertEqual(result['walking_wait_endpoints_ticks'], [240, 90])
        self.assertEqual(result['pause_endpoints_ticks'], [150, 90])
        self.assertEqual(result['walk_velocity_q16'], [-98304, 98304])
        self.assertEqual(result['initial_direction'], -1)

    def test_movement_variants_are_not_silently_classified_as_current_runner(self):
        for key in ('ambush', 'startInactive', 'waitForHeroX', 'preventTurn',
                    'ignoreHoles', 'preventTurningToFaceHero', 'preventScaleChange'):
            changed = walker(); changed[key] = 1
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, key):
                walker_parameters(changed)
        for key, value in [('walkSpeedL', -2), ('pauseWaitMin', 0.),
                           ('rightScale', 1), ('walkClip', 'Other'), ('m_Enabled', 0),
                           ('edgeXAdjuster', .1), ('turnPause', math.nan)]:
            changed = walker(); changed[key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, key):
                walker_parameters(changed)
        with self.assertRaisesRegex(ValueError, 'lunge speed'):
            walker_parameters(walker(), 0.)

    def test_barger_and_hornhead_placements_become_parameters(self):
        barger = walker(); barger.update(pauseWaitMin=1.5, pauseWaitMax=4., pauseTimeMin=1.)
        result = walker_parameters(barger, 14.)
        self.assertEqual(result['walking_wait_endpoints_ticks'], [240, 90])
        self.assertEqual(result['pause_endpoints_ticks'], [90, 60])
        self.assertEqual(result['lunge_velocity_q16'], [-917504, 917504])
        hornhead = walker(); hornhead.update(walkSpeedL=-2.5, walkSpeedR=2.5, pauseWaitMin=4.5, pauseWaitMax=2.)
        result = walker_parameters(hornhead, 9.)
        self.assertEqual(result['walk_velocity_q16'], [-163840, 163840])
        self.assertEqual(result['walking_wait_endpoints_ticks'], [270, 120])

    def test_clip_clock_wraps_and_event_rejection(self):
        clips = [{'name': name, 'fps': fps, 'wrapMode': wrap,
                  'loopStart': 1 if name == 'Fall' else 0,
                  'frames': [{'triggerEvent': False} for _ in range(count)]}
                 for name, (count, fps, wrap) in CLIPS.items()]
        contract = {c['name']: c for c in clip_contract(clips)}
        self.assertEqual([contract[n]['nominal_duration_ticks'] for n in
                          ['Attack Anticipate', 'Attack Lunge', 'Attack Cooldown']], [25, 40, 5])
        self.assertEqual(contract['Fall']['loop_start'], 1)
        # Frame counts and rates are per variant (Barger Attack Lunge is 2@10);
        # the wrap modes, events and loop starts stay contracted.
        for change in ('event', 'wrap', 'duplicate', 'missing', 'loop', 'extra'):
            bad = copy.deepcopy(clips)
            if change == 'event':bad[0]['frames'][0]['triggerEvent'] = True
            if change == 'wrap':bad[0]['wrapMode'] = 2
            if change == 'duplicate':bad.append(bad[0])
            if change == 'missing':bad.pop(0)
            if change == 'loop':bad[0]['loopStart'] = 100
            if change == 'extra':bad.append(dict(bad[0], name='Other'))
            with self.subTest(change=change), self.assertRaises(ValueError):clip_contract(bad)
        without_fall = [c for c in clips if c['name'] != 'Fall']
        self.assertEqual(len(clip_contract(without_fall)), len(CLIPS) - 1)

    def test_child_scale_offset_and_mirror_preserve_full_alert_range(self):
        matrix = [[11.14, 0, 0, 42], [0, 3.45, 0, 2 - .48], [0, 0, 1, 0], [0, 0, 0, 1]]
        box = axis_aligned_bounds(matrix, [0, -.15228271], [1, .69543451])
        expected = [42 - 5.57, 2 - 2.205, 42 + 5.57, 2 + .19424918]
        for a, b in zip(box, expected):self.assertAlmostEqual(a, b, places=6)
        matrix[0][0] *= -1
        self.assertEqual(axis_aligned_bounds(matrix, [0, -.15228271], [1, .69543451]), box)
        body = [[-1, 0, 0, 10], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]
        self.assertEqual(axis_aligned_bounds(body, [-.25, 0], [1, 2]), [9.75, -1, 10.75, 1])

    def test_unsupported_geometry_is_explicit(self):
        base = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]
        for r, c, v in [(0, 1, .5), (0, 0, 0), (1, 3, math.inf)]:
            matrix = copy.deepcopy(base); matrix[r][c] = v
            with self.assertRaises(ValueError):axis_aligned_bounds(matrix, [0, 0], [1, 1])
        with self.assertRaises(ValueError):axis_aligned_bounds(base, [math.nan, 0], [1, 1])
        with self.assertRaises(ValueError):axis_aligned_bounds(base, [0, 0], [-1, 1])


if __name__ == '__main__':unittest.main()
