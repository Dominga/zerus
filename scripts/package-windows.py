#!/usr/bin/env python3
"""Bundle tested hgs, the Tauri desktop and connector into the Windows zip.

The app-local MSVC runtime and pinned ConPTY pair accompany the unchanged
binaries. WebView2 uses the installed Evergreen runtime; Qt is never deployed.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib
import zipfile

import conpty_pair
from release_common import require
from windows_portable import TOP, inspect

ROOT = Path(__file__).resolve().parents[1]
CRT_REQUIRED = ("vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll")


def visual_studio():
    """The newest Visual Studio with the x64 C++ tools, as vswhere reports it."""
    program_files = os.environ.get("ProgramFiles(x86)") or os.environ.get("ProgramFiles") or r"C:\Program Files (x86)"
    vswhere = Path(program_files) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    require(vswhere.is_file(), f"vswhere is missing: {vswhere}")
    output = subprocess.check_output([str(vswhere), "-latest", "-products", "*", "-requires",
                                      "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                                      "-property", "installationPath", "-utf8"])
    install = output.decode("utf-8").strip()
    require(install and Path(install).is_dir(), "vswhere found no Visual Studio with the x64 C++ tools.")
    return Path(install)


def crt_directory(install):
    """App-local x64 CRT: the default redistributable version, never a hard-coded one."""
    redist = install / "VC" / "Redist" / "MSVC"
    default = install / "VC" / "Auxiliary" / "Build" / "Microsoft.VCRedistVersion.default.txt"
    if default.is_file():
        versions = [default.read_text(encoding="utf-8").strip()]
    else:
        versions = sorted((path.name for path in redist.iterdir() if re.fullmatch(r"\d+(?:\.\d+)+", path.name)),
                          key=lambda value: tuple(map(int, value.split("."))), reverse=True)[:1]
    require(versions, f"No MSVC redistributable under {redist}.")
    # Microsoft.VC143.CRT for Visual Studio 2022, Microsoft.VC145.CRT for 2026.
    folders = sorted((redist / versions[0] / "x64").glob("Microsoft.VC14*.CRT"))
    require(len(folders) == 1, f"Expected one x64 CRT folder under {redist / versions[0]}.")
    return folders[0]


def copy(source, destination):
    require(source.is_file() and not source.is_symlink(), f"Missing bundle input: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def source_commit():
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()


def source_time():
    """Entry time: SOURCE_DATE_EPOCH or the commit time, so rebuilt zips stay comparable."""
    value = os.environ.get("SOURCE_DATE_EPOCH") or subprocess.check_output(
        ["git", "log", "-1", "--format=%ct", "HEAD"], cwd=ROOT, text=True).strip()
    moment = datetime.datetime.fromtimestamp(int(value), datetime.timezone.utc)
    return max(moment.timetuple()[:6], (1980, 1, 1, 0, 0, 0))


def write_zip(stage, output, date_time):
    """Sorted, '/'-separated regular files under one top-level folder."""
    files = []
    for path in sorted(stage.rglob("*")):
        require(not path.is_symlink(), f"Symlinks cannot be bundled: {path}")
        if path.is_file():
            files.append(path)
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in files:
            entry = zipfile.ZipInfo(f"{TOP}/{path.relative_to(stage).as_posix()}", date_time=date_time)
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.create_system = 3
            entry.external_attr = (stat.S_IFREG | 0o644) << 16
            entry.file_size = path.stat().st_size
            with path.open("rb") as source, archive.open(entry, "w") as target:
                shutil.copyfileobj(source, target, 1024 * 1024)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--cli", type=Path, required=True, help="tested hgs.exe")
    parser.add_argument("--connector", type=Path, required=True, help="tested zerus-mobile-connector.exe")
    parser.add_argument("--gui", type=Path, required=True, help="tested hgs-tray.exe")
    # WhiteWh fork: a relay for this computer alone (docs/fork-updates.md).
    parser.add_argument("--mobile-relay", type=Path, help="tested zerus-relay.exe")
    parser.add_argument("--output", type=Path, required=True, help="zip to write")
    parser.add_argument("--crt-dir", type=Path, help="Microsoft.VC14*.CRT folder [default: found with vswhere]")
    parser.add_argument("--rust-licenses", type=Path,
                        help="notices from scripts/collect-licenses.py [default: collected for this host target]")
    parser.add_argument("--frontend-licenses", type=Path, help="collected production frontend notices")
    parser.add_argument("--conpty", type=Path,
                        help="folder with the pinned conpty.dll and OpenConsole.exe from scripts/fetch-conpty.py "
                             "[default: fetched from nuget.org]")
    args = parser.parse_args()
    require(args.output.suffix == ".zip", "The output must be a .zip file.")
    crt = args.crt_dir or crt_directory(visual_studio())
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    cli_version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    manifest = {"product": "Zerus", "version": version, "cli_version": cli_version, "platform": "Windows",
                "architecture": "x86_64", "desktop": "tauri", "webview2_runtime": "evergreen", "native_agents_bundled": False, "mobile_connector_bundled": True,
                "source_commit": source_commit()}
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="zerus-windows-") as temporary:
        stage = Path(temporary) / TOP
        # Microsoft's ConPTY next to hgs.exe (spec D2, D6): `hgs session-host
        # install` stages both files from there into the host image, whose
        # session host loads conpty.dll by absolute path; conpty.dll runs the
        # OpenConsole.exe next to it. Only the two pinned files are taken.
        if args.conpty:
            pair = conpty_pair.check_folder(args.conpty)
        else:
            pair = conpty_pair.fetch(Path(temporary) / "conpty")
        copy(args.cli, stage / "hgs.exe")
        copy(args.gui, stage / "hgs-tray.exe")
        copy(args.connector, stage / "zerus-mobile-connector.exe")
        if args.mobile_relay:
            copy(args.mobile_relay, stage / "zerus-relay.exe")
        for dll in sorted(crt.glob("*.dll")):
            copy(dll, stage / dll.name)
        missing = [name for name in CRT_REQUIRED if not (stage / name).is_file()]
        require(not missing, f"The MSVC runtime under {crt} lacks: {', '.join(missing)}")
        for name, path in pair.items():
            copy(path, stage / name)
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            copy(ROOT / name, stage / name)
        licenses = stage / "licenses"
        copy(ROOT / "docs/licenses/codex-LICENSE.txt", licenses / "codex-LICENSE.txt")
        copy(ROOT / "docs/licenses/conpty-LICENSE.txt", licenses / "conpty-LICENSE.txt")
        copy(ROOT / "docs/licenses/conpty-NOTICE.md", licenses / "conpty-NOTICE.md")
        if args.rust_licenses:
            require((args.rust_licenses / "inventory.json").is_file(), "Collect dependency license texts first.")
            shutil.copytree(args.rust_licenses, licenses / "rust")
        else:
            relay = ["--manifest-path", str(ROOT / "services/mobile/relay/Cargo.toml")] if args.mobile_relay else []
            subprocess.run([sys.executable, str(ROOT / "scripts/collect-licenses.py"), str(licenses / "rust"),
                            "--manifest-path", str(ROOT / "Cargo.toml"),
                            "--manifest-path", str(ROOT / "services/mobile/connector/Cargo.toml"), *relay],
                           cwd=ROOT, check=True)
            subprocess.run([sys.executable, str(ROOT / "scripts/collect-licenses.py"), str(licenses / "rust/desktop"),
                            "--manifest-path", str(ROOT / "client/src-tauri/Cargo.toml"),
                            "--manifest-features", f"{ROOT / 'client/src-tauri/Cargo.toml'}=video"],
                           cwd=ROOT, check=True)
        require((licenses / "rust/desktop/inventory.json").is_file(), "Collect Tauri dependency license texts first.")
        if args.frontend_licenses:
            require((args.frontend_licenses / "inventory.json").is_file(), "Collect frontend dependency license texts first.")
            shutil.copytree(args.frontend_licenses, licenses / "frontend")
        else:
            subprocess.run([sys.executable, str(ROOT / "client/scripts/collect-notices.py"),
                            str(licenses / "frontend")], cwd=ROOT, check=True)
        readme = (ROOT / "packaging/windows/README.txt").read_text(encoding="utf-8")
        for key in ("version", "cli_version", "source_commit"):
            readme = readme.replace(f"@{key.upper()}@", manifest[key])
        readme = readme.replace("@CONPTY_VERSION@", conpty_pair.PIN["version"])
        require(not re.search(r"@[A-Z_]+@", readme), "Unreplaced placeholder in the Windows README.")
        # Notepad and other Windows tools expect CRLF in a plain-text README.
        (stage / "README.txt").write_bytes(readme.replace("\n", "\r\n").encode("utf-8"))
        (stage / "build-info.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        partial = output.with_name(output.name + ".partial")
        partial.unlink(missing_ok=True)
        try:
            write_zip(stage, partial, source_time())
            # The same layout and manifest checks the release publishers repeat.
            inspect(partial)
        except BaseException:
            partial.unlink(missing_ok=True)
            raise
    os.replace(partial, output)
    print(f"Wrote {output} ({output.stat().st_size} bytes) for Zerus {version}, hgs {cli_version}, "
          f"Tauri/WebView2 Evergreen, commit {manifest['source_commit']}.")


if __name__ == "__main__":
    try:
        main()
    except ValueError as error:
        raise SystemExit(f"package-windows: {error}") from None
