"""Measured scenery quality and stable development-region residency layout.

Only static SpriteRenderer scenery uses this cap. Knight/nail/enemy animation
sampling and HUD quality remain unchanged. The first 98 region IDs remain
stable. Eight Town views append full supported scenery/collision coverage;
spatial partitions do not implement source scripts.
"""
# Scene catalog: every cooker, generated table and guest constant derives its
# scene list from here. Runtime/camera envelopes follow tools/cook_scene_pack.py.
SCENE_TABLE = (
    {'scene_id': 0, 'scene_name': 'Tutorial_01', 'file': 'level6',
     'runtime_bounds': [0,-5,210,100], 'camera_global_bounds': [14,0,196,92]},
    {'scene_id': 1, 'scene_name': 'Town', 'file': 'level7',
     'runtime_bounds': [0,-5,270,76], 'camera_global_bounds': [12,15,258,68]},
    {'scene_id': 2, 'scene_name': 'Crossroads_01', 'file': 'level37',
     'runtime_bounds': [-2,-5,102,49], 'camera_global_bounds': [0,0,100,42]},
    {'scene_id': 3, 'scene_name': 'Crossroads_02', 'file': 'level38',
     'runtime_bounds': [-2,-5,92,36], 'camera_global_bounds': [0,0,90,30]},
    {'scene_id': 4, 'scene_name': 'Crossroads_07', 'file': 'level43',
     'runtime_bounds': [-2,-5,45,116], 'camera_global_bounds': [0,0,43,110]},
)
SCENE_COUNT = len(SCENE_TABLE)
SCENE_FILES = tuple(scene['file'] for scene in SCENE_TABLE)
SCENERY_MAX_AXIS = 48
STATIC_PAGE_BUDGET = 5
TEXTURE_BUDGET = 416  # Four disjoint CLUT banks; see hk-cache::residency.
ROOM_BYTE_BUDGET = 384 * 1024
REGION_LAYOUT = (
    (0, (15, -5, 62, 25)),
    (0, (48, 11, 72, 27)),
    (0, (0, -5, 24, 11)),
    (0, (48, -5, 72, 11)),
    (0, (72, -5, 96, 11)),
    (0, (96, -5, 120, 11)),
    (0, (120, -5, 144, 11)),
    (0, (144, -5, 168, 11)),
    (0, (168, -5, 192, 11)),
    (0, (192, -5, 210, 11)),
    (0, (0, 11, 24, 27)),
    (0, (24, 11, 48, 27)),
    (0, (72, 11, 84.0, 27)),
    (0, (84.0, 11, 96, 27)),
    (0, (96, 11, 108.0, 27)),
    (0, (108.0, 11, 120, 19.0)),
    (0, (108.0, 19.0, 120, 27)),
    (0, (120, 11, 132.0, 19.0)),
    (0, (120, 19.0, 132.0, 27)),
    (0, (132.0, 11, 144, 19.0)),
    (0, (132.0, 19.0, 144, 27)),
    (0, (144, 11, 156.0, 27)),
    (0, (156.0, 11, 168, 27)),
    (0, (168, 11, 192, 27)),
    (0, (192, 11, 210, 27)),
    (0, (0, 27, 24, 43)),
    (0, (24, 27, 48, 43)),
    (0, (48, 27, 72, 43)),
    (0, (72, 27, 84.0, 43)),
    (0, (84.0, 27, 96, 43)),
    (0, (96, 27, 108.0, 43)),
    (0, (108.0, 27, 120, 43)),
    (0, (120, 27, 132.0, 35.0)),
    (0, (120, 35.0, 132.0, 43)),
    (0, (132.0, 27, 144, 35.0)),
    (0, (132.0, 35.0, 144, 43)),
    (0, (144, 27, 156.0, 35.0)),
    (0, (144, 35.0, 156.0, 43)),
    (0, (156.0, 27, 168, 43)),
    (0, (168, 27, 192, 43)),
    (0, (192, 27, 210, 43)),
    (0, (0, 43, 24, 59)),
    (0, (24, 43, 48, 59)),
    (0, (48, 43, 60.0, 59)),
    (0, (60.0, 43, 72, 51.0)),
    (0, (60.0, 51.0, 72, 59)),
    (0, (72, 43, 84.0, 59)),
    (0, (84.0, 43, 96, 59)),
    (0, (96, 43, 108.0, 51.0)),
    (0, (96, 51.0, 108.0, 59)),
    (0, (108.0, 43, 120, 51.0)),
    (0, (108.0, 51.0, 120, 59)),
    (0, (120, 43, 132.0, 51.0)),
    (0, (120, 51.0, 132.0, 59)),
    (0, (132.0, 43, 144, 59)),
    (0, (144, 43, 156.0, 59)),
    (0, (156.0, 43, 168, 59)),
    (0, (168, 43, 192, 59)),
    (0, (192, 43, 210, 59)),
    (0, (0, 59, 24, 75)),
    (0, (24, 59, 48, 75)),
    (0, (48, 59, 72, 75)),
    (0, (72, 59, 96, 75)),
    (0, (96, 59, 120, 75)),
    (0, (120, 59, 144, 75)),
    (0, (144, 59, 168, 75)),
    (0, (168, 59, 192, 75)),
    (0, (192, 59, 210, 75)),
    (0, (0, 75, 24, 91)),
    (0, (24, 75, 48, 91)),
    (0, (48, 75, 72, 91)),
    (0, (72, 75, 96, 91)),
    (0, (96, 75, 120, 91)),
    (0, (120, 75, 144, 91)),
    (0, (144, 75, 168, 91)),
    (0, (168, 75, 192, 91)),
    (0, (192, 75, 210, 91)),
    (0, (0, 91, 24, 100)),
    (0, (24, 91, 48, 100)),
    (0, (48, 91, 72, 100)),
    (0, (72, 91, 96, 100)),
    (0, (96, 91, 120, 100)),
    (0, (120, 91, 144, 100)),
    (0, (144, 91, 168, 100)),
    (0, (168, 91, 192, 100)),
    (0, (192, 91, 210, 100)),
    (1, (0, -5, 24, 11)),
    (1, (24, -5, 48, 11)),
    (1, (0, 11, 24, 27)),
    (1, (24, 11, 48, 27)),
    (1, (0, 27, 24, 43)),
    (1, (24, 27, 48, 43)),
    (1, (0, 43, 24, 59)),
    (1, (24, 43, 48, 59)),
    (1, (0, 59, 24, 75)),
    (1, (24, 59, 48, 75)),
    (1, (0, 75, 24, 76)),
    (1, (24, 75, 48, 76)),
)

# These measured views cover the rest of Town without changing the original
# first 98 chunk IDs. Cameras describe extraction coverage, not source lock FSMs.
# Lower camera tops reach activation top+2 so descending the entrance hill
# crosses into these views continuously with the fixed player camera offset.
def grid_layout(runtime_bounds, step=(24, 16)):
    """The measured 24x16 view stepping over a scene's runtime envelope."""
    left, bottom, right, top = runtime_bounds
    return tuple([x, y, min(x + step[0], right), min(y + step[1], top)]
                 for y in range(bottom, top, step[1]) for x in range(left, right, step[0]))
# Scenes after the two hand-tuned ones use the grid over their catalog envelope.
GRID_SCENE_LAYOUTS = {scene['scene_id']: grid_layout(scene['runtime_bounds']) for scene in SCENE_TABLE[2:]}
TOWN_EXTENSION_LAYOUT = (
    ((48, -5, 96, 40), (40, 15, 96, 42)),
    ((96, -5, 168, 40), (96, 15, 168, 42)),
    ((168, -5, 216, 40), (168, 15, 216, 42)),
    ((216, -5, 270, 40), (216, 15, 258, 42)),
    ((216, 40, 270, 76), (216, 32, 258, 68)),
    ((48, 40, 96, 76), (48, 32, 96, 68)),
    ((96, 40, 168, 76), (96, 32, 168, 68)),
    ((168, 40, 216, 76), (168, 32, 216, 68)),
)
