#!/usr/bin/env bash
set -euo pipefail

source_sha=$1
python3 scripts/ci/native-effort-seed.py build --source-sha "$source_sha" \
  --hgs "$GITHUB_WORKSPACE/target/debug/hgs" --image "$GITHUB_WORKSPACE/target/debug/hgs-effort-seed" \
  --receipt "$GITHUB_WORKSPACE/target/debug/hgs-effort-seed.json"
python3 scripts/ci/agent-compatibility.py
