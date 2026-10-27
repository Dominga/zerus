#!/usr/bin/env bash
# Build the mockup and copy it to the review folder served over Nebula.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
dest="${1:?usage: publish.sh <destination folder>}"
cd "$here"
npx tsc --noEmit
npx vite build --outDir dist
mkdir -p "$dest"
rm -rf "$dest/assets"
cp -r dist/. "$dest/"
echo "published to $dest"
