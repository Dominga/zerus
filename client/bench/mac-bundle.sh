#!/usr/bin/env bash
# Wraps a macOS executable in a minimal app bundle so `open` can start it in the
# owner's console session (a process started over SSH has no window server).
#   client/bench/mac-bundle.sh EXECUTABLE BUNDLE.app BUNDLE_ID [agent]
# "agent" marks the bundle LSUIElement (no Dock icon, no menu bar), for window-less probes.
# Run it:  open -n -W --stdout OUT --stderr ERR BUNDLE.app --args ...
set -euo pipefail
exe="$1" bundle="$2" id="$3" agent="${4:-}"
name="$(basename "$exe")"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS"
cp "$exe" "$bundle/Contents/MacOS/$name"
ui=""
[[ "$agent" == agent ]] && ui="<key>LSUIElement</key><true/>"
cat >"$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>$name</string>
<key>CFBundleIdentifier</key><string>$id</string>
<key>CFBundleName</key><string>$name</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
$ui
</dict></plist>
PLIST
codesign --force --sign - "$bundle" >/dev/null 2>&1 || true
