#!/usr/bin/env bash
# Local equivalent of the Arch package CI job.
set -euo pipefail
cd "$(dirname "$0")/../.."
git rev-parse --verify HEAD > /dev/null
bash scripts/build-arch-package.sh
