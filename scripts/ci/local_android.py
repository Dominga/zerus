"""Prepare public JDK/Android tools in the selected private cache, not the host."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import urllib.request
import xml.etree.ElementTree as ET
import zipfile


def fetch(url):
    if not url.startswith("https://"):
        raise ValueError("tool downloads require HTTPS")
    request = urllib.request.Request(url, headers={"User-Agent": "Zerus-local-ci/1.0"})
    return urllib.request.urlopen(request, timeout=90)


def download(url, path, expected, algorithm="sha256"):
    with fetch(url) as response, path.open("wb") as stream:
        shutil.copyfileobj(response, stream)
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, algorithm).hexdigest()
    if actual != expected:
        raise ValueError("official tool archive checksum mismatch")
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def java(base):
    target = base / "java17"
    metadata = json.load(fetch("https://api.adoptium.net/v3/assets/latest/17/hotspot?architecture=x64&image_type=jdk&os=linux"))
    packages = [entry["binary"]["package"] for entry in metadata]
    if len(packages) != 1:
        raise ValueError("official JDK did not resolve uniquely")
    package = packages[0]
    receipt = base / "java17.json"
    if receipt.exists() and json.loads(receipt.read_text()).get("sha256") == package["checksum"]:
        return target
    archive = base / "java17.tar.gz"
    sha = download(package["link"], archive, package["checksum"])
    staging = base / "java-unpack"
    staging.mkdir()
    with tarfile.open(archive) as bundle:
        bundle.extractall(staging, filter="data")
    roots = list(staging.iterdir())
    if len(roots) != 1 or not (roots[0] / "bin/java").is_file():
        raise ValueError("unrecognized official JDK archive")
    if target.exists():
        shutil.rmtree(target)
    roots[0].rename(target)
    staging.rmdir()
    archive.unlink()
    receipt.write_text(json.dumps({"url": package["link"], "sha256": sha}, indent=2))
    return target


def android(base):
    sdk = base / "android-sdk"
    target = sdk / "cmdline-tools/latest"
    document = ET.parse(fetch("https://dl.google.com/android/repository/repository2-3.xml"))
    candidates = []
    for package in document.getroot().findall("{*}remotePackage"):
        if not re.fullmatch(r"cmdline-tools;[0-9.]+", package.get("path", "")):
            continue
        channel = package.find("{*}channelRef")
        if channel is not None and channel.get("ref") != "channel-0":
            continue
        revision = tuple(int(package.findtext("{*}revision/{*}" + part, "0"))
                         for part in ("major", "minor", "micro"))
        for archive in package.findall("{*}archives/{*}archive"):
            if archive.findtext("{*}host-os") == "linux":
                complete = archive.find("{*}complete")
                candidates.append((revision, complete.findtext("{*}url"), complete.findtext("{*}checksum")))
    if not candidates:
        raise ValueError("official Android command-line tools did not resolve")
    revision, filename, checksum = max(candidates)
    url = "https://dl.google.com/android/repository/" + filename
    receipt = base / "android-tools.json"
    if not (receipt.exists() and json.loads(receipt.read_text()).get("checksum") == checksum):
        archive = base / "android-tools.zip"
        algorithm = "sha256" if len(checksum) == 64 else "sha1"
        sha = download(url, archive, checksum, algorithm)
        staging = base / "android-unpack"
        staging.mkdir()
        with zipfile.ZipFile(archive) as bundle:
            for item in bundle.infolist():
                path = Path(item.filename)
                if path.is_absolute() or ".." in path.parts or stat.S_ISLNK(item.external_attr >> 16):
                    raise ValueError("unsafe tool archive member")
            bundle.extractall(staging)
        source = staging / "cmdline-tools"
        if not (source / "bin/sdkmanager").is_file():
            raise ValueError("unrecognized official Android tools archive")
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            shutil.rmtree(target)
        source.rename(target)
        for binary in (target / "bin").iterdir():
            binary.chmod(binary.stat().st_mode | 0o111)
        staging.rmdir()
        archive.unlink()
        receipt.write_text(json.dumps({"revision": revision, "url": url, "checksum": checksum, "sha256": sha}, indent=2))
    subprocess.run([str(target / "bin/sdkmanager"), "--sdk_root=" + str(sdk), "--licenses"],
                   input="y\n" * 100, text=True, check=True, timeout=180)
    return sdk


def main():
    base = Path(sys.argv[1])
    base.mkdir(parents=True, exist_ok=True)
    jdk = java(base)
    import os
    os.environ["JAVA_HOME"] = str(jdk)
    sdk = android(base)
    print(json.dumps({"java": str(jdk), "android": str(sdk)}))


if __name__ == "__main__":
    main()
