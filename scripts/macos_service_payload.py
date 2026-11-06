"""Service-mode Python leaves required in both local and release Mac apps.

These are copied before the outer signature seals Resources. This module never
signs, installs, registers, starts or stops an app or service.
"""
import os
import stat

SERVICE_SCRIPTS = ("install-macos-desktop-service.py", "desktop-revoke-hold.py",
                   "desktop-service-activation.py")
SERVICE_RESOURCES = tuple("Contents/Resources/setup/" + name for name in SERVICE_SCRIPTS)
LIMIT = 128 * 1024


def findings(bundle):
    errors = []
    for relative in SERVICE_RESOURCES:
        path = bundle / relative
        try:
            node = path.lstat()
        except FileNotFoundError:
            errors.append(f"missing {relative}")
            continue
        if not stat.S_ISREG(node.st_mode) or node.st_nlink != 1 or not 0 < node.st_size <= LIMIT:
            errors.append(f"{relative}: service helper must be a nonempty single-link regular file")
    return errors


def require_payload(bundle):
    errors = findings(bundle)
    if errors:
        raise ValueError("\n".join(errors))


def stage(root, bundle):
    for relative in ("Contents", "Contents/Resources", "Contents/Resources/setup"):
        path = bundle / relative
        path.mkdir(exist_ok=True)
        if not stat.S_ISDIR(path.lstat().st_mode):
            raise ValueError("The service setup directory must not be a link or special file.")
    for name, relative in zip(SERVICE_SCRIPTS, SERVICE_RESOURCES):
        source, destination = root / "scripts" / name, bundle / relative
        node = source.lstat()
        if not stat.S_ISREG(node.st_mode) or not 0 < node.st_size <= LIMIT:
            raise ValueError("The service helper source must be an ordinary bounded file.")
        fd = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0))
        with os.fdopen(fd, "rb") as stream:
            held = os.fstat(stream.fileno())
            data = stream.read(LIMIT + 1)
            after = os.fstat(stream.fileno())
        if ((node.st_dev, node.st_ino) != (held.st_dev, held.st_ino)
                or (held.st_size, held.st_mtime_ns, held.st_ctime_ns)
                != (after.st_size, after.st_mtime_ns, after.st_ctime_ns)
                or len(data) != held.st_size):
            raise ValueError("The service helper source changed while copying.")
        if destination.exists() or destination.is_symlink():
            old = destination.lstat()
            if not stat.S_ISREG(old.st_mode) or old.st_nlink != 1:
                raise ValueError("The staged service helper must not be a link or special file.")
        fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_NOFOLLOW", 0), 0o644)
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
        destination.chmod(0o644)
    require_payload(bundle)
