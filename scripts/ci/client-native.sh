#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

platform="${1:?usage: scripts/ci/client-native.sh linux|macos|windows}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/client-native}"

case "$platform" in
    linux)
        cargo +1.99.1 test --locked --manifest-path client/media/Cargo.toml --features video
        env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
            xvfb-run -a -s "-screen 0 1440x900x24" \
            cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml
        cargo +1.99.1 clippy --locked --manifest-path client/src-tauri/Cargo.toml --all-targets --features video -- -D warnings
        ;;
    macos|windows)
        cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml --no-fail-fast
        if [[ "$platform" == macos ]]; then
            cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml
        fi
        cargo +1.99.1 check --locked --manifest-path client/media/Cargo.toml --all-targets --features video
        cargo +1.99.1 clippy --locked --manifest-path client/src-tauri/Cargo.toml --all-targets -- -D warnings
        ;;
    *)
        echo "usage: $0 linux|macos|windows" >&2
        exit 2
        ;;
esac
