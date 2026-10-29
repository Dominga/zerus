#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

platform="${1:?usage: scripts/ci/client-native.sh linux|macos|windows [core|media|tauri-test|tauri-clippy]}"
stage="${2:-all}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/client-native}"

core_tests() {
    cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml --no-fail-fast
}

media_check() {
    cargo +1.99.1 check --locked --manifest-path client/media/Cargo.toml --all-targets --features video
}

tauri_tests() {
    cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml -- --list
    RUST_BACKTRACE=1 cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml -- --test-threads=1
}

tauri_clippy() {
    cargo +1.99.1 clippy --locked --manifest-path client/src-tauri/Cargo.toml --all-targets -- -D warnings
}

case "$platform:$stage" in
    linux:all)
        cargo +1.99.1 test --locked --manifest-path client/media/Cargo.toml --features video
        env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
            xvfb-run -a -s "-screen 0 1440x900x24" \
            cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml
        cargo +1.99.1 clippy --locked --manifest-path client/src-tauri/Cargo.toml --all-targets --features video -- -D warnings
        ;;
    macos:all)
        core_tests
        tauri_tests
        media_check
        tauri_clippy
        ;;
    windows:all)
        core_tests
        media_check
        tauri_clippy
        ;;
    macos:core)
        core_tests
        ;;
    macos:media)
        media_check
        ;;
    macos:tauri-test)
        tauri_tests
        ;;
    macos:tauri-clippy)
        tauri_clippy
        ;;
    *)
        echo "usage: $0 linux|macos|windows [core|media|tauri-test|tauri-clippy]" >&2
        exit 2
        ;;
esac
