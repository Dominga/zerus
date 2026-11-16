#!/usr/bin/env python3
"""Run only the reviewed RAM accounting LIBRARY control; print aggregates only.

stdin: {"listing_json": "<complete actual native stdout>",
        "computer_name": "<actual display name>", "binding": "<actual binding hash>",
        "via": null}
No credentials/profile/service configuration are accepted or needed. Native
augmentation uses the chosen CLI, with a narrow read-only call allowlist. The
original service/caches are never inspected. Source docs explain this difference.
"""
import argparse
import json
import os
from pathlib import Path
import resource
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-bin', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    args = parser.parse_args()
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    env = dict(os.environ, ZERUS_ACCOUNTING_NATIVE=str(args.native.resolve()), RUST_BACKTRACE='0')
    result = subprocess.run([str(args.test_bin.resolve()), '--exact',
                            'encrypted_runtime::tests::payload_accounting::account_complete_native_body',
                            '--ignored', '--nocapture', '--test-threads=1'],
                            stdin=sys.stdin.buffer, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, env=env, timeout=60)
    reports = [line.partition(b'IU768_ACCOUNTING:')[2] for line in result.stdout.splitlines()
               if b'IU768_ACCOUNTING:' in line]
    if result.returncode or len(reports) != 1:
        print('{"ok":false,"error_class":"accounting control did not complete"}')
        return 1
    report = json.loads(reports[0])
    print(json.dumps(report, separators=(',', ':')))
    return 0 if report.get('ok') else 1


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.TimeoutExpired):
        print('{"ok":false,"error_class":"accounting invocation failed"}')
        raise SystemExit(1)
