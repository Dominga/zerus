#!/usr/bin/env python3
"""Run exact-commit Linux checks locally, without a host session or CI credentials.

This is an unprivileged Arch preflight, not a GitHub runner emulator.
The selected commit supplies checks.yml and all test bodies. Unsupported checks
remain NOT RUN. Independent steps continue after failure, including CLI smoke
and Python integration, which CI's cli.sh normally short-circuits.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time

from local_network import Network
from local_services import Postgres
from local_container import Container

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ".github/workflows/checks.yml"
RUST = "1.99.1"
SUPPORTED = {"quick", "client-frontend", "cli", "client-native-linux", "connector", "relay", "arch-package", "android"}
RUST_CACHE = ('echo "rustc=$(rustc{toolchain} -Vv | git hash-object --stdin | cut -c1-16)" >> "$GITHUB_OUTPUT"\n'
              'echo "epoch=$(date -u +%Y-%m)" >> "$GITHUB_OUTPUT"')
APT_PREREQUISITES = {
    'sudo apt-get update && sudo apt-get install -y libevent-dev libncurses-dev bison pkg-config fish',
    'sudo apt-get update && sudo apt-get install --no-install-recommends -y libasound2-dev libgstreamer1.0-dev '
    'libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good '
    'gstreamer1.0-plugins-bad gstreamer1.0-libav libgtk-3-dev libwebkit2gtk-4.1-dev '
    'libayatana-appindicator3-dev librsvg2-dev xvfb dbus-bin dbus-daemon',
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


def resolve(repo, value):
    if not re.fullmatch(r"[0-9a-f]{7,40}", value):
        raise ValueError("--sha must be a commit hash, not a branch or revision expression")
    return git(repo, "rev-parse", "--verify", value + "^{commit}")


def load_workflow(data):
    try:
        import yaml
    except ImportError as error:
        raise ValueError("local runner requires python-yaml (PyYAML); no checks started") from error
    return yaml.safe_load(data)


def cli_units(body):
    """Keep exact source slices; refuse an unfamiliar script rather than guess.

    The debug half performs the real build/tests. Separating smoke, Python and
    release build stops the first failing suite concealing independent failures.
    Python retains its original fixture-build preparation and exports.
    """
    marker = 'if [ "$phase" != debug ]; then\n'
    if body.count(marker) != 1 or not body.rstrip().endswith("fi"):
        raise ValueError("unrecognized cli.sh phase structure")
    remaining = body.split(marker)[1].rstrip()[:-2]
    lines = remaining.splitlines(keepends=True)
    smoke = [i for i, line in enumerate(lines) if line.startswith("bash tests/test_hgs.sh ")]
    python = [i for i, line in enumerate(lines) if line.startswith("python3 scripts/ci/python-tests.py ")]
    if len(smoke) != 1 or len(python) != 1 or smoke[0] >= python[0]:
        raise ValueError("unrecognized cli.sh suite boundaries")
    a, b = smoke[0], python[0]
    prefix = "".join(lines[:a])
    return [
        ("CLI build and Rust tests", "bash scripts/ci/cli.sh debug\n"),
        ("CLI shell smoke", prefix + lines[a]),
        ("CLI Python integration", prefix + "".join(lines[a + 1:b + 1])),
        ("CLI release build", prefix + "".join(lines[b + 1:])),
    ]


def infrastructure(step, online=False):
    """Return a local substitution or an unsupported prerequisite, explicitly."""
    action = step.get("uses", "")
    if action and "run" in step:
        return "not run", "mixed action and command body; no local substitution"
    if action.startswith("actions/setup-node@"):
        return None if online else ("not run", "online mode supplies the requested official Node major")
    if action.startswith(("actions/checkout@", "actions/cache/",
                          "actions/upload-artifact@")):
        return "local", "local checkout/tools/cache/logs replace GitHub infrastructure"
    if action:
        return "not run", "unsupported GitHub action: " + action
    body = step.get("run", "")
    exact = body.rstrip("\n")
    disk = re.fullmatch(r"python3 scripts/ci/ubuntu-disk\.py (?:prepare|relay-before-image|report [a-z0-9-]+)", exact)
    cache = exact in {RUST_CACHE.format(toolchain=toolchain) for toolchain in ("", " +1.99.1")}
    cache |= exact == 'echo "epoch=$(date -u +%Y-%m)" >> "$GITHUB_OUTPUT"'
    if disk or cache:
        return "local", "hosted disk/cache maintenance excluded; local disk floor enforced"
    toolchain = re.fullmatch(r"rustup toolchain install 1\.99\.1 --profile minimal"
                            r"(?: --component rustfmt)?(?: --component clippy)?", exact)
    if toolchain or exact == "rustup toolchain install stable --profile minimal && rustup default stable":
        return "local", "preinstalled pinned toolchain; no host toolchain mutation"
    if body.startswith("bash scripts/ci/install-tmux.sh"):
        return None if online else ("not run", "pinned CI tmux build requires online provisioning")
    if exact in APT_PREREQUISITES:
        return "local", "Arch tools supplied locally; no host package installation"
    if ("sudo " in body or "docker " in body or "${{" in body):
        return "not run", "requires service, container, or unsupported workflow expression"
    if not online and ("scripts/ci/dsh_fixture.py" in body or "scripts/ci/agent-compatibility.py" in body
                       or "curl " in body or "pip" in body or "rollback-connectors.py" in body):
        return "not run", "requires online provisioning, service, or unsupported workflow expression"
    return None


def summarize(steps):
    checks = [s for s in steps if s["status"] != "local"]
    if any(s["status"] == "fail" for s in checks):
        return "fail"
    if not checks or any(s["status"] != "pass" for s in checks):
        return "not run"
    return "pass"


def plan(workflow, cli, selected=None, online=False):
    jobs = []
    for key, job in workflow["jobs"].items():
        item = {"id": key, "name": job.get("name", key), "status": "not run", "steps": []}
        jobs.append(item)
        if key not in SUPPORTED or (selected is not None and key not in selected):
            item["reason"] = ("not selected" if selected is not None and key not in selected else
                              "phase 1 does not supply this platform/container/service/aggregate")
            continue
        services = job.get("services", {})
        if services and (not online or set(services) != {"postgres"}):
            item["reason"] = "private PostgreSQL needs online mode; other services unsupported"
            continue
        if job.get("container") and not (key == "arch-package" and online):
            item["reason"] = "Arch container requires online mode"
            continue
        if job.get("runs-on") != "ubuntu-24.04":
            item["reason"] = "unsupported job execution environment"
            continue
        item["services"] = services
        if key == "android":
            if not online:
                item["reason"] = "Android tool provisioning requires online mode"
                continue
            item["steps"].append(dict(name="Prepare private JDK 17 and Android tools", index=-1,
                                     status="not run", body="python3 /mnt/runner/local_android.py /mnt/android-cache\n",
                                     body_sha256=digest(b"python3 /mnt/runner/local_android.py /mnt/android-cache\n"),
                                     directory=".", env={}, timeout=600))
        if job.get("container"):
            item["container_image"] = job["container"]["image"]
        for index, step in enumerate(job.get("steps", [])):
            supplied_dbus = "sudo apt-get update && sudo apt-get install --no-install-recommends -y dbus-daemon\n"
            if key == "quick" and step.get("run", "").startswith(supplied_dbus) and shutil.which("dbus-daemon"):
                step = dict(step, run=step["run"][len(supplied_dbus):],
                            local_provisioning="preinstalled dbus-daemon replaces only the apt installation line")
            if step.get("uses", "").startswith("actions/setup-node@") and online:
                major = str(step.get("with", {}).get("node-version", ""))
                if re.fullmatch(r"[1-9][0-9]", major) and "run" not in step:
                    step = dict(step, name=f"Supply official Node {major}", run=(
                        f'python3 /mnt/runner/local_node.py {major} "$RUNNER_TEMP/node"\n'
                        'echo "$RUNNER_TEMP/node/bin" >> "$GITHUB_PATH"\n'
                        '"$RUNNER_TEMP/node/bin/node" --version\n'))
                    step.pop("uses")
            name = step.get("name", step.get("run", step.get("uses", "step")))
            row = {"name": name.splitlines()[0], "index": index, "status": "not run"}
            item["steps"].append(row)
            decision = infrastructure(step, online)
            if key == "relay" and ("docker build" in step.get("run", "") or "relay/tests/load.py" in step.get("run", "")):
                decision = ("not run", "owner decision: dedicated-daemon/cgroup memory qualification is GitHub-only")
            if key == "arch-package" and "run" in step:
                decision = None
            condition = step.get("if", "")
            if condition and condition not in ("runner.os == 'Linux'", "${{ !cancelled() }}", "always()", "failure()") and not decision:
                decision = ("not run", "unsupported step condition: " + condition)
            if decision:
                row["status"], row["reason"] = decision
                continue
            body = step.get("run")
            if not isinstance(body, str) or step.get("shell", "bash") != "bash":
                row["reason"] = "unsupported step body/shell"
                continue
            units = [(row["name"], body)]
            if key == "cli" and body.strip() == "bash scripts/ci/cli.sh":
                try:
                    units = cli_units(cli)
                    build = [line for line in cli.splitlines() if line == "cargo build --locked"]
                    if len(build) != 1:
                        raise ValueError("unrecognized cli.sh exact-binary build")
                    units.insert(0, ("Rebuild exact-SHA CLI", build[0] + "\n"))
                    fixture = [line for line in cli.splitlines() if line == "cargo build --locked --example fake_agent"]
                    if len(fixture) != 1:
                        raise ValueError("unrecognized cli.sh exact fixture build")
                    units.insert(1, ("Build exact-SHA fake agent", fixture[0] + "\n"))
                except ValueError as error:
                    row["reason"] = str(error)
                    continue
                item["steps"].pop()
            for part, (title, script) in enumerate(units):
                target = row if len(units) == 1 else dict(row, name=title, part=part)
                if step.get("local_provisioning"):
                    target["local_provisioning"] = step["local_provisioning"]
                target.update(body=script, body_sha256=digest(script.encode()),
                              condition=condition,
                              directory=step.get("working-directory", "."),
                              env={**job.get("env", {}), **step.get("env", {})},
                              timeout=min(3600, int(job.get("timeout-minutes", 30)) * 60))
                if len(units) != 1:
                    item["steps"].append(target)
    return jobs


def save(path, report):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(report, indent=2) + "\n")
    temporary.replace(path)


class Runner:
    def __init__(self, args, report):
        self.args, self.report = args, report
        self.created_output = False
        self.output = args.output.resolve()
        self.workspace = self.output / "checkout"
        self.pristine = self.output / "snapshot"
        self.cache = args.cache.resolve()
        self.target = args.target.resolve() if args.target else self.cache / "target"
        self.home = self.output / "home"
        self.tools = self.output / "tools"
        self.rustup = Path(os.environ.get("RUSTUP_HOME", Path.home() / ".rustup")).resolve()
        self.cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")).resolve()
        self.github_env = {}
        self.github_path = []
        self.postgres = None
        self.container = None
        self.objects = []
        self.android_enabled = any(job["id"] == "android" and job["steps"] for job in report["jobs"])
        self.android_cache = (args.android_cache.resolve() if args.android_cache else self.cache / "android")
        self.rustup_source = self.rustup

    def prepare(self):
        import fcntl
        if self.output.exists():
            raise ValueError("--output must be a new directory; retain earlier evidence")
        self.output.mkdir(parents=True, mode=0o700)
        self.created_output = True
        self.output.chmod(0o700)
        harness = self.output / "harness"
        harness.mkdir()
        for path in Path(__file__).parent.glob("local*.py"):
            shutil.copy2(path, harness / path.name)
        self.cache.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.lock = (self.cache / "lock").open("w")
        fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.target.mkdir(parents=True, exist_ok=True)
        self.target_lock = (self.target / ".local-ci.lock").open("w")
        fcntl.flock(self.target_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.home.mkdir(mode=0o700)
        self.tools.mkdir()
        if self.android_enabled:
            self.android_cache.mkdir(parents=True, exist_ok=True, mode=0o700)
            staged = self.cache / "rustup"
            toolchain = f"{RUST}-x86_64-unknown-linux-gnu"
            destination = staged / "toolchains" / toolchain
            if not destination.exists():
                destination.parent.mkdir(parents=True, exist_ok=True)
                subprocess.run(["cp", "-a", "--reflink=auto", str(self.rustup / "toolchains" / toolchain),
                                str(destination)], check=True)
            (staged / "settings.toml").write_text(f'version = "12"\ndefault_toolchain = "{toolchain}"\nprofile = "minimal"\nauto_self_update = "disable"\n')
            self.rustup_source = staged
        if self.args.online:
            if not self.args.slirp or not self.args.slirp.is_file():
                raise ValueError("--online requires --slirp or an installed slirp4netns")
            (self.output / "resolv.conf").write_text("nameserver 10.0.2.3\n")
        cargo = self.cache / "cargo"
        cargo.mkdir(exist_ok=True)
        # Copy only public package cache, never cargo config/credentials or git auth.
        source = self.cargo_home / "registry"
        if source.exists() and not (cargo / "registry").exists():
            subprocess.run(["cp", "-a", "--reflink=auto", str(source), str(cargo / "registry")], check=True)
        rustup = shutil.which("rustup")
        if not rustup:
            raise ValueError("rustup with the pinned toolchain is required")
        shutil.copy2(Path(rustup).resolve(), self.tools / "rustup")
        for tool in ("cargo", "rustc", "rustdoc", "rustfmt", "cargo-clippy", "clippy-driver"):
            (self.tools / tool).symlink_to("rustup")
        env = {"PATH": "/usr/bin:/bin", "HOME": str(self.home), "GIT_CONFIG_NOSYSTEM": "1"}
        subprocess.run(["git", "-c", "core.hooksPath=/dev/null", "clone", "--shared", "--no-checkout",
                        str(self.args.repo.resolve()), str(self.workspace)], env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(["git", "-C", str(self.workspace), "-c", "core.hooksPath=/dev/null",
                        "checkout", "--detach", self.report["commit"]], env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        manifests = list(self.workspace.rglob("Cargo.toml"))
        for manifest in manifests:
            target = manifest.parent / "target"
            if not target.exists() and not target.is_symlink():
                target.symlink_to("/mnt/build")
        self.workspace.rename(self.pristine)
        self.workspace = self.pristine
        self.objects = [Path(line) for line in
                        (self.pristine / ".git/objects/info/alternates").read_text().splitlines()]
        self.report["environment"] = {}
        fixture_path = os.pathsep.join([str(path.resolve()) for path in self.args.tool_bin] + [os.environ.get("PATH", "")])
        if not shutil.which("fish", path=fixture_path):
            for job in self.report["jobs"]:
                if job["id"] == "cli" and job["steps"]:
                    job["prerequisite_error"] = "Fish is missing; provide its private bin directory with --tool-bin"
        for tool, command in {"tmux": ["tmux", "-V"], "node": ["node", "--version"],
                              "rust": [rustup, "run", RUST, "rustc", "-Vv"]}.items():
            result = subprocess.run(command, capture_output=True, text=True, timeout=15)
            self.report["environment"][tool] = {"exit_code": result.returncode, "version": result.stdout.strip()}

    def new_checkout(self, job):
        destination = self.output / "checkouts" / job["id"]
        destination.mkdir(parents=True, exist_ok=False)
        # Independent files, preserving source mtimes for the shared build cache.
        # No hard links: a mutating test cannot change another job's source.
        subprocess.run(["cp", "-a", "--reflink=auto", str(self.pristine) + "/.", str(destination)], check=True)
        self.workspace = destination
        job["checkout"] = str(destination.relative_to(self.output))

    def command(self, script, cwd, job_home, environment):
        command = ["bwrap", "--unshare-user", "--unshare-pid", "--unshare-net", "--unshare-ipc",
                   "--unshare-uts", "--hostname", "local-ci", "--die-with-parent",
                   "--cap-drop", "ALL",
                   "--ro-bind", "/usr", "/usr", "--ro-bind", "/etc", "/etc",
                   "--ro-bind", "/sys", "/sys", "--proc", "/proc", "--dev", "/dev",
                   "--tmpfs", "/var", "--dir", "/var/tmp",
                   "--tmpfs", "/mnt", "--tmpfs", "/home", "--tmpfs", "/root", "--tmpfs", "/tmp", "--tmpfs", "/run",
                   "--bind", str(job_home), "/home/test", "--bind", str(self.workspace), "/mnt/workspace",
                   "--bind", str(self.target), "/mnt/build", "--bind", str(self.cache / "cargo"), str(self.cargo_home),
                   "--bind" if self.android_enabled else "--ro-bind", str(self.rustup_source), str(self.rustup),
                   "--ro-bind", str(self.tools), "/mnt/tools",
                   "--ro-bind", str(self.output / "harness"), "/mnt/runner",
                   "--ro-bind", str(script), "/mnt/step.sh"]
        for name in ("/bin", "/sbin", "/lib", "/lib64"):
            path = Path(name)
            if path.is_symlink():
                command += ["--symlink", os.readlink(path), name]
            elif path.exists():
                command += ["--ro-bind", name, name]
        for objects in self.objects:
            command += ["--ro-bind", str(objects), str(objects)]
        if self.android_enabled:
            command += ["--bind", str(self.android_cache), "/mnt/android-cache"]
        for index, directory in enumerate(self.args.tool_bin):
            command += ["--ro-bind", str(directory.resolve()), f"/mnt/extra/{index}"]
        if self.args.online:
            command += ["--ro-bind", str(self.output / "resolv.conf"), "/etc/resolv.conf"]
        command += ["--chdir", str(Path("/mnt/workspace") / cwd), "--", "/usr/bin/env", "-i"]
        command += [f"{k}={v}" for k, v in environment.items()]
        command += ["/usr/bin/setpriv", "--no-new-privs", "/bin/bash", "-c",
                    'mkdir -p "$TMUX_TMPDIR" "$XDG_RUNTIME_DIR" "$RUNNER_TEMP"; '
                    'chmod 700 "$TMUX_TMPDIR" "$XDG_RUNTIME_DIR"; exec dbus-run-session -- "$@"',
                    "local-ci", "/bin/bash", "-e", "-o", "pipefail", "/mnt/step.sh"]
        cpus = sorted(os.sched_getaffinity(0))[:self.args.workers]
        return ["nice", "-n", "10", "taskset", "--cpu-list", ",".join(map(str, cpus)), *command]

    def spawn(self, command, output, step, home):
        for attempt in range(3):
            child = None
            network = Network(self.args.slirp, output) if self.args.online and not self.container else None
            try:
                passed = network.before_spawn(command) if network else ()
                child = subprocess.Popen(command, stdout=output, pass_fds=passed, stdin=subprocess.DEVNULL,
                                         stderr=subprocess.STDOUT, start_new_session=True,
                                         env={"PATH": "/usr/bin:/bin", "HOME": str(home)})
                if network:
                    network.connect()
                step["execution_started"] = True
                return child, network
            except BaseException as error:
                entered = bool(network and network.release_entered)
                step["execution_started"] = entered
                step.setdefault("preparation_refusals", []).append(dict(attempt=attempt + 1,
                    before_release=not entered, reason=str(error)))
                if child:
                    if child.poll() is None:
                        os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=10)
                if network:
                    network.finish()
                if isinstance(error, RuntimeError) and network and not entered and attempt < 2:
                    # The original setup namespace and helper have physically
                    # settled, and the command gate was never entered.
                    command = [part for part in command]
                    start = command.index("--info-fd")
                    del command[start:start + 4]
                    continue
                raise

    def run(self, job, step):
        deadline = time.monotonic() + 600
        while os.getloadavg()[0] > self.args.max_load:
            if time.monotonic() >= deadline:
                step.update(status="not run", reason="load stayed above configured ceiling for 10 minutes")
                return
            time.sleep(10)
        if shutil.disk_usage(self.output).free < self.args.min_free_gib * 1024**3:
            step.update(status="not run", reason="disk floor reached; no new work started")
            return
        directory = Path(step["directory"])
        if directory.is_absolute() or ".." in directory.parts:
            step.update(status="not run", reason="working directory escapes checkout")
            return
        variables = dict(step["env"])
        for key, value in variables.items():
            if isinstance(value, str):
                variables[key] = value.replace("${{ runner.temp }}", "/home/test/runner-temp").replace(
                    "${{ github.workspace }}", "/mnt/workspace")
        if self.container:
            replacements = {
                "${{ inputs.save_caches }}": "false",
                "${{ inputs.arch_check_in_source && needs.source.outputs.commit || '' }}": "",
                "${{ inputs.arch_check_in_source && github.sha || '' }}": "",
                "${{ inputs.save_caches && steps.build.outcome == 'success' }}": "false",
                "${{ github.repository == 'ufna/zerus' && github.ref == 'refs/heads/main' && github.event_name == 'push' && vars.DEV_UPDATES_ENABLED == 'true' && github.run_number || '' }}": "",
            }
            for key, value in variables.items():
                if str(value) in replacements:
                    variables[key] = replacements[value]
                if value in ("${{ steps.ccache.outputs.started }}", "${{ steps.ccache-start.outputs.started }}"):
                    lines = (self.container.metadata / "output").read_text().splitlines()
                    variables[key] = next(line.split("=", 1)[1] for line in reversed(lines) if line.startswith("started="))
        if self.postgres and variables.get("ZERUS_RELAY_TEST_DATABASE_URL") == (
                "postgresql://postgres@127.0.0.1:${{ job.services.postgres.ports['5432'] }}/postgres"):
            variables["ZERUS_RELAY_TEST_DATABASE_URL"] = self.postgres.url
        if any("${{" in str(v) or k.startswith("HGS_") or k == "ZERUS_CLIENT_DESKTOP_SERVICE"
               for k, v in variables.items()):
            step.update(status="not run", reason="unsupported or authority-bearing workflow environment")
            return
        name = f"{job['id']}-{step['index']:02d}-{step.get('part', 0)}"
        script, log = self.output / (name + ".sh"), self.output / (name + ".log")
        script.write_text("set -euo pipefail\nmkdir -p artifacts/test-results\n" + step["body"])
        home = self.home / job["id"]
        home.mkdir(exist_ok=True, mode=0o700)
        env = {"HOME": "/home/test", "USER": "test", "LOGNAME": "test", "LANG": "C.UTF-8",
               "SHELL": "/bin/bash", "TMPDIR": "/tmp", "TMUX_TMPDIR": "/tmp/t",
               "XDG_RUNTIME_DIR": "/tmp/r", "XDG_CONFIG_HOME": "/home/test/.config",
               "XDG_STATE_HOME": "/home/test/.local/state", "XDG_DATA_HOME": "/home/test/.local/share",
               "XDG_CACHE_HOME": "/home/test/.cache", "GITHUB_WORKSPACE": "/mnt/workspace",
               "RUNNER_TEMP": "/home/test/runner-temp", "GITHUB_OUTPUT": "/home/test/github-output",
               "GITHUB_ENV": "/home/test/github-env", "GITHUB_PATH": "/home/test/github-path",
               "CARGO_HOME": str(self.cargo_home), "RUSTUP_HOME": str(self.rustup), "RUSTUP_TOOLCHAIN": RUST,
               "CARGO_TARGET_DIR": "/mnt/build", "CARGO_NET_OFFLINE": "false" if self.args.online else "true",
               "CARGO_BUILD_JOBS": str(self.args.workers),
               "CARGO_INCREMENTAL": "0", "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0",
               "GDK_BACKEND": "x11", "LIBGL_ALWAYS_SOFTWARE": "1",
               "__EGL_VENDOR_LIBRARY_FILENAMES": "/usr/share/glvnd/egl_vendor.d/50_mesa.json",
               "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
               "NPM_CONFIG_OFFLINE": "false" if self.args.online else "true",
               "PIP_NO_INDEX": "0" if self.args.online else "1", "RUST_TEST_THREADS": str(self.args.workers),
               "PATH": ":".join(self.github_path + [f"/mnt/extra/{i}" for i in range(len(self.args.tool_bin))]
                                + ["/mnt/tools", str(self.cargo_home / "bin"), "/usr/bin", "/bin"]),
               **self.github_env,
               **{k: str(v) for k, v in variables.items()}}
        # Keep machine load capped even when a workflow requests hosted parallelism.
        env.update(ZERUS_PYTHON_JOBS=str(self.args.workers), ZERUS_TEST_JOBS=str(self.args.workers))
        if job["id"] == "android":
            env.update(JAVA_HOME="/mnt/android-cache/java17", JAVA_HOME_17_X64="/mnt/android-cache/java17",
                       ANDROID_HOME="/mnt/android-cache/android-sdk", ANDROID_SDK_ROOT="/mnt/android-cache/android-sdk",
                       GRADLE_USER_HOME="/home/test/.gradle",
                       GRADLE_OPTS=f"-Dorg.gradle.workers.max={self.args.workers}")
            env["PATH"] = "/mnt/android-cache/java17/bin:" + env["PATH"]
        (home / "runner-temp").mkdir(exist_ok=True)
        step.update(status="running", log=log.name, started_at=time.time())
        save(self.output / "report.json", self.report)
        print(f"RUN {job['id']}: {step['name']}", flush=True)
        with log.open("wb") as output:
            command = (self.container.command(script, directory, env) if self.container
                       else self.command(script, directory, home, env))
            child = network = None
            try:
                deadline = time.monotonic() + step["timeout"]
                child, network = self.spawn(command, output, step, home)
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise subprocess.TimeoutExpired(command, step["timeout"])
                    try:
                        code = child.wait(timeout=min(2, remaining))
                        break
                    except subprocess.TimeoutExpired:
                        if min(shutil.disk_usage(path).free for path in
                               (self.output, self.cache, self.target)) < self.args.min_free_gib * 1024**3:
                            raise RuntimeError("disk floor reached during check; original namespace stopped")
            except (subprocess.TimeoutExpired, KeyboardInterrupt, RuntimeError, OSError) as error:
                if self.container:
                    self.container.finish()
                    self.container = None
                    step["reason"] = "original job container stopped; remaining steps will not resume it"
                if child is not None:
                    if child.poll() is None:
                        os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=10)
                code = 124
                if isinstance(error, KeyboardInterrupt):
                    step.update(status="not run", reason="interrupted; original namespace reaped")
                    save(self.output / "report.json", self.report)
                    raise
                if isinstance(error, RuntimeError):
                    step["reason"] = str(error)
            finally:
                if network:
                    network.finish()
        step.update(status=("pass" if code == 0 else "fail") if step.get("execution_started") else "not run", exit_code=code,
                    seconds=round(time.time() - step["started_at"], 2), log_sha256=file_digest(log))
        print(f"{step['status'].upper()} {job['id']}: {step['name']} ({step['seconds']}s)", flush=True)
        for key, filename in (("env", "github-env"), ("path", "github-path")):
            path = home / filename
            if path.exists():
                lines = path.read_text().splitlines()
                if key == "path":
                    self.github_path = [line for line in lines if line.startswith("/home/test/")]
                else:
                    # Only the official DeepSeek fixture receipt is passed to later CLI steps.
                    self.github_env = dict(line.split("=", 1) for line in lines
                                           if line.startswith("ZERUS_TEST_DSH_RECEIPT="))


def run_job(runner, args, report, job):
    binary_ready = False
    fixture_ready = False
    runner.github_env, runner.github_path = {}, []
    android_ready = job["id"] != "android"
    for step in job["steps"]:
        if "body" not in step:
            continue
        if job.get("prerequisite_error"):
            step.update(status="not run", reason=job["prerequisite_error"])
            continue
        if job["id"] == "arch-package" and step["name"] == "Verify installed system bundle in disposable home":
            if not any(row["name"] == "Build and check package" and row["status"] == "pass" for row in job["steps"]):
                step.update(status="not run", reason="package build did not pass; no installed-package claim")
                continue
        if step.get("condition") == "failure()" and not any(row["status"] == "fail" for row in job["steps"]):
            step.update(status="local", reason="failure-only collection is not applicable")
            continue
        if args.plan or (args.match and not re.search(args.match, step["name"])):
            step.update(status="not run", reason="plan only" if args.plan else "excluded by --match")
            continue
        if job["id"] == "android" and step["name"] != "Prepare private JDK 17 and Android tools" and not android_ready:
            step.update(status="not run", reason="private Android/JDK preparation did not pass")
            continue
        if job.get("container_image") and runner.container is None:
            step.update(status="not run", reason="original job container is unavailable; no replacement adopted")
            continue
        if job["id"] == "cli" and step["name"] in ("CLI shell smoke", "CLI Python integration") and not binary_ready:
            step.update(status="not run", reason="exact-SHA CLI rebuild did not pass; stale binary forbidden")
            continue
        if job["id"] == "cli" and step["name"] == "CLI Python integration" and not fixture_ready:
            step.update(status="not run", reason="exact-SHA fake-agent rebuild did not pass")
            continue
        runner.run(job, step)
        if job["id"] == "android" and step["name"] == "Prepare private JDK 17 and Android tools":
            android_ready = step["status"] == "pass"
        if job["id"] == "cli" and step["name"] == "Rebuild exact-SHA CLI":
            binary_ready = step["status"] == "pass"
            if binary_ready:
                step["binary_sha256"] = file_digest(runner.target / "debug/hgs")
        if job["id"] == "cli" and step["name"] == "Build exact-SHA fake agent":
            fixture_ready = step["status"] == "pass"
        job["status"] = summarize(job["steps"])
        save(runner.output / "report.json", report)
    job["status"] = summarize(job["steps"])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--repo", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--target", type=Path, help="exclusive reusable Cargo target (cache lock required)")
    parser.add_argument("--android-cache", type=Path, help="private SDK/JDK cache, optionally on a larger filesystem")
    parser.add_argument("--tool-bin", type=Path, action="append", default=[], help="read-only private fixture tool directory")
    parser.add_argument("--workers", type=int, choices=range(1, 13), default=12)
    parser.add_argument("--max-load", type=float, default=20)
    parser.add_argument("--jobs", help="comma-separated subset; every other job stays explicitly not run")
    parser.add_argument("--match", help="run only matching step names; omitted checks keep job not run")
    parser.add_argument("--plan", action="store_true", help="write plan without starting checks")
    parser.add_argument("--min-free-gib", type=int, default=40)
    parser.add_argument("--online", action="store_true", help="private user-mode outbound network, no host loopback")
    parser.add_argument("--slirp", type=Path, default=shutil.which("slirp4netns"), help="private or system slirp4netns executable")
    args = parser.parse_args(argv)
    if sys.platform != "linux":
        parser.error("phase 1 requires Linux with unprivileged bubblewrap and a private D-Bus")
    commit = resolve(args.repo, args.sha)
    workflow_bytes = subprocess.check_output(["git", "-C", str(args.repo), "show", commit + ":" + WORKFLOW])
    cli = git(args.repo, "show", commit + ":scripts/ci/cli.sh") + "\n"
    selected = set(args.jobs.split(",")) if args.jobs else None
    workflow = load_workflow(workflow_bytes)
    if selected and selected - workflow["jobs"].keys():
        parser.error("unknown jobs: " + ",".join(sorted(selected - workflow["jobs"].keys())))
    report = {"version": 1, "commit": commit, "workflow_sha256": digest(workflow_bytes),
              "cli_sha256": digest(cli.encode()), "runner_sha256": digest(Path(__file__).read_bytes()),
              "network_helper_sha256": file_digest(Path(__file__).with_name("local_network.py")),
              "platform": "Arch/Linux local preflight", "network": "private outbound" if args.online else "offline",
              "rust": RUST, "workers": args.workers,
              "jobs": plan(workflow, cli, selected, args.online), "completed": False}
    report["harness_files"] = {path.name: file_digest(path) for path in Path(__file__).parent.glob("local*.py")}
    if args.plan:
        args.output.mkdir(parents=True, exist_ok=False)
        save(args.output / "report.json", report)
        print("Plan (no checks started):", args.output / "report.json")
        return 2
    runner = Runner(args, report)
    try:
        runner.prepare()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        report["preparation_error"] = str(error)
        if runner.created_output:
            save(runner.output / "report.json", report)
        raise
    for job in report["jobs"]:
        try:
            if any("body" in step for step in job["steps"]):
                runner.new_checkout(job)
            if job.get("container_image"):
                runner.container = Container(runner, job["container_image"]).start()
                job["container_id"] = runner.container.identity
            if job.get("services") and any("body" in step for step in job["steps"]):
                runner.postgres = Postgres(runner.output, job["services"]["postgres"]["image"]).start()
                job["service_container_id"] = runner.postgres.container
            run_job(runner, args, report, job)
        except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
            job["reason"] = "local prerequisite failed: " + str(error)
            job["status"] = "fail"
            save(runner.output / "report.json", report)
        finally:
            if runner.container:
                runner.container.finish()
                runner.container = None
            if runner.postgres:
                runner.postgres.finish()
                runner.postgres = None
    report["completed"] = not args.plan
    save(runner.output / "report.json", report)
    for job in report["jobs"]:
        print(f"{job['status'].upper():7} {job['id']}: {job.get('reason', job['name'])}")
    print("Report:", runner.output / "report.json")
    return 1 if any(job["status"] == "fail" for job in report["jobs"]) else (
        2 if any(job["status"] == "not run" for job in report["jobs"]) else 0)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print("local CI refused:", error, file=sys.stderr)
        sys.exit(2)
