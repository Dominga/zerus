"""Validate fork provenance without executing or trusting external paths."""
import datetime
import re

DISTRIBUTION = "romanilyin/zerus"
CHANNEL = "candidate"
SOURCE = re.compile(r"[0-9a-f]{40}")
SEMVER = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def fork_version(value):
    require(isinstance(value, str) and len(value) <= 32, "Missing or oversized fork version")
    matched = re.fullmatch(r"([0-9]{4})\.([0-9]{1,2})\.([0-9]{1,2})-([1-9][0-9]*)", value)
    require(matched is not None, "Fork version must be YYYY.M.D-N")
    datetime.date(*(int(matched[i]) for i in (1, 2, 3)))
    return value


def cli_identity(value, commit, version, operating_system):
    require(isinstance(value, dict) and type(value.get("schema_version")) is int and value["schema_version"] == 1, "Invalid CLI build identity")
    require(isinstance(commit, str) and SOURCE.fullmatch(commit), "Invalid expected source commit")
    require(value.get("product") == "hgs" and value.get("distribution") == DISTRIBUTION,
            "CLI belongs to another distribution")
    require(value.get("source_commit") == commit and value.get("source_dirty") is False,
            "CLI is dirty or belongs to another source commit")
    require(value.get("fork_version") == fork_version(version) and value.get("channel") == CHANNEL,
            "CLI belongs to another fork version or channel")
    require(value.get("os") == operating_system and value.get("architecture") == "x86_64",
            "CLI platform or architecture does not match its component")
    require(isinstance(value.get("version"), str) and SEMVER.fullmatch(value["version"]), "Invalid CLI base version")
    require(value.get("wsl_receiver_abi") == "wsl-local-v1" and type(value.get("path_ref_schema")) is int and value["path_ref_schema"] == 1
            and value.get("opencode_binding_abi") == "opencode-bind-existing-v1", "CLI contracts do not match this candidate")
    return value


def gui_identity(value, commit, version):
    require(isinstance(value, dict), "Invalid desktop build identity")
    require(value.get("distribution") == DISTRIBUTION and value.get("forkVersion") == fork_version(version)
            and value.get("forkChannel") == CHANNEL, "Desktop distribution, fork version or channel mismatch")
    require(value.get("sourceCommit") == commit and value.get("sourceDirty") is False,
            "Desktop is dirty or belongs to another source commit")
    require(value.get("forkUpdatesEnabled") is False, "An unqualified update channel was enabled")
    require(isinstance(value.get("version"), str) and SEMVER.fullmatch(value["version"]), "Invalid desktop base version")
    return value
