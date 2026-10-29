# The shared cook store

A build that has nothing to cook should not cook. `cargo hk-build build` already skips the region,
menu and asset cooks when their caches match, but a fresh lane has no caches, and a lane whose
caches were written by another checkout misses on whatever differs. The cook store holds finished
cook states, keyed by the cooker inputs that produced them, so any lane on the same inputs starts
from a hit.

It is off unless `HK_COOK_STORE` names a directory. With it set, `build` restores before the cook
and publishes after the asset stage; nothing else about the build changes.

## What is stored

A snapshot is every file under `data/` and `.hkpsx/` except per-lane state: `.hkpsx/validate`,
`pgo`, `emulator`, the `*-tests` and `boss-sim` scratch directories, `doctor.json`, `sdk.json`,
`python-lock.sha256` (it stamps the lane's own `.venv`), `build*.json`, the cook-store marker, and
the files the guest build writes after the caches are stamped (`boot-art.hk`, `boot_art.rs`,
`intro.exe`). The code is `host/hk-build/cook_store.rs`.

```text
$HK_COOK_STORE/objects/<aa>/<sha256>   one read-only file per distinct content
$HK_COOK_STORE/keys/<key>.json         path -> sha256, size, exec bit; label; creation time
```

The key is a digest of the region cooker's code (`host/cook_inputs.txt`), the retail source
manifest, and every asset cooker's code: everything `assets_key` hashes except the cook's own
outputs. A lane can therefore compute it before it has cooked anything.

The key picks a snapshot to try. It is never trusted for correctness: the cook, menu and asset
caches re-hash every output they vouch for, so a stale or partial snapshot is a miss and a normal
cook.

## Why it costs no disk

Files move in and out of the store by APFS clone (`clonefile`). A restore of 33,000 files and
about 5 GB writes no data blocks; a lane pays for a file only when it changes it. The cookers
rewrite files with identical bytes, which severs a clone; `dedupe` (run automatically after the
asset stage) gives those files back to the store's blocks, so a recook costs its peak but not its
steady state. A store on another volume falls back to copies (correct, not free).

## Commands

```text
export HK_COOK_STORE=/Users/ebonura/Desktop/repos/campaign-2026-10-15/_shared/hk/cook-store
cargo hk-build store status            snapshots, and whether this lane is at one
cargo hk-build store key               the key of this checkout's cooker inputs
cargo hk-build store restore [--force] make this lane's cook state the snapshot for its key;
                                       exit 3 if the store has none
cargo hk-build store publish [--label] add this lane's state (only right after a build that printed
                                       all three cache hits)
cargo hk-build store dedupe            give identical rewritten files back to the store's blocks
cargo hk-build store gc [--keep N] [--apply]   keep the newest N snapshots (default 3); dry run unless --apply
cargo hk-build store verify            re-hash every object
cargo hk-build store phys DIR...       logical, physical and unshared bytes per directory
```

`store restore` makes the lane's `data/` and `.hkpsx/` exactly the snapshot: files the snapshot
does not hold are removed, files in the skipped list are left alone. It checks that every object
exists with the right size before it changes anything.

## Failure behaviour

A store problem is a printed warning and a normal cook, never a failed build. Two lanes publishing
the same key at once is safe: objects are content-named, manifests are written by rename, and the
first publisher of a key wins.
