#!/usr/bin/env python3
"""Install one verified connector artifact without building or managing services."""
import hashlib
import os
from pathlib import Path
import re
import stat
import sys
import tempfile


def trusted_parents(path):
    for parent in (path, *path.parents):
        if not parent.exists() and not parent.is_symlink():
            continue
        info = parent.lstat()
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid not in (0, os.geteuid())
                or info.st_mode & 0o022):
            raise ValueError("installation directory is not trusted")


def install(source, expected, prefix):
    if not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError("expected SHA256 must be 64 lowercase hexadecimal characters")
    source = Path(source)
    destination = Path(prefix) / "bin/zerus-mobile-connector"
    if not source.is_absolute() or not destination.is_absolute():
        raise ValueError("artifact and installation prefix must be absolute paths")
    trusted_parents(source.parent)
    trusted_parents(destination.parent)
    descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as artifact:
        before = os.fstat(artifact.fileno())
        if (not stat.S_ISREG(before.st_mode) or before.st_uid not in (0, os.geteuid())
                or before.st_mode & 0o022 or not 0 < before.st_size <= 64 * 1024 * 1024):
            raise ValueError("artifact is not a trusted bounded regular file")
        data = artifact.read(64 * 1024 * 1024 + 1)
        after = os.fstat(artifact.fileno())
        identity = lambda info: (info.st_dev, info.st_ino, info.st_size,
                                 info.st_mtime_ns, info.st_ctime_ns, info.st_mode, info.st_uid)
        if (identity(before) != identity(after) or len(data) != before.st_size
                or hashlib.sha256(data).hexdigest() != expected):
            raise ValueError("artifact changed or SHA256 did not match")
    if destination.is_symlink():
        raise ValueError("installed connector must not be a symlink")
    if destination.exists():
        old = destination.lstat()
        if (not stat.S_ISREG(old.st_mode) or old.st_uid != os.geteuid()
                or old.st_mode & 0o022):
            raise ValueError("installed connector is not a trusted regular file")
        if old.st_size == len(data) and destination.read_bytes() == data:
            print("Connector already matches the verified artifact; nothing changed.")
            return
    destination.parent.mkdir(parents=True, exist_ok=True)
    trusted_parents(destination.parent)
    descriptor, temporary = tempfile.mkstemp(prefix=".zerus-mobile-connector.", dir=destination.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            os.fchmod(output.fileno(), 0o755)
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
        directory = os.open(destination.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print("Installed the verified mobile connector; no service action was performed.")


if __name__ == "__main__":
    try:
        install(*sys.argv[1:])
    except (OSError, ValueError, TypeError) as error:
        raise SystemExit(f"connector installation refused: {error}") from None
