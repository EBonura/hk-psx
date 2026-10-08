#!/usr/bin/env python3
"""Discover Windows Hollow Knight in CrossOver; no third-party packages required."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import sys

ROOT = Path(__file__).resolve().parents[1]
APP_ID = "367520"


def vdf_value(text, key):
    match = re.search(r'"' + re.escape(key) + r'"\s+"((?:\\.|[^"\\])*)"', text)
    return match.group(1).replace('\\\\', '\\') if match else None


def steam_libraries():
    bottles = Path.home() / "Library/Application Support/CrossOver/Bottles"
    roots = []
    for bottle in sorted(bottles.glob("*")):
        for prefix in ("Program Files (x86)", "Program Files"):
            root = bottle / "drive_c" / prefix / "Steam"
            if not root.is_dir():
                continue
            roots.append(root)
            metadata = root / "steamapps/libraryfolders.vdf"
            if not metadata.is_file():
                continue
            text = metadata.read_text(errors="replace")
            for value in re.findall(r'"path"\s+"((?:\\.|[^"\\])*)"', text):
                value = value.replace('\\\\', '\\')
                windows_path = re.fullmatch(r"([A-Za-z]):[\\/](.*)", value)
                if windows_path:
                    drive, relative = windows_path.groups()
                    base = (bottle / "drive_c" if drive.lower() == "c" else
                            bottle / "dosdevices" / (drive.lower() + ":"))
                    if base.is_dir():
                        roots.append(base.joinpath(*re.split(r"[\\/]", relative)))
    return list(dict.fromkeys(p.expanduser().resolve() for p in roots))


def data_directory(install):
    candidates = [install, install / "hollow_knight_Data", install / "Hollow Knight_Data"]
    return next((p for p in candidates if p.name.lower().endswith("_data")
                 and (p / "globalgamemanagers").is_file()), None)


def inspect(install, manifest=None):
    data = data_directory(install)
    result = {"install": str(install), "data_directory": str(data) if data else None,
              "layout_detected": False, "installation_complete": "not verified"}
    if manifest and manifest.is_file():
        source = manifest.read_text(errors="replace")
        keys = ("appid", "name", "StateFlags", "installdir", "buildid",
                "BytesToDownload", "BytesDownloaded", "BytesToStage", "BytesStaged")
        result["steam_manifest"] = {k: vdf_value(source, k) for k in keys}
    if not data:
        return result
    required = ["globalgamemanagers", "Managed/Assembly-CSharp.dll", "../hollow_knight.exe"]
    result["required_files"] = {name: (data / name).is_file() for name in required}
    result["layout_detected"] = all(result["required_files"].values())
    with (data / "globalgamemanagers").open("rb") as stream:
        header = stream.read(4096)
    result["unity_version_candidates"] = sorted(set(
        value.decode("ascii") for value in re.findall(rb"\d{4}\.\d+\.\d+[abfp]\d+", header)))
    result["managed_assemblies"] = [
        {"name": p.name, "bytes": p.stat().st_size}
        for p in sorted((data / "Managed").glob("*.dll"))]
    result["content_counts"] = {
        "level_files": sum(p.is_file() and re.fullmatch(r"level\d+", p.name) is not None
                           for p in data.iterdir()),
        "shared_asset_files": sum(p.is_file() for p in data.glob("sharedassets*.assets")),
    }
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hollow-knight", default=os.environ.get("HK_DIR"),
                        help="Windows install or *_Data folder, using its host path (also HK_DIR)")
    args = parser.parse_args()
    candidates = []
    if args.hollow_knight:
        candidates.append((Path(args.hollow_knight).expanduser().resolve(), None))
    else:
        for library in steam_libraries():
            manifest = library / f"steamapps/appmanifest_{APP_ID}.acf"
            name = "Hollow Knight"
            if manifest.is_file():
                name = vdf_value(manifest.read_text(errors="replace"), "installdir") or name
            install = library / "steamapps/common" / name
            if install.is_dir() or manifest.is_file():
                candidates.append((install, manifest))
    installs = []
    for install, manifest in candidates:
        try:
            installs.append(inspect(install, manifest))
        except OSError as error:
            installs.append({"install": str(install), "layout_detected": False, "error": str(error)})
    report = {
        "schema_version": 2, "app_id": APP_ID, "source_platform": "Windows (CrossOver)",
        "note": "Layout discovery only. Steam completion and asset integrity are not verified.",
        "installs": installs,
        "host_tools": {name: shutil.which(name) for name in
                       ("python3", "cargo", "rustup", "mipsel-none-elf-objdump", "ffmpeg", "dotnet")},
        "sibling_checkouts": {name: (ROOT.parent / name).is_dir() for name in
                              ("PSoXide", "PSoXide-editor", "PSoXide-emulator", "hl-psx")},
    }
    output = ROOT / ".hkpsx/doctor.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n")
    found = any(item["layout_detected"] for item in installs)
    print("Windows Hollow Knight layout detected." if found else "No Windows Hollow Knight layout detected in CrossOver. Use --hollow-knight with its host path; macOS installs are excluded.")
    for item in installs:
        print(f"  Source: {item['install']}")
        if item.get("unity_version_candidates"):
            print("  Unity header: " + ", ".join(item["unity_version_candidates"]))
    print(report["note"])
    print(f"Report: {output}")
    return 0 if found else 1


if __name__ == "__main__":
    sys.exit(main())
