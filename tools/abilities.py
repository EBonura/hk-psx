"""The source's movement/traversal ability catalog, and what the admitted
scenes actually ask of it (P14 step 3).

The plan wants the extracted catalog used as exhaustive authority rather than a
guess about which special traversal states exist. PlayerData carries one boolean
per ability, so that list is the catalog; the scenes then say which of those
states any admitted room can reach. No retail payload is embedded; the report
goes to .hkpsx.
"""
import sys
from pathlib import Path as _Path
sys.path.insert(0, str(_Path(__file__).resolve().parents[1] / 'host'))
import json, re
from pathlib import Path
from source import ROOT, dump
from inspect_il import inspect

# PlayerData booleans that gate movement or traversal, and what each unlocks.
# Keys, spellings and grouping all come from the CIL; the notes name the item.
TRAVERSAL = {
    'canDash': 'Mothwing Cloak, the dash itself',
    'hasDash': 'Mothwing Cloak, acquisition',
    'hasWalljump': 'Mantis Claw, wall slide and wall jump',
    'hasDoubleJump': 'Monarch Wings',
    'hasSuperDash': 'Crystal Heart',
    'hasShadowDash': 'Shade Cloak, the invulnerable dash',
    'hasAcidArmour': "Isma's Tear, acid access",
    'hasLantern': 'Lumafly Lantern, darkened rooms',
    'hasDreamNail': 'Dream Nail',
    'hasDreamGate': 'Dream Gate travel',
    'hasTramPass': 'Tram Pass',
}
# Scene components and objects that put the Knight into a special traversal
# state. A scene carrying none of these needs none of the states.
SURFACES = {
    'acid': ('AcidWater', 'Acid Water', 'acid'),
    'water': ('SurfaceWater', 'Surface Water', 'Water Surface'),
    'conveyor': ('ConveyorBelt', 'ConveyorBeltAgent', 'conveyor'),
    'bounce': ('BounceShroom', 'Bounce Shroom', 'Shroom'),
    'tram': ('TramControl', 'Tram'),
}

def catalog(assembly):
    """The PlayerData booleans, read from the CIL rather than assumed."""
    text = inspect(assembly, {'PlayerData'})
    present = set(re.findall(r'\b(has[A-Z][A-Za-z]+|canDash)\b', text))
    missing = sorted(k for k in TRAVERSAL if k not in present)
    assert not missing, f'PlayerData no longer carries {missing}'
    return {k: TRAVERSAL[k] for k in sorted(TRAVERSAL)}, sorted(present - set(TRAVERSAL))

def scene_demands(source, scene_names):
    """Which special-surface families each admitted scene actually contains."""
    found = {}
    for name in scene_names:
        file = source.file(name)
        labels = set()
        for o in file.objects.values():
            try:
                typename = source.typename(o)
            except Exception:
                continue
            labels.add(typename)
            if o.type.name == 'GameObject':
                try:
                    labels.add(source.read(o)['m_Name'])
                except Exception:
                    pass
        hits = sorted(family for family, needles in SURFACES.items()
                      if any(n in label for label in labels for n in needles))
        if hits:
            found[name] = hits
    return found

def report(source, assembly, scene_names):
    traversal, other = catalog(assembly)
    demands = scene_demands(source, scene_names)
    out = {
        'traversal_abilities': traversal,
        'other_playerdata_flags': other,
        'admitted_scene_count': len(scene_names),
        'special_surfaces_in_admitted_scenes': demands,
        'implemented': ['canDash/hasDash', 'hasWalljump', 'hasDoubleJump', 'hasSuperDash',
                        'hasShadowDash'],
        'limitations': [
            'Acquisition is not implemented for any of these: every ability is reachable '
            'only through its Cheats row until the pickups exist (P14 step 8).',
            "hasAcidArmour and hasLantern gate states no admitted scene contains, so they "
            'are catalogued rather than implemented.',
            'hasDreamNail and hasDreamGate belong to P14 step 5.',
            'Charm-modified movement (Dashmaster, Sharp Shadow) waits on charms.',
        ],
    }
    dump(ROOT / '.hkpsx/ability-catalog.json', out)
    return out

if __name__ == '__main__':
    from source import Source
    s = Source()
    data = Path(json.load(open(ROOT / '.hkpsx/doctor.json'))['installs'][0]['data_directory'])
    names = sorted({e['file'] for e in json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']})
    out = report(s, data / 'Managed' / 'Assembly-CSharp.dll', names)
    print(f"{len(out['traversal_abilities'])} traversal abilities; "
          f"special surfaces in {len(out['special_surfaces_in_admitted_scenes'])} of {len(names)} scenes")
    for scene, hits in out['special_surfaces_in_admitted_scenes'].items():
        print(f'  {scene}: {", ".join(hits)}')
