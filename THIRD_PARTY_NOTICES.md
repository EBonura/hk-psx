# Code and source inputs

New project code is licensed GPL-2.0-or-later, compatible with the PSoXide
SDK it links. The GNU GPL text is in LICENSE.

- PSoXide SDK, runtime, GPU/pad/VRAM subsystems, telemetry, linker, disc writer
  and hazard analyzer: GPL-2.0-or-later. Revision and source owner are recorded
  in sdk.lock.json. Hydrated sources and their original notices stay in the
  ignored .psoxide directory. No sibling files were changed.
- The host-only opaque tile proof includes PSoXide emulator triangle raster
  setup/interpolation equations, GPL-2.0-or-later, in host/coverage_raster.rs.
  They are copied from EBonura/PSoXide-emulator revision
  38af605ac5a6961f3798d432bcfb7cceacece239,
  emu/crates/emulator-core/src/gpu/raster.rs (upstream file SHA256
  eb24fcaa17fb0a6f7f876b9c20466c9268d2c5c8bf35950fc39d5b91319ef6b6).
  The source URL and license notice are retained in that file. hk-cook links
  this vendored host file into its coverage proofs and the opaque reports
  record its hash; the build does not require an emulator checkout. Generated
  certificates contain only local cooked-asset coverage data and remain
  ignored alongside those assets.
- UnityPy 1.25.3: MIT, https://github.com/K0lb3/UnityPy.
- TypeTreeGeneratorAPI 0.0.10: MIT,
  https://github.com/UnityPy-Org/TypeTreeGeneratorAPI.
- dnfile 0.18.0: MIT, https://github.com/malwarefrank/dnfile.
- dncil 1.0.2: Apache-2.0, https://github.com/mandiant/dncil.
- host/hk-unity (the Rust Unity reader replacing UnityPy) follows UnityPy
  1.25.3's type tree reading rules, MIT, Copyright (c) 2019-2026 K0lb3. Its
  vendored built-in class trees (host/hk-unity/data/typetrees.tsv) were
  extracted from UnityPy's bundled AssetRipper TPK package through tpk_ar
  0.2.4 (MIT); provenance in host/hk-unity/data/PROVENANCE.md.
- host/hk-unity/src/generator.rs ports the MonoBehaviour template rules of
  AssetsTools.NET's MonoCecilTempGenerator and CommonMonoTemplateHelper
  (https://github.com/nesrak1/AssetsTools.NET, MIT, Copyright (c) 2020 nesrak1),
  the backend TypeTreeGeneratorAPI 0.0.10 uses.
- host/hk-pil ports image operations from Pillow 12.3.0's libImaging
  (https://github.com/python-pillow/Pillow), MIT-CMU (HPND) licence,
  Copyright (c) 1997-2011 Secret Labs AB, (c) 1995-2011 Fredrik Lundh and
  contributors, (c) 2010 Jeffrey A. Clark and contributors. Its BCn decoder
  follows BcnDecode.c, which is CC0. host/hk-unity/src/texture.rs follows
  UnityPy's Texture2DConverter, SpriteHelper and MeshHelper (MIT).
- host/hk-lz4 ports liblz4 1.9.4's HC block compressor (lz4hc.c,
  https://github.com/lz4/lz4, BSD 2-Clause, Copyright (c) Yann Collet).
- Pillow and transitive host dependencies retain their own licenses and
  installed notices. Exact tested package versions are in host/requirements.lock.
  These extraction and assembly-inspection dependencies are host-only.

Hollow Knight files, Unity data, game code, sprites and audio remain the property
of their respective owners. The project license grants no rights to redistribute
those inputs or their converted forms. The Windows installation and saves are
read-only inputs. Generated content, assembly inspection, captures and discs stay
in ignored local directories. Nothing was uploaded, published or burned.
