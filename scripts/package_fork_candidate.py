#!/usr/bin/env python3
"""Assemble or verify a matching unsigned Windows/WSL fork candidate.

Assembly runs on Linux, explicitly executes the selected Linux receiver and
collects its locked dependency notices. Verification is data-only on any OS.
Neither operation installs, starts services, replaces an application or publishes.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import subprocess
import tempfile

import fork_identity
import windows_portable

ROOT = Path(__file__).resolve().parents[1]
MAX_FILE = 512 * 1024 * 1024
MAX_MANIFEST = 1024 * 1024
MAX_FILES = 10000


def require(condition, message):
    fork_identity.require(condition, message)


def digest(path):
    require(path.is_file() and not path.is_symlink(), "Artifact must be a regular non-symlink file")
    require(path.stat().st_size <= MAX_FILE, "Artifact exceeds the candidate file bound")
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def safe_relative(value):
    require(isinstance(value, str) and value and len(value) <= 512, "Invalid artifact path")
    path = PurePosixPath(value)
    require(not path.is_absolute() and path.as_posix() == value and all(part not in ("", ".", "..") for part in path.parts)
            and not any(ord(c) < 32 or c in '\\:< >"|?*'.replace(' ', '') for c in value), "Unsafe artifact path")
    require(not any(part.endswith((".", " ")) for part in path.parts), "Unsafe artifact path suffix")
    windows_portable.safe_name(windows_portable.TOP + "/" + value)
    return path


def contained_file(directory, relative):
    target = directory
    for part in relative.parts:
        target = target / part
        require(not target.is_symlink(), "Candidate path traverses a symbolic link")
    require(target.is_file(), "Candidate artifact is missing or not a regular file")
    return target


def validate_pair(windows, linux, commit, version):
    require(windows.get("distribution") == fork_identity.DISTRIBUTION
            and windows.get("fork_version") == fork_identity.fork_version(version)
            and windows.get("fork_channel") == "candidate" and windows.get("source_commit") == commit,
            "Windows component does not match the fork candidate")
    fork_identity.cli_identity(windows.get("cli_build"), commit, version, "windows")
    fork_identity.gui_identity(windows.get("gui_build"), commit, version)
    fork_identity.cli_identity(linux, commit, version, "linux")
    require(windows["cli_build"]["version"] == linux["version"], "Receiver/client base versions differ")


def verify(directory):
    directory = Path(directory)
    require(directory.is_dir() and not directory.is_symlink(), "Candidate root must be a real directory")
    path = directory / "fork-candidate.json"
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= MAX_MANIFEST, "Invalid candidate manifest")
    manifest = json.loads(path.read_text(encoding="utf-8"))
    require(isinstance(manifest, dict) and type(manifest.get("schema_version")) is int and manifest["schema_version"] == 1,
            "Unsupported candidate manifest schema")
    require(manifest.get("distribution") == fork_identity.DISTRIBUTION and manifest.get("channel") == "candidate"
            and manifest.get("signed") is False and manifest.get("published") is False, "Candidate cannot claim a signed release")
    commit, version = manifest.get("source_commit"), manifest.get("fork_version")
    rows = manifest.get("files")
    require(isinstance(rows, list) and 0 < len(rows) <= MAX_FILES, "Invalid candidate inventory")
    names, folded, total = set(), set(), 0
    for row in rows:
        require(isinstance(row, dict), "Invalid inventory row")
        relative = safe_relative(row.get("path")); name = relative.as_posix()
        require(name != "fork-candidate.json" and name.casefold() not in folded, "Duplicate or recursive manifest entry")
        require(type(row.get("bytes")) is int and 0 <= row["bytes"] <= MAX_FILE, "Invalid artifact size")
        require(isinstance(row.get("sha256"), str) and len(row["sha256"]) == 64, "Invalid artifact digest")
        target = contained_file(directory, relative)
        require(target.stat().st_size == row["bytes"] and digest(target) == row["sha256"], "Candidate artifact integrity mismatch: " + name)
        total += row["bytes"];names.add(name);folded.add(name.casefold())
    require(total <= 768 * 1024 * 1024, "Candidate exceeds its total size bound")
    actual = set()
    for count, path in enumerate(directory.rglob("*")):
        require(count < MAX_FILES * 2, "Candidate tree exceeds its entry bound")
        require(not path.is_symlink(), "Candidate cannot contain symbolic links")
        if path.is_file() and path != directory / "fork-candidate.json": actual.add(path.relative_to(directory).as_posix())
    require(actual == names, "Candidate inventory is incomplete")
    require({"windows/zerus-windows-x86_64.zip", "linux/bin/hgs", "linux/bin/hgs_state.py", "linux/receiver.json", "linux/licenses/LICENSE"} <= names,
            "Candidate is missing a required component")
    windows = windows_portable.inspect(directory / "windows/zerus-windows-x86_64.zip")
    receiver_info = directory / "linux/receiver.json"
    require(receiver_info.stat().st_size <= 16384, "Receiver metadata exceeds its bound")
    linux = json.loads(receiver_info.read_text(encoding="utf-8"))
    validate_pair(windows, linux, commit, version)
    return manifest


def assemble(windows_zip, linux_binary, output):
    require(platform.system() == "Linux", "Candidate assembly must verify its receiver on Linux")
    output = Path(output).absolute()
    require(not output.exists(), "Candidate output already exists; never overwrite a verified candidate")
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    require(not subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=normal"], cwd=ROOT, text=True).strip(),
            "Assemble candidates only from clean committed source")
    version = fork_identity.fork_version((ROOT / "FORK_VERSION").read_text().strip())
    windows_zip, linux_binary = Path(windows_zip).resolve(), Path(linux_binary).resolve()
    windows_hash = digest(windows_zip)
    windows = windows_portable.inspect(windows_zip)
    linux_hash = digest(linux_binary)
    with linux_binary.open("rb") as stream: header = stream.read(20)
    require(header[:6] == b"\x7fELF\x02\x01" and header[18:20] == b"\x3e\x00", "Receiver is not an x86_64 Linux ELF")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".fork-candidate-", dir=output.parent) as temporary:
        stage = Path(temporary) / "payload";stage.mkdir()
        home = Path(temporary) / "identity-home";home.mkdir(mode=0o700)
        environment = {k: v for k, v in os.environ.items() if not k.startswith("HGS_")}
        environment.update(HOME=str(home), HGS_CONFIG_DIR=str(home / "config"), HGS_STATE_DIR=str(home / "state"))
        linux = json.loads(subprocess.check_output([str(linux_binary), "version", "--json"],
                          env=environment, cwd=home, timeout=15).decode("utf-8"))
        validate_pair(windows, linux, commit, version)
        (stage / "windows").mkdir();shutil.copyfile(windows_zip, stage / "windows/zerus-windows-x86_64.zip")
        (stage / "linux/bin").mkdir(parents=True);shutil.copyfile(linux_binary, stage / "linux/bin/hgs")
        (stage / "linux/bin/hgs").chmod(0o755)
        shutil.copyfile(ROOT / "hgs_state.py", stage / "linux/bin/hgs_state.py")
        (stage / "linux/bin/hgs_state.py").chmod(0o644)
        require(digest(stage / "linux/bin/hgs") == linux_hash == digest(linux_binary), "Receiver changed while copying")
        require(digest(stage / "windows/zerus-windows-x86_64.zip") == windows_hash == digest(windows_zip), "Windows archive changed while copying")
        (stage / "linux/receiver.json").write_text(json.dumps(linux, indent=2) + "\n", encoding="utf-8")
        notices = stage / "linux/licenses";notices.mkdir()
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):shutil.copyfile(ROOT / name, notices / name)
        subprocess.run(["python3", str(ROOT / "scripts/collect-licenses.py"), str(notices / "rust")], cwd=ROOT, check=True)
        (stage / "README.txt").write_text(
            "Roman Ilyin Zerus fork candidate " + version + "\nSource: " + commit + "\n\n"
            "Unsigned local candidate; not a published stable release. Both components must be used together.\n"
            "Windows: extract windows/zerus-windows-x86_64.zip into a new versioned folder.\n"
            "Linux/WSL: linux/bin/hgs is the matching receiver; keep the native agent installed separately.\n"
            "Point an explicit distro/user connection at the versioned receiver. Do not replace a live application.\n"
            "Keep the prior client folder, receiver and connection settings together for explicit rollback.\n"
            "This bundle starts no service and contains no native agent, account state, keys or machine settings.\n"
            "Verification checks integrity, not publisher authenticity; retain the externally recorded manifest SHA-256.\n",
            encoding="utf-8")
        files = [{"path":p.relative_to(stage).as_posix(),"bytes":p.stat().st_size,"sha256":digest(p)}
                 for p in sorted(stage.rglob("*")) if p.is_file()]
        manifest = {"schema_version":1,"distribution":fork_identity.DISTRIBUTION,"fork_version":version,
                    "channel":"candidate","source_commit":commit,"signed":False,"published":False,
                    "linux_libc":{"family":platform.libc_ver()[0],"version":platform.libc_ver()[1]},"files":files}
        (stage / "fork-candidate.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        verify(stage)
        stage.rename(output)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", type=Path)
    parser.add_argument("--windows-zip", type=Path)
    parser.add_argument("--linux-receiver", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.verify:
        require(not any([args.windows_zip,args.linux_receiver,args.output]), "Verification cannot also build a candidate")
        directory = args.verify;verify(directory)
    else:
        require(all([args.windows_zip,args.linux_receiver,args.output]), "Assembly requires Windows zip, Linux receiver and a fresh output")
        directory = assemble(args.windows_zip,args.linux_receiver,args.output)
    print(json.dumps({"candidate":str(directory),"manifest_sha256":digest(directory / "fork-candidate.json"),"verified":True}))


if __name__ == "__main__":
    try:main()
    except (ValueError,OSError,subprocess.SubprocessError) as error:raise SystemExit(str(error)) from None
