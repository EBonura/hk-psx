# Code and source inputs

New project code is licensed GPL-2.0-or-later, compatible with the PSoXide
SDK it links. The GNU GPL text is in LICENSE.

- PSoXide SDK, runtime, GPU/pad/VRAM subsystems, telemetry, linker, disc writer
  and hazard analyzer: GPL-2.0-or-later. Revision and source owner are recorded
  in sdk.lock.json. Hydrated sources and their original notices stay in the
  ignored .psoxide directory. No sibling files were changed.
- UnityPy 1.25.3: MIT, https://github.com/K0lb3/UnityPy.
- TypeTreeGeneratorAPI 0.0.10: MIT,
  https://github.com/UnityPy-Org/TypeTreeGeneratorAPI.
- dnfile 0.18.0: MIT, https://github.com/malwarefrank/dnfile.
- dncil 1.0.2: Apache-2.0, https://github.com/mandiant/dncil.
- Pillow and transitive host dependencies retain their own licenses and
  installed notices. Exact tested package versions are in host/requirements.lock.
  These extraction and assembly-inspection dependencies are host-only.

Hollow Knight files, Unity data, game code, sprites and audio remain the property
of their respective owners. The project license grants no rights to redistribute
those inputs or their converted forms. The Windows installation and saves are
read-only inputs. Generated content, assembly inspection, captures and discs stay
in ignored local directories. Nothing was uploaded, published or burned.
