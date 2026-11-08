#!/usr/bin/env python3
"""Task 157: the fork release candidate, run on the Windows worker.

It runs the same steps as .github/workflows/fork-windows.yml after the Rust
suite, records their result, and with --exp-tag also pushes a whitewh-exp-*
tag for the exact commit and waits for that workflow. Both legs must be green
before scripts/publish-whitewh-main.py may move whitewh/main.

    python scripts/release-candidate.py --commit <sha> [--exp-tag <name>]
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import shutil
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / ".zerus-release"
# Task 213: the fork identity the workflow takes from its tag and run number; a
# local leg has no tag, so it takes the same two values explicitly.
FORK_RELEASES = "https://github.com/WhiteWh/zerus/releases/"


def run(command: list[str], env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    merged = dict(os.environ)
    merged.update(env or {})
    completed = subprocess.run(command, cwd=ROOT, env=merged)
    if completed.returncode != 0:
        raise SystemExit(f"release candidate step failed: {' '.join(command)}")


def fork_identity(build: str, feed: str) -> tuple[str, str]:
    """Task 213: both a build number and a feed, or neither (experimental)."""
    build = build.strip()
    feed = feed.strip()
    if bool(build) != bool(feed):
        raise SystemExit("A fork build has both a build number and a feed, or neither (experimental).")
    if build and (not build.isdigit() or int(build) <= 0):
        raise SystemExit(f"The fork build is a positive number or empty (got {build!r}).")
    if feed and not feed.startswith(FORK_RELEASES):
        raise SystemExit(f"The fork feed is a release asset of WhiteWh/zerus (got {feed!r}).")
    return build, feed


def record(commit: str, leg: str, status: str, detail: str = "", extra: dict | None = None) -> None:
    EVIDENCE.mkdir(exist_ok=True)
    path = EVIDENCE / f"{commit}.json"
    data = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {"commit": commit}
    entry = {"status": status, "detail": detail, "at": int(time.time())}
    if extra:
        entry.update(extra)
    data[leg] = entry
    path.write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")



def bash() -> str:
    for candidate in (shutil.which("bash"), r"C:/Program Files/Git/bin/bash.exe", r"C:/Program Files/Git/usr/bin/bash.exe"):
        if candidate and Path(candidate).exists():
            return str(candidate)
    raise SystemExit("bash not found: install Git for Windows or put bash on PATH")

QT_RETRY = "2"


def qt_windows(qt_root: str) -> None:
    """The Qt suites on Windows, as scripts/ci/gui.sh runs them there.

    Git Bash/MSYS cannot start under the release sandbox
    (NtCreateDirectoryObject access denied), so this is gui.sh in Python:
    Ninja with MSVC from scripts/ci/msvc-env.py (the generator Zerus's
    qt-configure also uses for .ci-build/gui, and where release-candidate-local
    finds .ci-build/gui/hgs-tray.exe), per-function results, QtTest output kept
    in the CTest log, a 240-second timeout and the retry policy.
    Task 192: the earlier Visual Studio generator path differed in all of these:
    a failed suite left no QtTest output and was never retried, and its
    hgs-tray.exe landed in Release/.
    """
    build = ROOT / ".ci-build/gui"
    cache = build / "CMakeCache.txt"
    msvc = [sys.executable, "scripts/ci/msvc-env.py"]
    results = ROOT / "artifacts/test-results"
    functions = results / "qt-functions"
    configure = ["cmake", "-S", "tray", "-B", str(build), "-G", "Ninja", "-DCMAKE_C_COMPILER=cl", "-DCMAKE_CXX_COMPILER=cl",
                 "-DCMAKE_BUILD_TYPE=Release", "-DBUILD_TESTING=ON", f"-DZERUS_QT_RESULTS_DIR={functions.as_posix()}",
                 f"-DCMAKE_PREFIX_PATH={Path(qt_root).as_posix()}"]
    # A build folder made by another generator (the earlier Visual Studio
    # path) is generated output: --fresh configures it again from scratch.
    if cache.exists() and "CMAKE_GENERATOR:INTERNAL=Ninja\n" not in cache.read_text(encoding="utf-8", errors="replace"):
        configure.insert(1, "--fresh")
    run(msvc + configure)
    run(msvc + ["cmake", "--build", str(build), "--parallel", os.environ.get("ZERUS_BUILD_JOBS", "2")])
    shutil.rmtree(functions, ignore_errors=True)
    functions.mkdir(parents=True, exist_ok=True)
    retry = os.environ.get("ZERUS_QT_RETRY", QT_RETRY)
    # Task 213: four Qt suites (sessionswindow_opencode, teamsui_workspace,
    # sessionswindow_list3, machinespage) fail under four workers and pass alone;
    # two workers keep their timing stable.
    run(["ctest", "--test-dir", str(build), "--parallel", os.environ.get("ZERUS_TEST_JOBS", "2"), "--timeout", "240",
         "--output-on-failure", *(["--repeat", f"until-pass:{retry}"] if retry else []),
         "--output-junit", str(results / "qt.xml")],
        {"ZERUS_TEST_HGS": str(ROOT / "target/release/hgs.exe"),
         "QT_QPA_PLATFORM": "offscreen", "QT_QUICK_BACKEND": "software",
         "ZERUS_GUI_THREAD_CHECKS": "fatal", "ZERUS_SETTINGS_SYNC": "1",
         "QTWEBENGINE_CHROMIUM_FLAGS": "--disable-gpu",
         # Without a console window QtTest writes to OutputDebugString.
         "QT_FORCE_STDERR_LOGGING": "1",
         "PATH": str(Path(qt_root) / "bin") + os.pathsep + os.environ.get("PATH", "")})


def local_steps(commit: str, fork_build: str = "", fork_feed: str = "", qt_only: bool = False) -> None:
    run([sys.executable, "scripts/check-machine-dependent-tests.py"])
    if not qt_only:
        run(["cargo", "build", "--locked", "--release", "--bin", "hgs"])
    # The same order as fork-windows.yml after "Rust tests and release build".
    qt_root = os.environ.get("QT_ROOT_DIR", r"D:/Tools/zerus-deps/Qt/6.10.3/msvc2022_64")
    if os.name == "nt":
        qt_windows(qt_root)
    else:
        run([bash(), "scripts/ci/gui.sh", "all"], {"ZERUS_QT_RETRY": QT_RETRY, "ZERUS_TEST_HGS": str(ROOT / "target/release/hgs.exe"), "QT_ROOT_DIR": qt_root})
    if qt_only:
        return
    run(["cargo", "test", "--locked", "--manifest-path", "client/core/Cargo.toml", "--lib", "fork_update"])
    run(["cargo", "test", "--locked", "--manifest-path", "client/core/Cargo.toml", "--test", "fork_handoff"])
    # Tauri build and the portable zip are run through their scripts so this
    # file has no second copy of the flags. Task 213: the fork identity goes to
    # both, so hgs-tray embeds what build-info.json names and the portable
    # self-test compares them.
    run([sys.executable, "scripts/release-candidate-local.py", "--commit", commit,
         "--fork-build", fork_build, "--fork-feed", fork_feed])


def push_exp_tag(commit: str, tag: str) -> None:
    run(["git", "tag", "-f", tag, commit])
    run(["git", "push", "origin", f"refs/tags/{tag}"])


def credential_token() -> str:
    """GitHub's token from `git credential fill`; never printed."""
    filled = subprocess.run(
        ["git", "credential", "fill"], cwd=ROOT, input="protocol=https\nhost=github.com\n",
        capture_output=True, text=True)
    if filled.returncode != 0:
        raise SystemExit("no GitHub credential: set GH_TOKEN or run `git credential fill` for github.com")
    for line in filled.stdout.splitlines():
        if line.startswith("password="):
            return line.split("=", 1)[1]
    raise SystemExit("no GitHub credential: git credential fill returned no password")


def api(url: str, token: str) -> dict:
    request = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "zerus-release-candidate",
    })
    try:
        with urllib.request.urlopen(request, timeout=30) as answer:
            return json.loads(answer.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        raise SystemExit(f"GitHub API {error.code} for {url}") from None
    except urllib.error.URLError as error:
        raise SystemExit(f"GitHub API unreachable: {error.reason}") from None


def failed_steps(run_id: int, token: str) -> str:
    jobs = api(f"https://api.github.com/repos/WhiteWh/zerus/actions/runs/{run_id}/jobs?per_page=100", token)
    failed = []
    for job in jobs.get("jobs", []):
        for step in job.get("steps", []):
            if step.get("conclusion") in {"failure", "cancelled", "timed_out"}:
                failed.append(f"{job.get('name')}: {step.get('name')} ({step.get('conclusion')})")
    return "; ".join(failed)


def wait_exp_rest(commit: str, timeout_minutes: int = 150) -> None:
    token = os.environ.get("GH_TOKEN") or credential_token()
    deadline = time.time() + timeout_minutes * 60
    print(f"waiting for the fork-windows workflow on {commit} through the GitHub API...", flush=True)
    while time.time() < deadline:
        runs = api(f"https://api.github.com/repos/WhiteWh/zerus/actions/runs?head_sha={commit}&per_page=50", token)
        for run in runs.get("workflow_runs", []):
            if run.get("head_sha") != commit or not str(run.get("path", "")).endswith("fork-windows.yml"):
                continue
            if run.get("status") != "completed":
                continue
            if run.get("conclusion") != "success":
                detail = failed_steps(run["id"], token)
                raise SystemExit(f"experimental run failed: {run.get('html_url')} {detail}")
            return
        time.sleep(30)
    raise SystemExit("timed out waiting for the experimental workflow")


def wait_exp(tag: str, commit: str, timeout_minutes: int = 150) -> None:
    if shutil.which("gh"):
        deadline = time.time() + timeout_minutes * 60
        print(f"waiting for the {tag} workflow on {commit}...", flush=True)
        while time.time() < deadline:
            output = subprocess.run(
                ["gh", "run", "list", "--workflow", "fork-windows.yml", "--commit", commit,
                 "--json", "databaseId,headSha,status,conclusion,url", "--limit", "20"],
                cwd=ROOT, capture_output=True, text=True)
            if output.returncode == 0:
                for run in json.loads(output.stdout or "[]"):
                    if run.get("headSha") == commit and run.get("status") == "completed":
                        if run.get("conclusion") != "success":
                            raise SystemExit(f"experimental run failed: {run.get('url')}")
                        return
            time.sleep(30)
        raise SystemExit("timed out waiting for the experimental workflow")
    wait_exp_rest(commit, timeout_minutes)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", default="HEAD")
    parser.add_argument("--exp-tag", default="")
    # Task 213: the local leg takes the fork identity the workflow gets from its
    # tag and run number; both values together, or neither (experimental).
    parser.add_argument("--fork-build", default=os.environ.get("ZERUS_FORK_BUILD", ""),
                        help="the fork build number the desktop embeds and build-info.json names "
                             "[default: $ZERUS_FORK_BUILD; empty for an experimental build]")
    parser.add_argument("--fork-feed", default=os.environ.get("ZERUS_FORK_FEED") or os.environ.get("ZERUS_FORK_FEED_URL", ""),
                        help="the fork update feed [default: $ZERUS_FORK_FEED, else $ZERUS_FORK_FEED_URL; empty for an experimental build]")
    # Task 192: only the machine-dependent scan and the Qt suites, to look at a
    # Qt failure; records nothing.
    parser.add_argument("--qt-only", action="store_true")
    args = parser.parse_args()
    commit = subprocess.run(["git", "rev-parse", args.commit], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
    fork_build, fork_feed = fork_identity(args.fork_build, args.fork_feed)
    print(f"fork identity: build {fork_build or 'none (experimental)'}, feed {fork_feed or 'none (experimental)'}", flush=True)
    extra = {"fork": {"build": int(fork_build) if fork_build else None, "feed": fork_feed}}
    if args.qt_only:
        local_steps(commit, fork_build, fork_feed, qt_only=True)
        return 0
    try:
        local_steps(commit, fork_build, fork_feed)
    except BaseException as error:
        record(commit, "local", "failed", str(error)[:1000], extra)
        raise
    record(commit, "local", "green", "", extra)
    if args.exp_tag:
        try:
            push_exp_tag(commit, args.exp_tag)
            wait_exp(args.exp_tag, commit)
        except BaseException as error:
            record(commit, "experimental", "failed", str(error)[:1000])
            raise
        record(commit, "experimental", "green", args.exp_tag)
    else:
        print("no --exp-tag: the runner leg is not green yet; publishing stays refused")
    print(f"release candidate: {commit}")
    return 0


if __name__ == "__main__":
    sys.exit(main())





