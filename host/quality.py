"""Measured scenery quality and stable development-region residency layout.

Only static SpriteRenderer scenery uses this cap. Knight/nail/enemy animation
sampling and HUD quality remain unchanged. The first 98 region IDs remain
stable. Eight Town views append full supported scenery/collision coverage;
spatial partitions do not implement source scripts.
"""
# Scene catalog: every cooker, generated table and guest constant derives its
# scene list from here. Runtime/camera envelopes follow tools/cook_scene_pack.py.
#
# Before adding a row, measure the candidate with tools/cook_scene_pack.py and
# read its per-view numbers: the 416-slot TEXTURE_BUDGET below is per view, so a
# scene's answer is its worst view, not its total. Appending keeps every existing
# chunk id, because fixed_regions() lays the grid scenes out in scene_id order
# and cook_fingerprints() deliberately leaves this file out of the cache key.
#
# Room_shop (level17), Sly's shop and the only scene that makes P17's purchases
# reachable, is measured and fits with room to spare: 6 views, worst view 90 of
# 416 CLUT slots, 1 of 5 pages, 70,372 of 393,216 room bytes, a 71,656-byte
# scene bank, decoder and packet bound PASS, 7.8 s to cook. Its way in is
# Dirtmouth's door_sly, whose serialized targetScene is empty because its Door
# Control FSM holds the destination; `regions.door_destination` reads it, so the
# gate cooks once this row is in. See out_of_reach in .hkpsx/shop-catalog.json.
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
    {'scene_id': 5, 'scene_name': 'Crossroads_03', 'file': 'level39',
     'runtime_bounds': [-2, -6, 32, 77], 'camera_global_bounds': [0, 0, 30, 72]},
    {'scene_id': 6, 'scene_name': 'Crossroads_04', 'file': 'level40',
     'runtime_bounds': [-2, -5, 162, 32], 'camera_global_bounds': [0, 0, 160, 30]},
    {'scene_id': 7, 'scene_name': 'Crossroads_05', 'file': 'level41',
     'runtime_bounds': [-2, -5, 77, 26], 'camera_global_bounds': [0, 0, 75, 25]},
    {'scene_id': 8, 'scene_name': 'Crossroads_06', 'file': 'level42',
     'runtime_bounds': [-2, -5, 62, 61], 'camera_global_bounds': [0, 0, 60, 60]},
    {'scene_id': 9, 'scene_name': 'Crossroads_08', 'file': 'level44',
     'runtime_bounds': [-2, -5, 54, 44], 'camera_global_bounds': [0, 0, 52, 43]},
    {'scene_id': 10, 'scene_name': 'Crossroads_09', 'file': 'level45',
     'runtime_bounds': [-2, -5, 88, 26], 'camera_global_bounds': [0, 0, 86, 25]},
    {'scene_id': 11, 'scene_name': 'Crossroads_10', 'file': 'level46',
     'runtime_bounds': [-2, -5, 77, 69], 'camera_global_bounds': [0, 0, 75, 68]},
    {'scene_id': 12, 'scene_name': 'Crossroads_12', 'file': 'level51',
     'runtime_bounds': [-2, -5, 72, 25], 'camera_global_bounds': [0, 0, 70, 24]},
    {'scene_id': 13, 'scene_name': 'Crossroads_13', 'file': 'level52',
     'runtime_bounds': [-2, -5, 82, 48], 'camera_global_bounds': [0, 0, 80, 47]},
    {'scene_id': 14, 'scene_name': 'Crossroads_14', 'file': 'level53',
     'runtime_bounds': [-2, -5, 35, 49], 'camera_global_bounds': [0, 0, 33, 48]},
    {'scene_id': 15, 'scene_name': 'Crossroads_16', 'file': 'level55',
     'runtime_bounds': [-2, -6, 78, 28], 'camera_global_bounds': [0, 0, 76, 27]},
    {'scene_id': 16, 'scene_name': 'Crossroads_18', 'file': 'level56',
     'runtime_bounds': [-1, -6, 43, 51], 'camera_global_bounds': [0, 0, 41, 50]},
    {'scene_id': 17, 'scene_name': 'Crossroads_19', 'file': 'level57',
     'runtime_bounds': [-2, -5, 52, 50], 'camera_global_bounds': [0, 0, 50, 45]},
    {'scene_id': 18, 'scene_name': 'Crossroads_21', 'file': 'level58',
     'runtime_bounds': [-2, -5, 102, 34], 'camera_global_bounds': [0, 0, 100, 29]},    {'scene_id': 19, 'scene_name': 'Crossroads_11_alt', 'file': 'level50',
     'runtime_bounds': [-2, -5, 122, 37], 'camera_global_bounds': [0, 0, 120, 36]},
    {'scene_id': 20, 'scene_name': 'Crossroads_15', 'file': 'level54',
     'runtime_bounds': [-2, -5, 62, 19], 'camera_global_bounds': [0, 0, 60, 18]},
    {'scene_id': 21, 'scene_name': 'Crossroads_22', 'file': 'level59',
     'runtime_bounds': [-1, -6, 106, 36], 'camera_global_bounds': [0, 0, 105, 35]},
    {'scene_id': 22, 'scene_name': 'Crossroads_25', 'file': 'level60',
     'runtime_bounds': [-2, -5, 72, 28], 'camera_global_bounds': [0, 0, 70, 27]},
    {'scene_id': 23, 'scene_name': 'Crossroads_27', 'file': 'level61',
     'runtime_bounds': [-2, -6, 32, 73], 'camera_global_bounds': [0, 0, 30, 72]},
    {'scene_id': 24, 'scene_name': 'Crossroads_30', 'file': 'level62',
     'runtime_bounds': [-1, -5, 51, 24], 'camera_global_bounds': [0, 0, 50, 23]},
    {'scene_id': 25, 'scene_name': 'Crossroads_31', 'file': 'level63',
     'runtime_bounds': [-1, -5, 71, 24], 'camera_global_bounds': [0, 0, 69, 23]},
    {'scene_id': 26, 'scene_name': 'Crossroads_33', 'file': 'level64',
     'runtime_bounds': [-2, -5, 47, 54], 'camera_global_bounds': [0, 0, 45, 50]},
    {'scene_id': 27, 'scene_name': 'Crossroads_35', 'file': 'level65',
     'runtime_bounds': [-1, -6, 72, 76], 'camera_global_bounds': [0, 0, 70, 75]},
    {'scene_id': 28, 'scene_name': 'Crossroads_36', 'file': 'level66',
     'runtime_bounds': [-1, -5, 62, 59], 'camera_global_bounds': [0, 0, 60, 58]},
    {'scene_id': 29, 'scene_name': 'Crossroads_37', 'file': 'level67',
     'runtime_bounds': [-1, -5, 112, 31], 'camera_global_bounds': [0, 0, 110, 30]},
    {'scene_id': 30, 'scene_name': 'Crossroads_38', 'file': 'level68',
     'runtime_bounds': [-1, -5, 71, 26], 'camera_global_bounds': [0, 0, 70, 25]},
    {'scene_id': 31, 'scene_name': 'Crossroads_39', 'file': 'level69',
     'runtime_bounds': [-2, -5, 90, 26], 'camera_global_bounds': [0, 0, 88, 25]},
    {'scene_id': 32, 'scene_name': 'Crossroads_40', 'file': 'level70',
     'runtime_bounds': [-2, -5, 90, 26], 'camera_global_bounds': [0, 0, 88, 25]},
    {'scene_id': 33, 'scene_name': 'Crossroads_42', 'file': 'level71',
     'runtime_bounds': [-2, -5, 112, 26], 'camera_global_bounds': [0, 0, 110, 25]},
    {'scene_id': 34, 'scene_name': 'Crossroads_43', 'file': 'level72',
     'runtime_bounds': [-2, -5, 90, 26], 'camera_global_bounds': [0, 0, 88, 25]},
    {'scene_id': 35, 'scene_name': 'Crossroads_45', 'file': 'level73',
     'runtime_bounds': [-2, -5, 72, 51], 'camera_global_bounds': [0, 0, 70, 50]},
    {'scene_id': 36, 'scene_name': 'Crossroads_46', 'file': 'level74',
     'runtime_bounds': [-2, -5, 56, 26], 'camera_global_bounds': [0, 0, 55, 25]},
    {'scene_id': 37, 'scene_name': 'Crossroads_46b', 'file': 'level75',
     'runtime_bounds': [-1, -5, 57, 26], 'camera_global_bounds': [0, 0, 55, 25]},
    {'scene_id': 38, 'scene_name': 'Crossroads_47', 'file': 'level77',
     'runtime_bounds': [-1, -5, 49, 20], 'camera_global_bounds': [0, 0, 47, 19]},
    {'scene_id': 39, 'scene_name': 'Crossroads_48', 'file': 'level78',
     'runtime_bounds': [-2, -5, 58, 20], 'camera_global_bounds': [0, 0, 57, 19]},
    {'scene_id': 40, 'scene_name': 'Crossroads_49', 'file': 'level79',
     'runtime_bounds': [-2, -5, 32, 177], 'camera_global_bounds': [0, 0, 30, 176]},
    {'scene_id': 41, 'scene_name': 'Crossroads_49b', 'file': 'level80',
     'runtime_bounds': [-2, -5, 32, 87], 'camera_global_bounds': [0, 0, 30, 86]},
    {'scene_id': 42, 'scene_name': 'Crossroads_50', 'file': 'level81',
     'runtime_bounds': [-2, -5, 262, 61], 'camera_global_bounds': [0, 0, 260, 60]},
    {'scene_id': 43, 'scene_name': 'Crossroads_52', 'file': 'level82',
     'runtime_bounds': [-2, -5, 56, 71], 'camera_global_bounds': [0, 0, 55, 70]},
    {'scene_id': 44, 'scene_name': 'Crossroads_ShamanTemple', 'file': 'level76',
     'runtime_bounds': [-2, -5, 143, 76], 'camera_global_bounds': [0, 0, 142, 75]},
    # Sly's shop, reached through Dirtmouth's door_sly. Envelope and camera from
    # tools/cook_scene_pack.py; the 24x16 grid over it is the same six views that
    # measurement cooked, so its numbers carry over unchanged.
    #
    # The row itself, verified and held back only because admitting a 46th scene
    # is not an append. Paste it back as the last entry, never anywhere else:
    #
    #   {'scene_id': 45, 'scene_name': 'Room_shop', 'file': 'level17',
    #    'runtime_bounds': [-1, -5, 34, 31], 'camera_global_bounds': [0, 0, 33, 30]},
    #
    # What has to happen with it, in this order, because each is keyed on the
    # scene table and will otherwise raise or index off the end:
    #   1. host/cook_music.py, whose provenance covers 45 scenes and which
    #      host/ambience.py checks against.
    #   2. host/ambience.py, whose AMBIENCE_SCENES the guest indexes by scene.
    #      Room_shop needs no new SPU audio: one SceneManager, atmos cue
    #      SurfaceInterior, channels [1,3,5,6] all resident, environmentType 0.
    #   3. host/regions.py, taking 693 regions to 699 as chunks 694 to 699. No
    #      existing chunk moves; tests/test_regions.py guards that, because a row
    #      inserted anywhere but the end renumbers ids the packs, the guest
    #      tables and the save records all address by.
    # Expect every chunk sha to change even so: postpack_similarity picks
    # representatives world-wide, so 90 new textures can move what any other
    # view's textures collapse to. The per-region cook cache survives it.
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
