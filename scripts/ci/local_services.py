"""Disposable, labelled local CI services; never publish a host port."""
import json
import os
from pathlib import Path
import subprocess
import time
import uuid


class Postgres:
    def __init__(self, directory, image):
        if not image.startswith("postgres:") or "@sha256:" not in image:
            raise ValueError("local PostgreSQL requires the workflow's pinned official image")
        self.directory, self.image = Path(directory), image
        self.token = str(uuid.uuid4())
        self.network = "zerus-local-ci-" + self.token
        self.container = None
        self.environment = {"PATH": "/usr/bin:/bin", "HOME": str(self.directory),
                            "DOCKER_CONFIG": str(self.directory / "docker-config")}

    def docker(self, *args):
        return subprocess.check_output(["docker", *args], env=self.environment, text=True, timeout=180).strip()

    def start(self):
        self.docker("network", "create", "--label", "dev.zerus.local-ci=" + self.token, self.network)
        try:
            self.container = self.docker("run", "--detach", "--rm", "--network", self.network,
                                         "--label", "dev.zerus.local-ci=" + self.token,
                                         "--cpus", "2", "--memory", "1g", "--pids-limit", "256",
                                         "--env", "POSTGRES_HOST_AUTH_METHOD=trust", self.image)
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                result = subprocess.run(["docker", "exec", self.container, "pg_isready", "-U", "postgres"],
                                        env=self.environment, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.DEVNULL, timeout=10)
                if result.returncode == 0:
                    break
                time.sleep(1)
            else:
                raise RuntimeError("original PostgreSQL service was not ready")
            observed = json.loads(self.docker("inspect", self.container))[0]
            if observed["Config"]["Labels"].get("dev.zerus.local-ci") != self.token:
                raise RuntimeError("original PostgreSQL container identity changed")
            address = observed["NetworkSettings"]["Networks"][self.network]["IPAddress"]
            self.url = "postgresql://postgres@" + address + ":5432/postgres"
            return self
        except BaseException:
            self.finish()
            raise

    def finish(self):
        if self.container:
            # The immutable original container ID and our label must both agree.
            result = subprocess.run(["docker", "inspect", self.container], env=self.environment,
                                    capture_output=True, text=True, timeout=15)
            if result.returncode == 0:
                observed = json.loads(result.stdout)[0]
                if observed["Config"]["Labels"].get("dev.zerus.local-ci") != self.token:
                    raise RuntimeError("refusing cleanup of a foreign service")
                self.docker("stop", "--time", "5", self.container)
            self.container = None
        self.docker("network", "rm", self.network)
