#!/usr/bin/env python3
"""Select the exact speech fixture produced before parallel Python checks."""
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]


def executable(manifest, root=ROOT):
    with manifest.open("rb") as stream:
        raw = stream.read(4 * 1024 * 1024 + 1)
    if len(raw) > 4 * 1024 * 1024:
        raise ValueError('Speech fixture build metadata exceeds its bound')
    outputs = []
    source = (root / 'tests/voice_recovery_fixture.rs').resolve()
    for line in raw.splitlines():
        item = json.loads(line)
        if (item.get('reason') == 'compiler-artifact'
                and item.get('target', {}).get('name') == 'voice_recovery_fixture'
                and item.get('executable')):
            if Path(item['target']['src_path']).resolve() != source:
                raise ValueError('Speech fixture source differs')
            outputs.append(Path(item['executable']))
    if len(outputs) != 1:
        raise ValueError('Expected exactly one speech fixture executable')
    path = outputs[0]
    if (path.is_symlink() or not path.is_file() or not os.access(path, os.X_OK)
            or not path.resolve().is_relative_to((root / 'target/speech-recovery').resolve())):
        raise ValueError('Speech fixture is not the dedicated build output')
    return path.resolve()


if __name__ == '__main__':
    print(executable(Path(sys.argv[1])))
