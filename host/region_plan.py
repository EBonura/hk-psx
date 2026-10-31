"""The fixed region plan: which views the world is cooked in.

Split out of regions.py so a cook worker needs only this and the cooker, not the postpasses.
"""
from cook import CAM_X, CAM_Y
from quality import SCENE_TABLE, GRID_SCENE_LAYOUTS, REGION_LAYOUT, TOWN_EXTENSION_LAYOUT

SCENES = [dict(scene) for scene in SCENE_TABLE]


def intersects(a, b):
    return a[0] <= b[2] and b[0] <= a[2] and a[1] <= b[3] and b[1] <= a[3]


def region_spec(scene, bounds):
    x0,y0,x1,y1 = bounds
    global_camera = scene['camera_global_bounds']
    cx = [max(global_camera[0],min(global_camera[2],x)) for x in (x0,x1)]
    cy = [max(global_camera[1],min(global_camera[3],y+2)) for y in (y0,y1)]
    return {'scene_id': scene['scene_id'], 'scene_name': scene['scene_name'],
            'scene_file': scene['file'], 'activation_bounds': list(bounds),
            'camera_bounds': [cx[0],cy[0],cx[1],cy[1]], 'camera_x': cx, 'camera_y': cy,
            'collision_bounds': [x0-3,y0-5,x1+3,y1+5],
            'interaction_bounds': [x0-2,y0-2,x1+2,y1+2],
            'grass_bounds': [x0-2,y0-2,x1+2,y1+2], 'allow_slopes': True}


def initial_regions():
    initial = region_spec(SCENES[0], [15,-5,62,25])
    # Preserve the existing opening view and initial interaction test route.
    initial.update(camera_x=list(CAM_X), camera_y=list(CAM_Y),
                   camera_bounds=[CAM_X[0],CAM_Y[0],CAM_X[1],CAM_Y[1]],
                   collision_bounds=[15,-5,78,35], grass_bounds=[20,0,70,25])
    pending=[]
    for scene in SCENES:
        left,bottom,right,top=scene['runtime_bounds']
        for y in range(bottom,top,16):
            for x in range(left,right,24):
                bounds=[x,y,min(x+24,right),min(y+16,top)]
                base=initial['activation_bounds']
                if scene['scene_id']==0 and base[0]<=bounds[0] and base[1]<=bounds[1] and base[2]>=bounds[2] and base[3]>=bounds[3]:
                    continue
                pending.append(region_spec(scene,bounds))
    # Expose the immediate first-door continuation first for incremental smoke
    # testing. Remaining IDs stay deterministic for this region plan revision.
    pending.sort(key=lambda r:(r['scene_id'],
                 0 if r['activation_bounds'][:2]==[48,11] else 1,
                 r['activation_bounds'][1],r['activation_bounds'][0]))
    return [initial]+pending


def fixed_regions():
    """Keep the measured layout stable across texture quality changes."""
    # Keep every existing chunk ID and Tutorial view. Town's old x40 clamp was
    # the development coverage edge; extending it allows continuous traversal
    # into the new views without changing screen scale or the gate's x12 clamp.
    result=[region_spec(SCENES[scene],bounds) for scene,bounds in REGION_LAYOUT]
    for bounds,camera in TOWN_EXTENSION_LAYOUT:
        region=region_spec(SCENES[1],bounds)
        region.update(camera_bounds=list(camera),camera_x=[camera[0],camera[2]],
                      camera_y=[camera[1],camera[3]])
        result.append(region)
    for scene_id,boxes in sorted(GRID_SCENE_LAYOUTS.items()):
        for bounds in boxes:
            result.append(region_spec(SCENES[scene_id],bounds))
    initial=initial_regions()[0]
    if result[0]['activation_bounds'] != initial['activation_bounds']:
        raise ValueError('fixed layout opening differs from initial room')
    result[0]=initial
    return result


def split_region(region):
    x0,y0,x1,y1=region['activation_bounds']
    if max(x1-x0,y1-y0)<=3:
        raise ValueError(f'One camera view exceeds residency budgets at {region["activation_bounds"]}')
    if x1-x0 >= y1-y0:
        mid=(x0+x1)/2;pieces=([x0,y0,mid,y1],[mid,y0,x1,y1])
    else:
        mid=(y0+y1)/2;pieces=([x0,y0,x1,mid],[x0,mid,x1,y1])
    scene=SCENES[region['scene_id']]
    return [region_spec(scene,piece) for piece in pieces]
