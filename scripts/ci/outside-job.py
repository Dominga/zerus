#!/usr/bin/env python3
"""Run a test command outside the CI step's job object (Windows CI only).

GitHub's Windows runner starts every step inside a job that forbids breakaway, so
a session host started from a step lands in that job and hgs refuses it (spec D4,
docs/windows-host-testing.md). This is a CI harness for the host suites only: the
product keeps D4 and never escapes a job through WMI.

WMI's Win32_Process.Create starts this script again outside any job. That copy
restores the step's environment explicitly (WMI processes do not inherit it;
credentials are left out), joins a kill-on-close job of its own that allows
breakaway, as an sshd session does, runs the command there with stdin from NUL,
and writes its output and exit status to files that this side follows. When the
command ends, or this side times out and ends the copy, the job ends everything
the command left, and session hosts that legitimately broke away from it are
ended by executable path (--end, a PowerShell -like pattern). A detached-connector
fixture instead uses --custody-only, which forbids --end and retains the wrapper,
private fixture and unknown outcome on timeout, without PID/tree cleanup. Usage:

    python scripts/ci/outside-job.py --log FILE [--timeout SECONDS] [--end EXE]... -- COMMAND...

It exits with the command's status, or 124 after the timeout. Cleanup errors
cannot turn a failed or unknown outcome into success. Cleanup subprocesses have
a shared 120 s allowance and a 30 s per-action cap. Utilities use file-backed
output and a gated non-breakaway kill-on-close job; timeout never waits for pipe
EOF. The outside copy also enforces the command deadline independently of this
step. These bounds do not preempt host kernel/filesystem or runner logging stalls.
The portable zip/installer is retained before the survival gate; its diagnostics
are uploaded separately with always(), while test failure still fails the job.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

# Step variables that grant access; the command never needs them.
SECRET = re.compile(r"TOKEN|SECRET|PASSWORD|^ACTIONS_(RUNTIME|CACHE|RESULTS|ID_TOKEN)", re.IGNORECASE)
CLEANUP_ALLOWANCE = 120
CLEANUP_ACTION_LIMIT = 30
LOG_CHUNK = 16 * 1024
OUTPUT_LIMIT = 1024 * 1024
KILL_WAIT = 5


def create_job(breakaway=False):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateJobObjectW.restype = ctypes.c_void_p
    kernel.CreateJobObjectW.argtypes = (ctypes.c_void_p, ctypes.c_wchar_p)
    kernel.SetInformationJobObject.argtypes = (ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32)
    job = kernel.CreateJobObjectW(None, None)
    info = (ctypes.c_ubyte * 144)()
    ctypes.c_uint32.from_buffer(info, 16).value = 0x2000 | (0x800 if breakaway else 0)
    if not job:
        raise OSError(ctypes.get_last_error(), "cannot create the command's job")
    if not kernel.SetInformationJobObject(job, 9, info, len(info)):
        error = ctypes.get_last_error()
        kernel.CloseHandle.argtypes = (ctypes.c_void_p,)
        kernel.CloseHandle(job)
        raise OSError(error, "cannot configure the command's job")
    return job


def assign_job(job, process):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.AssignProcessToJobObject.argtypes = (ctypes.c_void_p, ctypes.c_void_p)
    if not kernel.AssignProcessToJobObject(job, process):
        raise OSError(ctypes.get_last_error(), "cannot assign the command's job")


def close_job(job):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.TerminateJobObject.argtypes = (ctypes.c_void_p, ctypes.c_uint)
    kernel.QueryInformationJobObject.argtypes = (ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p,
                                                 ctypes.c_uint32, ctypes.c_void_p)
    kernel.CloseHandle.argtypes = (ctypes.c_void_p,)
    try:
        if not kernel.TerminateJobObject(job, 125):
            raise OSError(ctypes.get_last_error(), "cannot end the utility tree")
        deadline = time.monotonic() + KILL_WAIT
        while True:
            # JOBOBJECT_BASIC_ACCOUNTING_INFORMATION: ActiveProcesses at 40.
            info = (ctypes.c_ubyte * 48)()
            if not kernel.QueryInformationJobObject(job, 1, info, len(info), None):
                raise OSError(ctypes.get_last_error(), "cannot observe utility tree exit")
            if ctypes.c_uint32.from_buffer(info, 40).value == 0:
                break
            if time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired("utility tree exit", KILL_WAIT)
            time.sleep(0.01)
    finally:
        if not kernel.CloseHandle(job):
            raise OSError(ctypes.get_last_error(), "cannot close the command's job")


def action(spec_path):
    """No utility may spawn before its original parent assigns this worker's job.

    Only stdin is a pipe (one fixed release byte); output is always regular files.
    A utility and ALL its descendants therefore lose neither tree custody nor a
    timeout to subprocess.run's unbounded post-kill communicate() on Windows.
    """
    spec = json.loads(Path(spec_path).read_text(encoding="utf-8"))
    Path(spec_path).unlink()
    if os.read(0, 1) != b"G":
        return 125
    return subprocess.call(spec["argv"], env=spec["env"], stdin=subprocess.DEVNULL)


def bounded_run(argv, *, timeout, env=None, capture_output=False, text=False, stdin=None):
    """Finite utility execution, including termination and inherited-output cases.

    The Windows worker waits for assignment to a private non-breakaway,
    kill-on-close job before starting the utility. POSIX uses a private session
    for the same real-component controls. Never communicate() or enter Popen's
    context manager: either can wait forever for a descendant's pipe or exit.
    """
    with tempfile.TemporaryDirectory(prefix="outside-action-") as directory:
        root = Path(directory)
        spec = root / "action.json"
        environment = os.environ if env is None else env
        spec.write_text(json.dumps({"argv": list(map(str, argv)),
                                   "env": {k: v for k, v in environment.items() if not SECRET.search(k)}}),
                        encoding="utf-8")
        python = getattr(sys, "_base_executable", None) or sys.executable
        job = create_job() if os.name == "nt" else None
        process = None
        with open(root / "stdout", "w+b") as out, open(root / "stderr", "w+b") as err:
            try:
                process = subprocess.Popen([python, "-I", str(Path(__file__).resolve()), "--action", str(spec)],
                                           stdin=subprocess.PIPE, stdout=out, stderr=err,
                                           env={k: v for k, v in environment.items() if not SECRET.search(k)},
                                           start_new_session=os.name != "nt")
                if job is not None:
                    assign_job(job, int(process._handle))
                process.stdin.write(b"G")
                process.stdin.close()
                code = process.wait(timeout=timeout)
            finally:
                # Includes successful utility exit: it may have left a descendant
                # holding its output files. No utility inherits the step's pipes.
                if job is not None:
                    close_job(job)
                elif process is not None:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                if process is not None:
                    if process.stdin and not process.stdin.closed:
                        process.stdin.close()
                    process.wait(timeout=KILL_WAIT)
            out.seek(0); err.seek(0)
            stdout, stderr = out.read(OUTPUT_LIMIT), err.read(OUTPUT_LIMIT)
        if not capture_output:
            for stream, data in ((sys.stdout, stdout), (sys.stderr, stderr)):
                if data:
                    stream.write(data.decode("utf-8", "replace")); stream.flush()
            stdout = stderr = None
        elif text:
            stdout, stderr = stdout.decode("utf-8", "replace"), stderr.decode("utf-8", "replace")
        return subprocess.CompletedProcess(argv, code, stdout, stderr)


class Cleanup:
    """One finite allowance for independent cleanup attempts; never retry one.

    Keep failures even when the next attempt succeeds. The caller retains its
    status/work directory if cleanup or the command's outcome is unknown.
    """
    def __init__(self, report, deadline=None):
        self.deadline = time.monotonic() + CLEANUP_ALLOWANCE if deadline is None else deadline
        self.problems = []
        self.report = report

    def problem(self, message):
        self.problems.append(message)
        try:
            self.report(f"outside-job: cleanup: {message}\n")
        except (OSError, ValueError) as error:
            self.problems.append(f"cannot report cleanup outcome: {type(error).__name__}")

    def run(self, label, argv, max_seconds=CLEANUP_ACTION_LIMIT, **kwargs):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            self.problem(f"{label}: not attempted; cleanup allowance exhausted")
            return None
        limit = min(max_seconds, remaining)
        try:
            result = bounded_run(argv, timeout=limit, **kwargs)
        except (OSError, subprocess.SubprocessError) as error:
            self.problem(f"{label}: outcome unknown ({type(error).__name__})")
            return None
        if type(result.returncode) is not int:
            self.problem(f"{label}: exit status unknown")
            return None
        if result.returncode != 0:
            self.problem(f"{label}: exit status {result.returncode}")
            return None
        return result

    def exit_code(self, command_code):
        if type(command_code) is not int:
            self.problem("command exit status unknown")
            return 1
        return 1 if command_code == 0 and self.problems else command_code


def in_job():
    result = ctypes.c_int()
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.GetCurrentProcess.restype = ctypes.c_void_p
    kernel32.IsProcessInJob.argtypes = (ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int))
    if not kernel32.IsProcessInJob(kernel32.GetCurrentProcess(), None, ctypes.byref(result)):
        raise OSError(ctypes.get_last_error(), "IsProcessInJob failed")
    return bool(result.value)


def exited(pid):
    """Whether process `pid` has ended (or cannot be opened, so it has)."""
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.restype = ctypes.c_void_p
    kernel32.OpenProcess.argtypes = (ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32)
    kernel32.WaitForSingleObject.argtypes = (ctypes.c_void_p, ctypes.c_uint32)
    kernel32.CloseHandle.argtypes = (ctypes.c_void_p,)
    handle = kernel32.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
    if not handle:
        return True
    try:
        return kernel32.WaitForSingleObject(handle, 0) == 0
    finally:
        kernel32.CloseHandle(handle)


def join_kill_on_close_job():
    """Put this process in a new job that ends its members when the last handle closes
    and lets children break away, as Win32-OpenSSH does for a session. The handle is
    kept until this process exits."""
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetCurrentProcess.restype = ctypes.c_void_p
    job = create_job(breakaway=True)
    assign_job(job, kernel.GetCurrentProcess())
    return job


def end_executables(patterns, cleanup):
    """End every process whose executable path matches one of these -like patterns
    (left behind by a failed suite)."""
    for index, pattern in enumerate(patterns, 1):
        cleanup.run(f"executable cleanup {index}", ["powershell", "-NoProfile", "-NonInteractive", "-Command",
                        "Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like $env:OUTSIDE_JOB_EXE } "
                        "| ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue; "
                        "'outside-job: ended leftover ' + $_.ProcessId }"],
                    env=dict(os.environ, OUTSIDE_JOB_EXE=pattern), stdin=subprocess.DEVNULL)


def report_survivors(started, log, cleanup):
    """Log processes created since `started` that still run: a suite should leave none,
    and anything left keeps loading the runner for the steps that follow."""
    listing = cleanup.run("survivor report",
        ["powershell", "-NoProfile", "-NonInteractive", "-Command",
         "$since = [datetime]::FromFileTimeUtc([long]$env:OUTSIDE_JOB_SINCE).ToLocalTime(); "
         "Get-CimInstance Win32_Process | Where-Object { $_.CreationDate -gt $since -and $_.ProcessId -ne $PID } "
         "| Select-Object -First 256 | ForEach-Object { 'outside-job: still running: ' + $_.ProcessId + ' ' + $_.Name + ' ' + $_.CommandLine }"],
        env=dict(os.environ, OUTSIDE_JOB_SINCE=str(started)), stdin=subprocess.DEVNULL,
        capture_output=True, text=True, max_seconds=5)
    if listing is None:
        return
    lines = [line for line in listing.stdout.splitlines() if line.startswith("outside-job:")]
    log.write(("\n".join(lines) + "\n" if lines else "outside-job: no process of the suites is left\n").encode())


def stop_copy(pid, patterns, report, deadline=None, cleanup=None):
    """The existing timeout cleanup, bounded even when taskkill does not return."""
    cleanup = Cleanup(report, deadline) if cleanup is None else cleanup
    cleanup.run("taskkill", ["taskkill", "/T", "/F", "/PID", str(pid)],
                stdin=subprocess.DEVNULL, capture_output=True)
    end_executables(patterns, cleanup)
    return cleanup


def timeout_cleanup(pid, patterns, report, custody_only=False, deadline=None, log_path=None, started=None):
    if custody_only:
        # Detached connector custody belongs to its fixture. A wrapper timeout
        # proves neither child identity nor exit and grants no PID/tree kill.
        report("outside-job: custody-only timeout; wrapper/fixture retained, process outcome unknown\n")
        return
    cleanup = Cleanup(report, deadline)
    if log_path is not None:
        try:
            with open(log_path, "ab", buffering=0) as log:
                log.write(b"outside-job: parent timeout; process snapshot before tree cleanup\n")
                report_survivors(started, log, cleanup)
        except OSError as error:
            cleanup.problem(f"timeout diagnostics unavailable ({type(error).__name__})")
    stop_copy(pid, patterns, report, cleanup=cleanup)


def file_time_now():
    """Now as a Windows FILETIME (100 ns ticks since 1601)."""
    return int((time.time() + 11644473600) * 10_000_000)


def child(spec_path):
    spec = json.loads(Path(spec_path).read_text(encoding="utf-8"))
    Path(spec_path).unlink()
    started = None
    with open(spec["log"], "ab", buffering=0) as log:
        try:
            log.write(f"outside-job: pid {os.getpid()}, in_job {in_job()} before its own job\n".encode())
            job = join_kill_on_close_job()  # noqa: F841 - held until this process exits
            started = file_time_now()
            process = subprocess.Popen(spec["argv"], cwd=spec["cwd"], env=spec["env"], stdin=subprocess.DEVNULL,
                                       stdout=log, stderr=subprocess.STDOUT)
            try:
                code = process.wait(timeout=None if spec.get("custody_only") else
                                    max(0, spec["deadline"] - time.monotonic()))
            except subprocess.TimeoutExpired:
                # The outside copy has its OWN deadline even if the step follower
                # is blocked or cancelled. Its job ends ordinary descendants on
                # exit; the existing image cleanup covers escaped session hosts.
                log.write(b"outside-job: command deadline expired; collecting diagnostics before cleanup\n")
                code = 124
        except OSError as error:
            log.write(f"outside-job: cannot run {spec['argv']}: {error}\n".encode())
            code = 127
        cleanup = Cleanup(lambda message: log.write(message.encode()),
                          None if spec.get("custody_only") else
                          min(time.monotonic() + CLEANUP_ALLOWANCE, spec["deadline"] + CLEANUP_ALLOWANCE))
        if code != 0 and started is not None:
            report_survivors(started, log, cleanup)
        end_executables(spec["end"], cleanup)
        if started is not None:
            report_survivors(started, log, cleanup)
        code = cleanup.exit_code(code)
        log.flush()
    temporary = spec["status"] + ".tmp"
    Path(temporary).write_text(str(code), encoding="utf-8")
    os.replace(temporary, spec["status"])


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--child", help=argparse.SUPPRESS)
    parser.add_argument("--action", help=argparse.SUPPRESS)
    parser.add_argument("--log")
    parser.add_argument("--timeout", type=float, default=900)
    parser.add_argument("--end", action="append", default=[],
                        help="end processes whose executable path is -like this pattern afterwards")
    parser.add_argument("--custody-only", action="store_true",
                        help="retain unknown fixture on timeout; no PID/tree/image cleanup")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if getattr(args, "custody_only", False) and args.end:
        parser.error("--custody-only forbids --end cleanup patterns")
    if getattr(args, "action", None):
        return action(args.action)
    if args.child:
        child(args.child)
        return 0
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if os.name != "nt" or not command or not args.log:
        parser.error("Windows only; give --log and a command after --")
    deadline = time.monotonic() + args.timeout
    log = Path(args.log).resolve()
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_bytes(b"")
    work = Path(tempfile.mkdtemp(prefix="outside-job-"))
    status = work / "status"
    spec = work / "spec.json"
    spec.write_text(json.dumps({
        "argv": command, "cwd": os.getcwd(), "log": str(log), "status": str(status),
        "end": args.end, "deadline": deadline, "custody_only": getattr(args, "custody_only", False),
        "env": {key: value for key, value in os.environ.items() if not SECRET.search(key)},
    }), encoding="utf-8")
    # The base interpreter: a virtual environment's launcher would add a job of its own.
    python = getattr(sys, "_base_executable", None) or sys.executable
    line = subprocess.list2cmdline([python, "-I", str(Path(__file__).resolve()), "--child", str(spec)])
    started = file_time_now()
    created = bounded_run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command",
         "$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments "
         "@{ CommandLine = $env:OUTSIDE_JOB_LINE; CurrentDirectory = $env:OUTSIDE_JOB_CWD }; "
         "'{0} {1}' -f $r.ReturnValue, $r.ProcessId"],
        env=dict(os.environ, OUTSIDE_JOB_LINE=line, OUTSIDE_JOB_CWD=os.getcwd()),
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=min(120, args.timeout))
    fields = created.stdout.split()
    if created.returncode != 0 or len(fields) != 2 or fields[0] != "0":
        sys.stderr.write(f"outside-job: Win32_Process.Create failed: {created.stdout}{created.stderr}\n")
        return 1
    pid = int(fields[1])
    print(f"outside-job: started pid {pid} outside the step's job", flush=True)
    gone = None
    keep_work = False
    with open(log, "rb") as follow:
        while True:
            chunk = follow.read(LOG_CHUNK)
            if chunk:
                sys.stdout.write(chunk.decode("utf-8", "replace"))
                sys.stdout.flush()
            if status.exists():
                sys.stdout.write(follow.read(LOG_CHUNK).decode("utf-8", "replace"))
                try:
                    code = int(status.read_text(encoding="utf-8"))
                except (OSError, ValueError):
                    sys.stderr.write(f"outside-job: exit status unreadable; retained {work}\n")
                    code = 1
                keep_work = code != 0
                break
            # Without a status the command or cleanup outcome is unknown.
            if gone is None and exited(pid):
                gone = time.monotonic()
            if gone is not None and time.monotonic() - gone > 5:
                sys.stderr.write(f"outside-job: pid {pid} ended without a status\n")
                code = 1
                keep_work = True
                break
            if time.monotonic() > deadline + (0 if getattr(args, "custody_only", False) else 30):
                # The custody-only gate leaves identity/exit to its fixture.
                timeout_cleanup(pid, args.end, sys.stderr.write, getattr(args, "custody_only", False),
                                deadline=deadline + CLEANUP_ALLOWANCE, log_path=log, started=started)
                sys.stderr.write(f"outside-job: timed out after {args.timeout:.0f} s; outcome unknown for pid {pid}\n")
                code = 124
                keep_work = True
                break
            time.sleep(1)
    if keep_work:
        sys.stderr.write(f"outside-job: retained status/work directory {work}\n")
    else:
        try:
            for path in (spec, status):
                path.unlink(missing_ok=True)
            work.rmdir()
        except OSError as error:
            sys.stderr.write(f"outside-job: cannot remove status/work directory {work}: {type(error).__name__}\n")
            return 1
    return code


if __name__ == "__main__":
    sys.exit(main())
