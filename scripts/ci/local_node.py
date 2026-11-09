"""Supply setup-node's requested Linux major in the private job home."""
import hashlib
from pathlib import Path
import re
import shutil
import sys
import tarfile
import urllib.request


def main():
    major, output = sys.argv[1], Path(sys.argv[2])
    if not re.fullmatch(r"[1-9][0-9]", major) or output.exists():
        raise SystemExit("expected a supported numeric major and a fresh private directory")
    base = f"https://nodejs.org/dist/latest-v{major}.x/"
    checksums = urllib.request.urlopen(base + "SHASUMS256.txt", timeout=30).read().decode()
    matches = [line.split() for line in checksums.splitlines()
               if re.fullmatch(rf"[0-9a-f]{{64}}  node-v{major}\.[0-9]+\.[0-9]+-linux-x64.tar.xz", line)]
    if len(matches) != 1:
        raise SystemExit("official Node archive did not resolve uniquely")
    expected, filename = matches[0]
    output.parent.mkdir(parents=True, exist_ok=True)
    archive = output.with_suffix(".tar.xz")
    with urllib.request.urlopen(base + filename, timeout=60) as response, archive.open("wb") as stream:
        shutil.copyfileobj(response, stream)
    with archive.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != expected:
        raise SystemExit("official Node archive SHA256 mismatch")
    staging = output.with_suffix(".unpack")
    staging.mkdir()
    with tarfile.open(archive) as bundle:
        bundle.extractall(staging, filter="data")
    root = staging / filename.removesuffix(".tar.xz")
    root.rename(output)
    staging.rmdir()
    archive.unlink()
    print(filename, expected)


if __name__ == "__main__":
    main()
