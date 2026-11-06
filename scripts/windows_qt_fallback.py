"""WhiteWh fork, task #65 (owner, 2026-11-06): the Qt GUI as a fallback in the
fork's Tauri zip, for one release.

The tested Qt hgs-tray.exe goes to qt/zerus-qt.exe with its own Qt runtime
(windeployqt with the rules of scripts/package-windows-qt.py), the app-local
MSVC runtime and the Qt notices under qt/licenses. hgs-tray.exe stays the
Tauri app; the owner starts the fallback by hand, and it uses the hgs.exe of
the folder above it (AppConfig::resolveHgsPath). windows_portable.inspect
accepts Qt files only in that folder, declared as "qt_fallback".
"""
from pathlib import Path
import re
import shutil
import subprocess

from release_common import require
from windows_portable import QT_FALLBACK, missing_license_texts

ROOT = Path(__file__).resolve().parents[1]
CRT_REQUIRED = ("vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll")


def qt_version(qt_root):
    """The exact Qt release, read from its CMake package version file."""
    for name in ("Qt6CoreConfigVersionImpl.cmake", "Qt6CoreConfigVersion.cmake"):
        path = qt_root / "lib" / "cmake" / "Qt6Core" / name
        if path.is_file():
            match = re.search(r'set\(PACKAGE_VERSION "(\d+\.\d+\.\d+)"\)', path.read_text(encoding="utf-8"))
            if match:
                return match[1]
    raise ValueError(f"Cannot determine the Qt version under {qt_root}.")


def _copy(source, destination):
    require(source.is_file() and not source.is_symlink(), f"Missing bundle input: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def deploy(stage, gui, qt_root, crt, windeployqt=None):
    """Deploys the Qt fallback into stage/qt; returns its build-info declaration."""
    qt_root = Path(qt_root).resolve()
    version = qt_version(qt_root)
    folder = stage / QT_FALLBACK
    executable = folder / "zerus-qt.exe"
    _copy(Path(gui), executable)
    tool = Path(windeployqt) if windeployqt else qt_root / "bin" / "windeployqt.exe"
    require(tool.is_file(), f"windeployqt is missing: {tool}")
    # The plugins and exclusions of package-windows-qt.py: the SVG plugins load
    # at run time only; QML debugging, TUIO touch and NMEA position are unused;
    # no compiler runtime installer, Mesa OpenGL or Direct3D compilers.
    subprocess.run([str(tool), "--release", "--no-compiler-runtime", "--no-opengl-sw",
                    "--no-system-d3d-compiler", "--no-system-dxc-compiler",
                    "--include-plugins", "qsvg,qsvgicon", "--skip-plugin-types", "qmltooling,generic",
                    "--exclude-plugins", "qtposition_nmea", str(executable)], check=True)
    # windeployqt copies the build machine's ICU forwarder; Qt6Core uses the
    # system ICU, which is never redistributed.
    for path in sorted(folder.glob("*.dll")):
        if path.name.lower().startswith("icu"):
            path.unlink()
    for dll in sorted(Path(crt).glob("*.dll")):
        _copy(dll, folder / dll.name)
    missing = [name for name in CRT_REQUIRED if not (folder / name).is_file()]
    require(not missing, f"The MSVC runtime under {crt} lacks: {', '.join(missing)}")
    licenses = folder / "licenses"
    for name, source in (("libvterm-LICENSE", "tray/vendor/libvterm/LICENSE"), ("md4c-LICENSE", "tray/vendor/md4c/LICENSE.md"),
                         ("qrcodegen-LICENSE", "tray/vendor/qrcodegen/LICENSE")):
        _copy(ROOT / source, licenses / name)
    for text in sorted((ROOT / "packaging/windows/licenses").glob("*.txt")):
        _copy(text, licenses / "qt" / text.name)
    # Qt's SPDX documents per module, and a text for each license they name.
    sboms = sorted(path for path in (qt_root / "sbom").glob("*.spdx") if not path.name.endswith(".source.spdx"))
    names = {path.name for path in sboms}
    require(f"qtbase-{version}.spdx" in names and f"qtmultimedia-{version}.spdx" in names,
            "The Qt installation lacks its SBOM notices; review Qt licensing before distribution.")
    for sbom in sboms:
        _copy(sbom, licenses / "qt" / "sbom" / sbom.name)
    texts = {path.stem for path in (licenses / "qt").glob("*.txt")}
    absent = missing_license_texts({sbom.name: sbom.read_text(encoding="utf-8") for sbom in sboms}, texts)
    require(not absent, "Qt SPDX documents name licenses without a text in packaging/windows/licenses: " + ", ".join(absent))
    return {"executable": f"{QT_FALLBACK}/zerus-qt.exe", "qt_version": version}
