#!/usr/bin/env python3
"""Complete worktree fixtures under one disposable, unprivileged hosted Mac user.

A runner can own protected OS processes whose identity it cannot read. They
correctly block production cleanup. A separate UID gives the whole fixture,
including its real blockers, its own kernel process inventory without excluding
any process from an individual hgs call. Never create accounts on owner Macs.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import selectors
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
MODULES = ("test_worktrees", "test_worktree_lifecycle", "test_worktree_maintenance")
# Finish before the existing integration driver's ten-minute deadline, including
# admission and cleanup. Exhausting a bound fails the scope; it never retries.
DEADLINE = time.monotonic() + 570


def admit(environment, system):
    if (system != "Darwin" or environment.get("GITHUB_ACTIONS") != "true"
            or environment.get("RUNNER_ENVIRONMENT") != "github-hosted"
            or environment.get("GITHUB_REPOSITORY") not in {"ufna/zerus", "WhiteWh/zerus", "romanilyin/zerus"}):
        raise ValueError("The isolated Mac user requires an authorized disposable hosted runner")


def command(*arguments, check=True):
    remaining = DEADLINE - time.monotonic()
    if remaining <= 0:
        raise TimeoutError("The private fixture scope exhausted its original bound")
    return subprocess.run(arguments, check=check, capture_output=True, text=True, timeout=min(30, remaining))


def privileged(*arguments, check=True):
    return command("/usr/bin/sudo", "-n", "--", *arguments, check=check)


def pids(uid):
    lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    lib.proc_listpids.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_int]
    lib.proc_listpids.restype = ctypes.c_int
    result = set()
    for kind in (4, 5):
        values = (ctypes.c_int * 32768)()
        size = ctypes.sizeof(values)
        count = lib.proc_listpids(kind, uid, values, size)
        if count < 0 or count >= size or count % ctypes.sizeof(ctypes.c_int):
            raise ValueError("The fixture UID inventory is incomplete")
        result.update(pid for pid in values[:count // ctypes.sizeof(ctypes.c_int)] if pid > 0)
    return result


def dscl(*arguments, check=True):
    return privileged("/usr/bin/dscl", ".", *arguments, check=check)


def identity(name):
    result = dscl("-read", "/Users/" + name, "GeneratedUID", "UniqueID", "NFSHomeDirectory", check=False)
    if result.returncode:
        if result.returncode == 56 and "eDSRecordNotFound" in result.stdout + result.stderr:
            return None
        raise ValueError("Directory-service identity could not be verified")
    return dict(line.split(": ", 1) for line in result.stdout.splitlines() if ": " in line)


def user_domain(uid):
    # Looking up user/UID itself materializes an otherwise absent domain.
    # Read the existing parent inventory instead, including after bootout.
    result = privileged("/bin/launchctl", "print", "system", check=False)
    if result.returncode or len(result.stdout) > 1024**2:
        raise ValueError("Private launchd domain could not be verified: " + result.stderr[:300])
    parts = result.stdout.split("\tsubdomains = {\n")
    if len(parts) != 2 or "\n\t}" not in parts[1]:
        raise ValueError("Unexpected launchd subdomain inventory")
    rows = parts[1].split("\n\t}", 1)[0].splitlines()
    domains = set()
    for row in rows:
        label = row.strip()
        if label.startswith("user/"):
            match = re.fullmatch(r"user/([0-9]+)", label)
            if match is None:
                raise ValueError("Unknown user domain identity in the launchd inventory")
            domains.add(int(match[1]))
    return uid in domains


def close_domain(name, uid, expected, absent_before):
    if not absent_before or identity(name) != expected or expected.get("UniqueID") != str(uid):
        raise ValueError("Private launchd ownership changed; no bootout is authorized")
    if user_domain(uid):
        # This domain did not exist before this locked fixture UID was allocated.
        # Its OS services respawn after pkill until the domain itself is closed.
        privileged("/bin/launchctl", "bootout", "user/" + str(uid))
        if user_domain(uid):
            raise ValueError("Private launchd domain survived bootout")
        return True
    return False


def signal_scope(name, uid, expected, signal):
    if identity(name) != expected:
        raise ValueError("Private fixture identity changed; no signal is authorized")
    # Only this locked, newly allocated UID can own these fixture descendants.
    # Signal as that unprivileged user: even a PID race cannot signal another
    # user's process. Both real and effective UID must match; never run pkill as
    # root or target the runner's own UID, process group or launch services.
    result = command("/usr/bin/sudo", "-n", "-u", name, "--", "/usr/bin/pkill",
                     "-" + signal, "-U", str(uid), "-u", str(uid), check=False)
    if result.returncode not in (0, 1):
        raise ValueError("Private fixture processes could not be signalled")


def run(module, binary):
    # Admission is before allocation, sudo or any directory-service mutation.
    admit(os.environ, platform.system())
    source = command("git", "rev-parse", "HEAD").stdout.strip()
    if source != os.environ.get("SOURCE_COMMIT"):
        raise ValueError("Fixture checkout differs from the admitted CI source")
    metadata = binary.lstat()
    if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size < 512 * 1024**2:
        raise ValueError("The selected normal CLI is not a bounded regular file")
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    fixture_tools = {}
    if module == "test_worktree_maintenance":
        # Resolve the hosted runner's selected tools before entering the fresh
        # UID. /usr/bin/cc is a dispatcher, not the selected clang executable.
        developer = Path(os.environ.get("DEVELOPER_DIR") or
                         command("/usr/bin/xcode-select", "--print-path").stdout.strip()).resolve(strict=True)
        compiler = Path(command("/usr/bin/xcrun", "--sdk", "macosx", "--find", "clang").stdout.strip()).resolve(strict=True)
        sdk = Path(command("/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-path").stdout.strip()).resolve(strict=True)
        if (not developer.is_dir() or not compiler.is_file() or not os.access(compiler, os.X_OK)
                or compiler.parent == Path("/usr/bin") or not (sdk / "SDKSettings.plist").is_file()):
            raise ValueError("The selected native fixture compiler or macOS SDK is unavailable")
        fixture_tools = {"HGS_TEST_WORKTREE_CC": str(compiler), "SDKROOT": str(sdk),
                         "DEVELOPER_DIR": str(developer)}
    inventory = dscl("-list", "/Users", "UniqueID").stdout
    used = {int(row.split()[-1]) for row in inventory.splitlines() if row.split() and row.split()[-1].isdigit()}
    uid = next((candidate for candidate in range(60000, 60128)
                if candidate not in used and not pids(candidate) and not user_domain(candidate)), None)
    if uid is None:
        raise ValueError("No empty private fixture UID is available")
    root = Path(tempfile.mkdtemp(prefix="zwt-user-", dir="/private/tmp"))
    root.chmod(0o755)
    name = "_zerus_fixture_" + uuid.uuid4().hex[:12]
    generated = str(uuid.uuid4()).upper()
    home, runtime, code = root/"home", root/"tmp", root/"source"
    expected = {"GeneratedUID": generated, "UniqueID": str(uid), "NFSHomeDirectory": str(home)}
    report = dict(source=source, module=module, uid=uid, user=name, generated=generated,
                  root=str(root), binary_sha256=binary_hash, account_removed=False, tests_started=False,
                  user_domain_absent_before=True)
    if fixture_tools:
        report["fixture_tools"] = dict(fixture_tools,
                                      compiler_sha256=hashlib.sha256(Path(fixture_tools["HGS_TEST_WORKTREE_CC"]).read_bytes()).hexdigest())
    output = ROOT/"artifacts/test-results/mac-worktree-scope"
    output.mkdir(parents=True, exist_ok=True)
    destination = output/(module + ".json")
    if destination.exists():
        raise ValueError("No fixture-scope replay")
    destination.write_text(json.dumps(report, indent=2) + "\n")
    created = False
    try:
        archive = root/"source.tar"
        command("git", "archive", "--format=tar", "--output=" + str(archive), source)
        code.mkdir()
        with tarfile.open(archive) as package:
            entries = package.getmembers()
            if len(entries) > 10000 or sum(entry.size for entry in entries) > 200 * 1024**2:
                raise ValueError("Fixture source is outside its bounds")
            for entry in entries:
                path = Path(entry.name)
                if path.is_absolute() or ".." in path.parts or not (entry.isfile() or entry.isdir()):
                    raise ValueError("Fixture source contains a non-regular member")
            package.extractall(code, filter="data")
        for path in code.rglob("*"):
            path.chmod(0o555 if path.is_dir() or path.stat().st_mode & 0o111 else 0o444)
        code.chmod(0o555)
        hgs = root/"hgs"
        shutil.copyfile(binary, hgs)
        hgs.chmod(0o555)
        if hashlib.sha256(hgs.read_bytes()).hexdigest() != binary_hash:
            raise ValueError("Staged CLI differs from the normal admitted build")
        record = "/Users/" + name
        if identity(name) is not None:
            raise ValueError("Fixture account already exists")
        dscl("-create", record)
        dscl("-create", record, "GeneratedUID", generated)
        created = True
        # Locked password and non-login shell precede the usable UID.
        for key, value in (("UserShell", "/usr/bin/false"), ("Password", "*"),
                           ("RealName", "Zerus isolated fixture"), ("PrimaryGroupID", "20"),
                           ("NFSHomeDirectory", str(home)), ("UniqueID", str(uid))):
            dscl("-create", record, key, value)
        if identity(name) != expected:
            raise ValueError("Fixture account identity was not confirmed")
        for directory in (home, runtime):
            privileged("/usr/bin/install", "-d", "-m", "700", "-o", str(uid), "-g", "20", str(directory))
        paths = os.pathsep.join([str(Path(sys.executable).parent), "/opt/homebrew/bin", "/usr/local/bin",
                                 "/usr/bin", "/bin", "/usr/sbin", "/sbin"])
        prefix = ["/usr/bin/sudo", "-n", "-u", name, "--", "/usr/bin/env", "-i",
                  "HOME=" + str(home), "USER=" + name, "LOGNAME=" + name, "SHELL=/bin/bash",
                  "PATH=" + paths, "TMPDIR=" + str(runtime), "LANG=en_US.UTF-8", "LC_ALL=en_US.UTF-8",
                  "HGS_TEST_BIN=" + str(hgs), "PYTHONDONTWRITEBYTECODE=1"]
        prefix.extend(key + "=" + value for key, value in fixture_tools.items())
        probe = "import os,json,sys; print(json.dumps([os.getpid(),os.getuid(),os.geteuid()]),flush=True); sys.stdin.buffer.read(1)"
        child = subprocess.Popen([*prefix, sys.executable, "-I", "-B", "-c", probe],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(child.stdout, selectors.EVENT_READ)
                if not selector.select(20):
                    raise ValueError("Private interpreter admission did not respond")
            line = child.stdout.readline()
            pid, real, effective = json.loads(line)
            if real != uid or effective != uid or pid not in pids(uid):
                raise ValueError("Private UID or kernel inventory admission failed")
            child.communicate(b"x", timeout=10)
            if child.returncode:
                raise ValueError("Private interpreter admission failed")
        finally:
            if child.poll() is None:
                child.terminate()
                child.communicate(timeout=10)
        entry = ("import os,sys,unittest; "
                 "os.umask(0o077); sys.path.insert(0," + repr(str(code/"tests")) + "); "
                 "unittest.main(module=None,argv=['worktree-scope','-v'," + repr(module) + "])")
        report["tests_started"] = True
        report["interpreter_identity"] = {"pid": pid, "uid": real, "euid": effective}
        destination.write_text(json.dumps(report, indent=2) + "\n")
        completed = subprocess.run([*prefix, sys.executable, "-I", "-B", "-c", entry],
                                   cwd=code, timeout=max(.1, min(480, DEADLINE - time.monotonic() - 40)))
        report["test_exit_code"] = completed.returncode
        return completed.returncode
    finally:
        # Never signal unknown processes, delete a live account or claim cleanup
        # from a successful test result. A failed scope stays for VM disposal.
        try:
            end = min(DEADLINE, time.monotonic() + 20)
            remaining = pids(uid) if created else set()
            report["processes_after_tests"] = sorted(remaining)
            if remaining:
                detail = command("/bin/ps", "-p", ",".join(str(pid) for pid in sorted(remaining)[:128]),
                                 "-o", "pid=,ruid=,uid=,stat=,comm=", check=False)
                report["processes_after_tests_details"] = [line[:512] for line in detail.stdout.splitlines()
                    if len(line.split()) >= 4 and line.split()[1:3] == [str(uid), str(uid)]]
                signal_scope(name, uid, expected, "TERM")
            if created:
                report["private_user_domain_closed"] = close_domain(name, uid, expected,
                                                                   report["user_domain_absent_before"])
                remaining = pids(uid)
            while remaining and time.monotonic() < end:
                time.sleep(.05)
                remaining = pids(uid)
                if remaining and time.monotonic() >= end - 10 and not report.get("scope_kill_sent"):
                    signal_scope(name, uid, expected, "KILL")
                    report["scope_kill_sent"] = True
            report["remaining_processes"] = sorted(remaining)
            report["user_domain_remaining"] = user_domain(uid) if created else None
            if created and not remaining and not report["user_domain_remaining"] and identity(name) == expected:
                dscl("-delete", "/Users/" + name)
                report["account_removed"] = identity(name) is None
            if created and (remaining or not report["account_removed"]):
                raise ValueError("Private fixture account remains; retained for hosted VM disposal")
        except Exception as error:
            report["cleanup_error"] = str(error)
            raise
        finally:
            destination.write_text(json.dumps(report, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--hgs", required=True, type=Path)
    parser.add_argument("module", choices=MODULES)
    args = parser.parse_args()
    return run(args.module, args.hgs.resolve(strict=True))


if __name__ == "__main__":
    sys.exit(main())
