"""Execute the Arch package job in its pinned, disposable CI container."""
import json
from pathlib import Path
import subprocess
import uuid


class Container:
    def __init__(self, runner, image):
        if not image.startswith("archlinux@sha256:"):
            raise ValueError("only the workflow's pinned Arch container is supported")
        self.runner, self.image = runner, image
        self.token = str(uuid.uuid4())
        self.network = "zerus-local-ci-" + self.token
        self.identity = None
        self.env = {"PATH": "/usr/bin:/bin", "HOME": str(runner.output),
                    "DOCKER_CONFIG": str(runner.output / "docker-config")}
        self.metadata = runner.output / "container-metadata"
        self.metadata.mkdir()

    def docker(self, *args):
        return subprocess.check_output(["docker", *args], env=self.env, text=True, timeout=300).strip()

    def start(self):
        self.docker("network", "create", "--label", "dev.zerus.local-ci=" + self.token, self.network)
        wrapper = self.metadata / "cargo"
        wrapper.write_text('''#!/bin/bash
set -euo pipefail
selected="${CARGO_TARGET_DIR:-$PWD/target}"
case "$selected" in /mnt/workspace/*/target|/mnt/workspace/target) ;; *) echo 'Cargo target escaped the private checkout' >&2; exit 2 ;; esac
if [ ! -e "$selected" ] && [ ! -L "$selected" ]; then ln -s /mnt/build "$selected"; fi
if [ "$(readlink "$selected")" != /mnt/build ]; then echo 'Unrecognized local target' >&2; exit 2; fi
exec env CARGO_TARGET_DIR=/mnt/build /usr/bin/cargo "$@"
''')
        wrapper.chmod(0o755)
        objects = Path(self.runner.workspace / ".git/objects/info/alternates").read_text().strip()
        try:
            self.identity = self.docker(
                "run", "--detach", "--rm", "--init", "--network", self.network,
                "--label", "dev.zerus.local-ci=" + self.token,
                "--cpus", str(self.runner.args.workers), "--memory", "32g", "--pids-limit", "4096",
                "--security-opt", "seccomp=unconfined", "--security-opt", "apparmor=unconfined",
                "--mount", f"type=bind,src={self.runner.workspace},dst=/mnt/workspace",
                "--mount", f"type=bind,src={objects},dst={objects},readonly",
                "--mount", f"type=bind,src={self.runner.target},dst=/mnt/build",
                "--mount", f"type=bind,src={self.runner.cache / 'cargo'},dst=/home/user/.cargo",
                "--mount", f"type=bind,src={self.metadata},dst=/mnt/metadata",
                "--mount", f"type=bind,src={wrapper},dst=/usr/local/bin/cargo,readonly",
                "--workdir", "/mnt/workspace", self.image, "sleep", "infinity")
            return self
        except BaseException:
            self.finish()
            raise

    def command(self, script, directory, environment):
        destination = self.metadata / script.name
        destination.write_bytes(script.read_bytes())
        environment = dict(environment, HOME="/root", PATH="/usr/local/bin:/usr/bin:/bin",
                           GITHUB_WORKSPACE="/mnt/workspace", GITHUB_OUTPUT="/mnt/metadata/output",
                           RUNNER_TEMP="/tmp/runner", TMPDIR="/tmp", CARGO_HOME="/home/user/.cargo",
                           XDG_CONFIG_HOME="/home/user/.config", XDG_DATA_HOME="/home/user/.local/share",
                           XDG_STATE_HOME="/home/user/.local/state", XDG_CACHE_HOME="/home/user/.cache",
                           XDG_RUNTIME_DIR="/home/user/.runtime")
        for name in ("RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "CARGO_TARGET_DIR", "DBUS_SESSION_BUS_ADDRESS"):
            environment.pop(name, None)
        command = ["docker", "exec", "--workdir", str(Path("/mnt/workspace") / directory)]
        for name, value in environment.items():
            command += ["--env", name + "=" + value]
        return command + [self.identity, "/bin/bash", "-e", "-o", "pipefail", "/mnt/metadata/" + script.name]

    def finish(self):
        if self.identity:
            result = subprocess.run(["docker", "inspect", self.identity], env=self.env,
                                    capture_output=True, text=True, timeout=15)
            if result.returncode == 0:
                observed = json.loads(result.stdout)[0]
                if observed["Config"]["Labels"].get("dev.zerus.local-ci") != self.token:
                    raise RuntimeError("refusing cleanup of a foreign job container")
                self.docker("stop", "--time", "5", self.identity)
            self.identity = None
        self.docker("network", "rm", self.network)
