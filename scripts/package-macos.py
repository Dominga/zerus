#!/usr/bin/env python3
"""Assemble the unsigned Apple Silicon Zerus.app from verified build outputs.

The result adds the hgs CLI, connector, per-user setup command and every notice
to the selected GUI, thins all code to arm64 and applies
an ad-hoc signature so the bundle can be checked locally. The macos-signing job
replaces that signature with the Developer ID one. Nothing is published here.
"""
import argparse
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess

import macos_service_payload

ROOT = Path(__file__).resolve().parents[1]
MINIMUM_SYSTEM = "14.0"
# Installers zerus-setup runs from Contents/Resources/setup, with the local modules they import.
SETUP_SCRIPTS = ("install-macos-session-service.py", "install-recovery-service.py",
                 "install-swarm-service.py", "launchd_agent.py", "install-mobile-connector-service.py",
                 *macos_service_payload.SERVICE_SCRIPTS)


def run(*command):
    subprocess.run([str(part) for part in command], check=True)


def thin_to_arm64(bundle):
    """Replace every universal Mach-O file with its arm64 slice."""
    for directory, _, files in os.walk(bundle):
        for name in files:
            path = Path(directory) / name
            if path.is_symlink():
                continue
            with path.open("rb") as stream:
                if stream.read(4) not in (b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"):
                    continue
            architectures = subprocess.check_output(["lipo", "-archs", str(path)], text=True).split()
            if architectures != ["arm64"]:
                mode = path.stat().st_mode
                run("lipo", path, "-thin", "arm64", "-output", path.with_name(name + ".arm64"))
                path.with_name(name + ".arm64").replace(path)
                path.chmod(mode)


def install(source, destination, mode=0o644):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.chmod(mode)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--gui", choices=("tauri",), default="tauri")
    parser.add_argument("--app", type=Path, required=True, help="tested release GUI app")
    parser.add_argument("--cli", type=Path, required=True, help="release hgs executable")
    parser.add_argument("--lore", type=Path, required=True, help="release lore executable (hgs subprocess)")
    parser.add_argument("--connector", type=Path, required=True,
                        help="release zerus-mobile-connector executable (services/mobile/connector)")
    parser.add_argument("--licenses", type=Path, required=True, help="output of collect-licenses.py")
    parser.add_argument("--client-licenses", type=Path, help="Cargo notices for the Tauri video build")
    parser.add_argument("--frontend-licenses", type=Path, help="locked frontend dependency notices")
    parser.add_argument("--version", required=True)
    parser.add_argument("--release", help="stable, nightly or numbered Dev release (default: --version)")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.version != (ROOT / "VERSION").read_text().strip():
        parser.error("bundle version must match VERSION")
    if args.release:
        nightly = re.fullmatch(re.escape(args.version) + r"(?:\.r\d+\.g[0-9a-f]{7}\.n\d+)?", args.release)
        dev = re.fullmatch(re.escape((ROOT / "DEV_VERSION").read_text().strip()) + r"-dev\.[1-9]\d*", args.release)
        if not nightly and not dev:
            parser.error("--release must match the stable version or the numbered nightly/Dev source")
    for directory in (args.client_licenses, args.frontend_licenses):
        if directory is None or not (directory / "inventory.json").is_file():
            parser.error("collect the Tauri Cargo and frontend notices first")
    if not (args.licenses / "inventory.json").is_file():
        parser.error("collect Cargo dependency notices first")
    source_info = (args.app / "Contents/Info.plist").read_bytes()
    info = plistlib.loads(source_info)
    if (info.get("CFBundleShortVersionString") != args.version
            or info.get("CFBundleIdentifier") not in ("com.hgdev.hgs-tray.local", "com.hgdev.hgs-tray")):
        parser.error("the GUI build does not match this version and bundle identifier")
    if args.gui == "tauri" and (info.get("ZerusGUI") != "tauri"
                                or info.get("ZerusReleaseVersion") != (args.release or args.version)):
        parser.error("the source must be the Tauri switch bundle for this release")
    if args.output.exists() and any(args.output.iterdir()):
        parser.error("use an empty output directory")
    args.output.mkdir(parents=True, exist_ok=True)
    work = args.output / "work"
    # Keep the installed executable name while assembling the public bundle.
    staged = work / "hgs-tray.app"
    run("ditto", args.app, staged)
    info_path = staged / "Contents/Info.plist"
    if info_path.read_bytes() != source_info:
        parser.error("the source bundle changed while staging")
    # LaunchServices needs CFBundleExecutable once the bundle is no longer named hgs-tray.app.
    # Terminal integration runs osascript; under the hardened runtime TCC attributes its
    # Apple Events to Zerus, which therefore declares their purpose.
    info.update(CFBundleIdentifier="com.hgdev.hgs-tray", CFBundleName="Zerus", CFBundleDisplayName="Zerus", CFBundleExecutable="hgs-tray",
                CFBundleVersion=args.version, LSMinimumSystemVersion=MINIMUM_SYSTEM,
                ZerusReleaseVersion=args.release or args.version,
                NSAppleEventsUsageDescription="Zerus opens your agent sessions in Terminal.")
    info_path.write_bytes(plistlib.dumps(info))

    contents = staged / "Contents"
    install(args.cli, contents / "MacOS/hgs", 0o755)
    install(args.lore, contents / "MacOS/lore", 0o755)
    # Opt-in through zerus-setup --mobile-connector install; signed with the other code.
    install(args.connector, contents / "MacOS/zerus-mobile-connector", 0o755)
    resources = contents / "Resources"
    install(ROOT / "packaging/macos/zerus-setup", resources / "bin/zerus-setup", 0o755)
    for name in SETUP_SCRIPTS:
        if name not in macos_service_payload.SERVICE_SCRIPTS:
            install(ROOT / "scripts" / name, resources / "setup" / name)
    macos_service_payload.stage(ROOT, staged)
    install(ROOT / "hgs_state.py", resources / "setup/hgs_state.py")
    install(ROOT / "packaging/desktop/com.hgdev.hgs-tray.plist", resources / "setup/com.hgdev.hgs-tray.plist")
    install(ROOT / "README.md", resources / "doc/README.md")
    install(ROOT / "tmux.conf", resources / "doc/tmux.conf")
    licenses = resources / "licenses"
    install(ROOT / "LICENSE", licenses / "LICENSE")
    install(ROOT / "packaging/macos/TAURI-THIRD-PARTY-NOTICES.md", licenses / "THIRD_PARTY_NOTICES.md")
    install(ROOT / "docs/licenses/codex-LICENSE.txt", licenses / "codex-LICENSE.txt")
    shutil.copytree(args.licenses, licenses / "rust")
    shutil.copytree(args.client_licenses, licenses / "client-rust")
    shutil.copytree(args.frontend_licenses, licenses / "frontend")
    thin_to_arm64(staged)
    bundle = args.output / "Zerus.app"
    run("ditto", staged, bundle)
    shutil.rmtree(work)
    macos_service_payload.require_payload(bundle)
    # Thinning invalidates signatures. An ad-hoc signature keeps the unsigned
    # candidate runnable for checks; release signing replaces it.
    run("codesign", "--force", "--deep", "--sign", "-", bundle)
    print(f"Assembled {bundle} for Zerus {args.version} ({args.gui}). Nothing was signed for release.")


if __name__ == "__main__":
    main()
