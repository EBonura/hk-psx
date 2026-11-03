"""Cached cook units for the regions: their scenery and their annotations.

A region's cook (host/cook.py `cook`) used to be one unit keyed by the hash of about sixty files.
Most of its cost, the texture quantiser, belongs to the scenery; the lists of actors, hazards and
shrooms laid over it cost almost nothing but every enemy-kind edit changes the code that makes
them. So the region is two units with their own receipts, each keyed by what it really ran
(host/depcache.py):

  scenery      room.hk, tilemap-fill.json, unsupported.json, scene-base.json and the base report.
  annotations  the actor, hazard and shroom records (cook.cook_annotations).

`assemble` merges them into scene.json and the cooked report the rest of the pipeline reads, the
same bytes the single unit wrote.

A receipt names the dependency record of the worker process that made it (every region of a scene
for one part is cooked by one process, so they share one record) and holds the unit's identity
(its region spec, the budget constants, the retail root), the digest of every file it wrote and
the report it returned. It is fresh when its identity matches, the record still describes the code
and files on disk, and every output still has the recorded bytes. A receipt is written only after
the outputs are complete, and removed before a unit starts to rewrite them, so a cook that dies
leaves no receipt that vouches for a half-written file.

Run as a script this module is the worker: `region_units.py --precook SCENE_ID --part scenery`.
Only the standard library and host/depcache.py are imported at module level, because the dependency
tracker has to be armed before the cooker's own modules are imported.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import depcache

ROOT = depcache.ROOT
SCHEMA = 4
SCENERY_RECEIPT = 'scenery-cache.json'
ANNOTATION_RECEIPT = 'annotations-cache.json'
SCENE_BASE = 'scene-base.json'
ANNOTATION_KEYS = ('actors', 'hazards', 'shrooms')
PARTS = ('scenery', 'annotations')
RECEIPT_NAMES = (SCENERY_RECEIPT, ANNOTATION_RECEIPT)
# What a unit's receipts and merged result are not: inputs of any unit, or outputs of its own.
NOT_DEPENDENCIES = ('cook-cache.json',) + RECEIPT_NAMES
# The scene catalog (quality.py module level) is left out of the dependency set on purpose: growing it
# keeps every earlier region. What a cook reads from it is keyed by value in the receipt's identity: the
# budget constants and the region's own scene row.
SKIP_SKELETON = ('host/quality.py',)
# Written by region 1 only (cook.py write_shared), and read by the passes after it.
SHARED_OUTPUTS = ('data/params.rs', '.hkpsx/gameplay-source.json', '.hkpsx/focus-source.json',
                  '.hkpsx/vitals-source.json', '.hkpsx/combat-source.json')
# Unit outputs are flushed to receipts at least this often, so an interrupted cook keeps its progress.
FLUSH_EVERY = 6


def normalise(value):
    """The JSON form of `value`, as a receipt hands it back."""
    return json.loads(json.dumps(value, default=lambda v: list(v)))


def is_not_dependency(path: str) -> bool:
    return os.path.basename(path) in NOT_DEPENDENCIES


def new_tracker(retail) -> depcache.Tracker:
    return depcache.Tracker(retail=retail, skip_skeleton=SKIP_SKELETON, skip_file=is_not_dependency,
                            extra_skeletons=['host/region_units.py'])


def quality_values() -> list:
    from quality import (SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, TEXTURE_BUDGET, ROOM_BYTE_BUDGET,
                         SCENERY_TEXEL_CAP, SCENERY_SCENE_CAPS)
    return [SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, TEXTURE_BUDGET, ROOM_BYTE_BUDGET, SCENERY_TEXEL_CAP,
            sorted(SCENERY_SCENE_CAPS.items())]


def unit_params(part: str, region: dict, chunk_id: int, retail) -> dict:
    """What identifies a unit besides the code it ran: the region, the budgets, the retail root."""
    from region_plan import SCENES
    params = {'part': part, 'region': region, 'source': retail.directory,
              'scene_row': SCENES[region['scene_id']]}
    if part == 'scenery':
        params.update(quality=quality_values(), write_shared=chunk_id == 1)
    return normalise(params)


def _read(path: Path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def _outputs_intact(destination: Path, outputs: dict) -> bool:
    for name, digest in outputs.items():
        path = ROOT / name[7:] if name.startswith('shared:') else destination / name
        if not path.is_file() or depcache.sha256_file(path) != digest:
            return False
    return True


def receipt_status(path: Path, params: dict, retail, destination: Path):
    """(receipt, '') when `path` holds a fresh receipt for `params`, else (None, why not)."""
    if retail is None:
        return None, 'retail source not checked by the build driver'
    if depcache.force_cook():
        return None, 'full recook requested'
    old = _read(path)
    if not isinstance(old, dict) or old.get('schema') != SCHEMA:
        return None, 'no receipt'
    if old.get('params') != params:
        return None, 'region spec or quality constants changed'
    reasons = depcache.stale_by_name(old['deps'], retail)
    if reasons:
        return None, reasons[0]
    if not _outputs_intact(destination, old['outputs']):
        return None, 'an output differs from the receipt'
    return old, ''


def write_receipt(path: Path, **fields):
    temp = path.with_name(path.name + f'.{os.getpid()}.tmp')
    temp.write_text(json.dumps({'schema': SCHEMA, **fields}))
    temp.replace(path)


def receipt_names(part: str) -> str:
    return SCENERY_RECEIPT if part == 'scenery' else ANNOTATION_RECEIPT


def part_status(part: str, region: dict, chunk_id: int, destination: Path, retail):
    receipt, why = receipt_status(destination / receipt_names(part), unit_params(part, region, chunk_id, retail),
                                  retail, destination)
    return receipt, why


def region_is_fresh(region: dict, chunk_id: int, destination: Path, retail) -> bool:
    return all(part_status(part, region, chunk_id, destination, retail)[0] is not None for part in PARTS)


def missing_parts(regions, out: Path, retail) -> dict:
    """{(scene id, part): regions to cook} over `regions` (in chunk order)."""
    misses: dict = {}
    for index, region in enumerate(regions):
        destination = out / f'region-{index + 1:03}'
        for part in PARTS:
            if part_status(part, region, index + 1, destination, retail)[0] is None:
                key = (region['scene_id'], part)
                misses[key] = misses.get(key, 0) + 1
    return misses


def check_packet_budget(chunk_id,region,destination,cooked):
    """Total the view's scenery packets now, while the cook can still stop: a view
    over CAP-ACTOR_RESERVE fails here, minutes into the cook, instead of in the
    similarity pass after every other region has cooked. Views near the limit
    are printed (host/scenery_geometry.py has the bound)."""
    from scenery_geometry import check_view_budget
    check_view_budget(chunk_id,region['scene_name'],(destination/'room.hk').read_bytes(),
                      [g['draws']for g in cooked.get('decor',[])])


def assemble(destination: Path, scenery: dict, annotations: dict) -> dict:
    """Write scene.json from scene-base.json and the annotations; return the merged cooked report."""
    cooked = dict(scenery)
    cooked.update({key: annotations[key] for key in ANNOTATION_KEYS})
    scene = json.loads((destination / SCENE_BASE).read_text())
    scene.update({key: annotations[key] for key in ANNOTATION_KEYS})
    text = json.dumps(scene, indent=2, default=lambda v: list(v))
    target = destination / 'scene.json'
    if not target.is_file() or target.read_text() != text:
        target.write_text(text)
    return cooked


def read_region(region: dict, chunk_id: int, destination: Path, retail) -> dict:
    """The merged cooked report of a region whose two receipts are fresh; scene.json is written to match.

    Raises when a unit is not fresh: the workers ran first, so a stale one is a bug or a changed input.
    """
    values = {}
    for part in PARTS:
        receipt, why = part_status(part, region, chunk_id, destination, retail)
        if receipt is None:
            raise ValueError(f'Region {chunk_id} {part} is not fresh after its worker ran: {why}')
        values[part] = receipt['cooked'] if part == 'scenery' else receipt['value']
        depcache.STAGE_UNITS.add(receipt['deps'])
    return assemble(destination, values['scenery'], values['annotations'])


class Worker:
    """Brings the units of one part of one scene up to date, in this process, under one tracker."""

    def __init__(self, scene_id: int, part: str, out: Path, retail, tracker, only=None):
        from source import Source
        from region_plan import fixed_regions
        self.part = part
        self.out = out
        self.retail = retail
        self.tracker = tracker
        self.source = Source()
        self.scene = None
        self.pixels = {}
        self.geometry = {}
        self.regions = [(i + 1, r) for i, r in enumerate(fixed_regions())
                        if r['scene_id'] == scene_id and (only is None or i + 1 in only)]
        self.pending: list = []

    def _scene(self, region):
        from scene import Scene
        if self.scene is None:
            self.scene = Scene(self.source, region['scene_file'])
        return self.scene

    def flush(self):
        """Write the receipts of the units finished so far, naming this process's record as they stand."""
        if not self.pending:
            return
        name = depcache.save(self.tracker.record())
        for path, fields in self.pending:
            write_receipt(path, deps=name, **fields)
        self.pending = []

    def run(self):
        done = 0
        for chunk_id, region in self.regions:
            destination = self.out / f'region-{chunk_id:03}'
            receipt, _ = part_status(self.part, region, chunk_id, destination, self.retail)
            if receipt is not None:
                continue
            print(f'REGION {chunk_id}: {region["scene_name"]} {region["activation_bounds"]} {self.part} (worker)', flush=True)
            path = destination / receipt_names(self.part)
            params = unit_params(self.part, region, chunk_id, self.retail)
            # No receipt may outlive the files its unit is about to rewrite (a kill leaves nothing fresh).
            path.unlink(missing_ok=True)
            if self.part == 'scenery':
                fields = self.scenery(region, chunk_id, destination)
            else:
                fields = self.annotations(region, destination)
            self.pending.append((path, dict(params=params, **fields)))
            done += 1
            if done % FLUSH_EVERY == 0:
                self.flush()
        self.flush()

    def scenery(self, region, chunk_id, destination: Path) -> dict:
        from cook import cook
        destination.mkdir(parents=True, exist_ok=True)
        # The unit owns its directory: nothing a rerun does not write may be left behind from an older cook.
        for entry in destination.iterdir():
            if entry.is_file() and entry.name != ANNOTATION_RECEIPT:
                entry.unlink()
        try:
            cooked = cook(self.source, self._scene(region), region, destination, self.pixels, self.geometry,
                          write_shared=chunk_id == 1, stage_for_similarity=True, annotations=False)
        except ValueError as error:
            if chunk_id == 1 or not any(text in str(error) for text in
                                        ('budget exceeded', 'pool exceeds', 'cache dimensions', 'bank exceeds')):
                raise
            raise ValueError(f'Fixed layout region {chunk_id} exceeds measured residency budget: {error}') from error
        outputs = {entry.name: depcache.sha256_file(entry) for entry in sorted(destination.iterdir())
                   if entry.is_file() and entry.name not in NOT_DEPENDENCIES and entry.name != 'scene.json'}
        if chunk_id == 1:
            for name in SHARED_OUTPUTS:
                outputs[f'shared:{name}'] = depcache.sha256_file(ROOT / name)
        check_packet_budget(chunk_id, region, destination, cooked)
        return {'outputs': outputs, 'cooked': normalise(cooked)}

    def annotations(self, region, destination: Path) -> dict:
        from cook import cook_annotations
        destination.mkdir(parents=True, exist_ok=True)
        value = normalise(cook_annotations(self.source, self._scene(region), region))
        return {'outputs': {}, 'value': value}


def run_worker(scene_id: int, part: str, out: Path, retail, tracker, only=None):
    Worker(scene_id, part, out, retail, tracker, only).run()


def main(argv=None):
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--precook', type=int, required=True, metavar='SCENE_ID')
    parser.add_argument('--part', choices=PARTS, required=True)
    parser.add_argument('--out', type=Path, help='Diagnostic: write the region directories here instead of data/regions')
    parser.add_argument('--only', help='Diagnostic: comma separated chunk ids; the rest of the scene is left alone')
    args = parser.parse_args(argv)
    retail = depcache.Retail.checked()
    if retail is None:
        raise SystemExit('region_units: no checked retail source manifest; run through cargo hk-build')
    with new_tracker(retail) as tracker:
        only = {int(x) for x in args.only.split(',')} if args.only else None
        run_worker(args.precook, args.part, args.out or ROOT / 'data/regions', retail, tracker, only)


if __name__ == '__main__':
    main()
