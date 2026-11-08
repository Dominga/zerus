"""The lore binary Zerus ships is CodeLore's, built by the crates/lore-bin package from a git
dependency pinned in that package's Cargo.lock. This is where the packaging scripts ask what
it is: the manifest to build and the version the lock pins."""
from pathlib import Path
import re

MANIFEST = "crates/lore-bin/Cargo.toml"
LOCK = "crates/lore-bin/Cargo.lock"


def lore_version(root: Path) -> str:
    """The version of the pinned `codelore` package, which is what `lore --version` prints."""
    text = (Path(root) / LOCK).read_text(encoding="utf-8").replace("\r\n", "\n")
    found = re.search(r'\[\[package\]\]\nname = "codelore"\nversion = "([^"]+)"', text)
    if not found:
        raise SystemExit(f"{LOCK} does not pin the codelore package")
    return found.group(1)
