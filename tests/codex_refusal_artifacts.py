"""Keep only the bounded Codex refusal sample from an explicit private fixture."""
import hashlib
import os
from pathlib import Path
import stat
import tempfile

LIMIT = 16 * 1024  # src/state/input/diagnostic.rs has the same maximum.


def preserve(fixture_root, test_id, artifact_root):
    """Best effort before cleanup; never alter the send result or retry input."""
    root = Path(fixture_root)
    source = root / "state/input_diagnostics/codex.json"
    try:
        # No home/config fallback or linked source can reach outside this fixture.
        for directory in (root, root / "state", source.parent):
            if not stat.S_ISDIR(directory.lstat().st_mode):
                return
        if not stat.S_ISREG(source.lstat().st_mode):
            return
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
        with os.fdopen(os.open(source, flags), "rb") as sample:
            if not stat.S_ISREG(os.fstat(sample.fileno()).st_mode):
                return
            raw = sample.read(LIMIT + 1)
        if not raw or len(raw) > LIMIT:
            print("Codex refusal diagnostic unavailable: empty or oversized fixture sample", flush=True)
            return
        folder = Path(artifact_root) / "input-diagnostics"
        folder.mkdir(parents=True, exist_ok=True)
        prefix = "codex-" + hashlib.sha256(test_id.encode()).hexdigest()[:12] + "-"
        with tempfile.NamedTemporaryFile(dir=folder, prefix=prefix, suffix=".json", delete=False) as saved:
            saved.write(raw)
        print(f"Codex refusal diagnostic: input-diagnostics/{Path(saved.name).name}", flush=True)
    except FileNotFoundError:
        pass  # Most cases have no refusal sample.
    except OSError as error:
        print(f"Codex refusal diagnostic unavailable: {type(error).__name__}", flush=True)
