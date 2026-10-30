#!/usr/bin/env python3
"""Sample one exact Qt test PID if it is still running, then wait for it."""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys


def stack_command(pid):
    if sys.platform == "darwin":
        if Path("/usr/bin/sample").is_file():
            return ["/usr/bin/sample", str(pid), "5"]
        if shutil.which("lldb"):
            return ["lldb", "--batch", "-p", str(pid), "-o", "thread backtrace all"]
    elif sys.platform.startswith("linux") and shutil.which("gdb"):
        return [shutil.which("gdb"), "-batch", "-p", str(pid), "-ex", "thread apply all bt"]
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", required=True)
    parser.add_argument("--after", type=float, default=270)
    parser.add_argument("--log")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.after < 0:
        parser.error("--after must not be negative")
    if args.command[:1] == ["--"]:
        args.command = args.command[1:]
    if not args.command:
        parser.error("a test command is required")
    child = subprocess.Popen(args.command)
    try:
        child.wait(timeout=args.after)
        return child.returncode
    except subprocess.TimeoutExpired:
        pass
    command = stack_command(child.pid)
    if command is None:
        payload = f"Qt hang sampler: {args.name} is still running as PID {child.pid} after {args.after:g} seconds; no supported stack tool is available."
    else:
        print(f"Qt hang sampler: {args.name} is still running as PID {child.pid} after {args.after:g} seconds; capturing all-thread stacks.", flush=True)
        try:
            result = subprocess.run(command, capture_output=True, text=True, errors="replace")
            payload = f"$ {' '.join(command)}\n{result.stdout}{result.stderr}"
        except Exception as error:
            payload = f"Qt hang sampler could not capture PID {child.pid}: {error}"
    if args.log:
        log = Path(args.log)
        log.parent.mkdir(parents=True, exist_ok=True)
        log.write_text(payload + "\n", encoding="utf-8")
    print(payload, flush=True)
    return child.wait()


if __name__ == "__main__":
    sys.exit(main())
