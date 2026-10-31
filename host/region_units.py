"""Cached cook units for one region: its scenery and its annotations.

A region's cook (host/cook.py `cook`) used to be one unit. Most of its cost, the texture
quantiser, belongs to the scenery; the lists of actors, hazards and shrooms on top of it cost
almost nothing but every enemy-kind edit changes the code that makes them. So the region is two
units with their own receipts, each keyed by what it really ran (host/depcache.py):

  scenery      room.hk, tilemap-fill.json, unsupported.json, scene-base.json and the base report.
  annotations  the actor, hazard and shroom records (cook.cook_annotations).

`assemble` merges them into scene.json and the cooked report the rest of the pipeline reads, the
same bytes the single unit wrote. A receipt is fresh when its inputs (region spec, retail source
manifest, quality constants), the code and files it recorded, and its outputs are all unchanged.
"""
from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path

import depcache
from source import ROOT, Source, dump

SCENERY_RECEIPT = 'scenery-cache.json'
ANNOTATION_RECEIPT = 'annotations-cache.json'
SCENE_BASE = 'scene-base.json'
ANNOTATION_KEYS = ('actors', 'hazards', 'shrooms')
# The receipts and the merged scene.json are not inputs of any unit.
NOT_DEPENDENCIES = ('cook-cache.json', SCENERY_RECEIPT, ANNOTATION_RECEIPT)
# The scene catalog (quality.py module level) is left out of the dependency set on purpose: growing it
# keeps every earlier region. The constants a cook reads from it are keyed by value in `params`.
SKIP_SKELETON = ('host/quality.py',)
# Written by region 1 only (cook.py write_shared).
SHARED_OUTPUTS = ('data/params.rs', '.hkpsx/gameplay-source.json', '.hkpsx/focus-source.json',
                  '.hkpsx/vitals-source.json', '.hkpsx/combat-source.json', '.hkpsx/provenance.json')


def quality_values() -> list:
    from quality import (SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, TEXTURE_BUDGET, ROOM_BYTE_BUDGET,
                         SCENERY_TEXEL_CAP, SCENERY_SCENE_CAPS)
    return [SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, TEXTURE_BUDGET, ROOM_BYTE_BUDGET, SCENERY_TEXEL_CAP,
            sorted(SCENERY_SCENE_CAPS.items())]


def source_base() -> str | None:
    """The build driver's checked hash of the whole retail source manifest, or None (no reuse)."""
    manifest = ROOT / '.hkpsx/regions-source-inputs.json'
    checked = os.environ.get('HK_SOURCE_FINGERPRINT_CHECKED')
    if not checked or not manifest.is_file() or depcache.sha256_file(manifest) != checked:
        return None
    return checked


def normalise(value):
    """The JSON form of `value`, as a cache hit would hand it back."""
    return json.loads(json.dumps(value, default=lambda v: list(v)))


def is_not_dependency(path: str) -> bool:
    return os.path.basename(path) in NOT_DEPENDENCIES


def new_tracker() -> depcache.Tracker:
    return depcache.Tracker(skip_skeleton=SKIP_SKELETON, skip_file=is_not_dependency)


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


def receipt_status(path: Path, params: dict, base: str | None, destination: Path) -> tuple[dict | None, str]:
    """(receipt, '') when `path` holds a fresh receipt for `params`, else (None, why not)."""
    if base is None:
        return None, 'no checked retail source manifest'
    old = _read(path)
    if not isinstance(old, dict) or old.get('schema') != 3:
        return None, 'no receipt'
    if old.get('base') != base:
        return None, 'retail source changed'
    if old.get('params') != params:
        return None, 'region spec or quality constants changed'
    reasons = depcache.stale(old['deps'])
    if reasons:
        return None, reasons[0]
    if not _outputs_intact(destination, old['outputs']):
        return None, 'an output differs from the receipt'
    return old, ''


def _output_digests(destination: Path, names, shared=()) -> dict:
    outputs = {name: depcache.sha256_file(destination / name) for name in names}
    for name in shared:
        outputs[f'shared:{name}'] = depcache.sha256_file(ROOT / name)
    return outputs


def write_receipt(path: Path, **fields):
    temp = path.with_name(path.name + f'.{os.getpid()}.tmp')
    temp.write_text(json.dumps({'schema': 3, **fields}))
    temp.replace(path)


class RegionCook:
    """Cooks, or finds fresh, the two units of one region; `tracker` spans the whole process."""

    def __init__(self, source, tracker, base):
        self.source = source
        self.tracker = tracker
        self.base = base
        self.pixels = {}
        self.geometry = {}
        self.scenes = {}

    def _scene(self, region):
        from scene import Scene
        file = region['scene_file']
        if file not in self.scenes:
            self.scenes[file] = Scene(self.source, file)
        return self.scenes[file]

    def scenery(self, region, chunk_id, destination: Path):
        """The scenery unit's base report: fresh from its receipt or cooked now."""
        from cook import cook
        params = {'region': normalise(region), 'quality': quality_values(), 'part': 'scenery'}
        receipt, _ = receipt_status(destination / SCENERY_RECEIPT, params, self.base, destination)
        if receipt is not None:
            return receipt['cooked'], True
        try:
            cooked = cook(self.source, self._scene(region), region, destination, self.pixels, self.geometry,
                          write_shared=chunk_id == 1, stage_for_similarity=True, annotations=False)
        except ValueError as error:
            if chunk_id == 1 or not any(text in str(error) for text in
                                        ('budget exceeded', 'pool exceeds', 'cache dimensions', 'bank exceeds')):
                raise
            raise ValueError(f'Fixed layout region {chunk_id} exceeds measured residency budget: {error}') from error
        names = ['room.hk', 'unsupported.json', 'tilemap-fill.json', SCENE_BASE]
        if chunk_id == 1:
            names += ['palettes.bin'] + sorted(p.name for p in destination.glob('page-*.bin'))
        cooked = normalise(cooked)
        if self.base is not None:
            write_receipt(destination / SCENERY_RECEIPT, params=params, base=self.base, deps=self.tracker.record(),
                          outputs=_output_digests(destination, names, SHARED_OUTPUTS if chunk_id == 1 else ()),
                          cooked=cooked)
        return cooked, False

    def annotations(self, region, destination: Path):
        from cook import cook_annotations
        params = {'region': normalise(region), 'part': 'annotations'}
        receipt, _ = receipt_status(destination / ANNOTATION_RECEIPT, params, self.base, destination)
        if receipt is not None:
            return receipt['value'], True
        value = normalise(cook_annotations(self.source, self._scene(region), region))
        if self.base is not None:
            write_receipt(destination / ANNOTATION_RECEIPT, params=params, base=self.base,
                          deps=self.tracker.record(), outputs={}, value=value)
        return value, False

    def region(self, region, chunk_id, destination: Path, cooked_hook=None):
        """The merged cooked report of one region, with scene.json written to match."""
        scenery, scenery_hit = self.scenery(region, chunk_id, destination)
        if cooked_hook is not None:
            cooked_hook(chunk_id, region, scenery, scenery_hit)
        annotations, annotations_hit = self.annotations(region, destination)
        cooked = assemble(destination, scenery, annotations)
        return cooked, scenery_hit and annotations_hit


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


def precook_scene(scene_id: int, out: Path):
    """Worker process: bring every region of one scene up to date."""
    from region_plan import fixed_regions
    base = source_base()
    regions = [(index + 1, r) for index, r in enumerate(fixed_regions()) if r['scene_id'] == scene_id]
    with new_tracker() as tracker:
        cook = RegionCook(Source(), tracker, base)
        for chunk_id, region in regions:
            destination = out / f'region-{chunk_id:03}'
            ordinal = len(cook.scenes)
            _, hit = cook.region(region, chunk_id, destination)
            if not hit:
                print(f'REGION {chunk_id}: {region["scene_name"]} {region["activation_bounds"]} (worker)', flush=True)
