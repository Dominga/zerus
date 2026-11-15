#!/usr/bin/env python3
"""Measure advisory display publication with private real canonical files."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--scratch-parent', type=Path, required=True)
    parser.add_argument('--small', action='store_true', help='64/128 rows for a driver smoke check')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    for key in list(os.environ):
        if key.startswith('HGS_'):
            del os.environ[key]
    def interrupt(number, _frame):
        raise SystemExit(128 + number)

    for sig in [signal.SIGTERM, signal.SIGINT, signal.SIGHUP]:
        signal.signal(sig, interrupt)
    args.scratch_parent.mkdir(parents=True, exist_ok=True)
    required = 100 << 20 if args.small else 18 << 30
    if shutil.disk_usage(args.scratch_parent).free < required:
        parser.error('insufficient space for the private fixture and cleanup margin')
    repo = Path(__file__).resolve().parents[2]
    environment = dict(os.environ, CARGO_BUILD_JOBS='4', CARGO_INCREMENTAL='0',
                       CARGO_TARGET_DIR=str(repo / 'target'))
    with tempfile.TemporaryDirectory(prefix='listing-metadata-', dir=args.scratch_parent) as temporary:
        root = Path(temporary)
        (root / '.listing-metadata-scale-fixture').touch()
        environment.update(ZERUS_LISTING_SCALE_BENCH='1', ZERUS_LISTING_SCALE_ROOT=str(root),
                           ZERUS_LISTING_SCALE_COUNTS='64,128' if args.small else '20000,50000')
        subprocess.run(['cargo', '+1.99.1', 'test', '--locked', '-j', '4', '--bin', 'hgs', '--',
                        'state::listing_metadata::tests::bench::canonical_write_scale', '--exact',
                        '--ignored', '--nocapture', '--test-threads=1'], cwd=repo, env=environment,
                       check=True)
        result = json.loads((root / 'result.json').read_text())
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
