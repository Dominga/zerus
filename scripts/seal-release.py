#!/usr/bin/env python3
"""Seal an assembled candidate after all independent qualification jobs succeed."""
import argparse
from pathlib import Path
import subprocess
from release_common import finish_candidate, validate_candidate

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('directory', type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
version = (root / 'VERSION').read_text().strip()
commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
finish_candidate(args.directory)
validate_candidate(args.directory, version, commit)
