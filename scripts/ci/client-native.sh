#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

platform="${1:?usage: scripts/ci/client-native.sh linux|macos|windows [core|media|tauri-test|tauri-clippy]}"
stage="${2:-all}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/client-native}"
# Only candidate_cli below provides the CLI the Tauri tests run; an inherited
# value could name a stale or unrelated build.
unset HGS_TEST_BIN

disk_checkpoint() {
    if [[ "$platform" == linux && "${GITHUB_ACTIONS:-}" == true \
        && "${RUNNER_ENVIRONMENT:-}" == github-hosted && "${RUNNER_OS:-}" == Linux \
        && "${GITHUB_JOB:-}" == client-native-linux ]]; then
        python3 scripts/ci/ubuntu-disk.py report "$1"
    fi
}

core_tests() {
    presence_tests
    if [ "$platform" = windows ]; then
        # Exact hosted parent only, with mandatory one-test completion evidence.
        bash scripts/ci/windows-frontend-adversary.sh core
    fi
    cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml --no-fail-fast
    if [ "$platform" = macos ]; then
        cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml \
            --features macos-native-peer --all-targets -- -D warnings
        # Selection refusals and isolated native original-child controls (fixed
        # Apple helpers, no genuine Zerus pair positive). The separate signed-peer
        # fixtures stay ignored and receive no signing identity or key access.
        cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml \
            --features macos-native-peer macos_notify::peer::installation
    fi
}

presence_tests() {
    rustup component add --toolchain 1.99.1 rustfmt
    cargo +1.99.1 fmt --check --manifest-path crates/presence-sampler/Cargo.toml
    cargo +1.99.1 clippy --locked --manifest-path crates/presence-sampler/Cargo.toml --all-targets -- -D warnings
    cargo +1.99.1 test --locked --manifest-path crates/presence-sampler/Cargo.toml
}

media_check() {
    cargo +1.99.1 check --locked --manifest-path client/media/Cargo.toml --all-targets --features video
}

# The drive-folder integration tests run the actual CLI from this source
# (zerus-h43u3). Build it into the same target directory right before the
# Tauri tests and pass that exact executable; a failed build stops here.
candidate_cli() {
    cargo +1.99.1 build --locked --bin hgs
    # Cargo resolves a relative target directory from here; the tests run in
    # their package directory, so they get an absolute path.
    local cli
    if ! cli="$(cd "$CARGO_TARGET_DIR/debug" 2> /dev/null && pwd)/hgs" \
        || [ ! -f "$cli" ] || [ ! -x "$cli" ]; then
        echo "client-native: the source-built CLI is missing at $CARGO_TARGET_DIR/debug/hgs" >&2
        exit 1
    fi
    export HGS_TEST_BIN="$cli"
}

tauri_tests() {
    candidate_cli
    cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml
}

tauri_clippy() {
    cargo +1.99.1 clippy --locked --manifest-path client/src-tauri/Cargo.toml --all-targets -- -D warnings
}

case "$platform:$stage" in
    linux:all)
        presence_tests
        disk_checkpoint after-native-presence-tests
        cargo +1.99.1 test --locked --manifest-path client/media/Cargo.toml --features video
        disk_checkpoint after-native-media-tests
        candidate_cli
        env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
            xvfb-run -a -s "-screen 0 1440x900x24" \
            cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml
        disk_checkpoint after-native-tauri-tests
        cargo +1.99.1 clippy --locked --manifest-path client/src-tauri/Cargo.toml --all-targets --features video -- -D warnings
        disk_checkpoint after-native-tauri-clippy
        # The default-off Linux notification transport compiles nowhere else.
        cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml --all-targets --features linux-native-notify -- -D warnings
        disk_checkpoint before-native-core-test-linking
        dbus-run-session -- cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml --features linux-native-notify linux_notify
        disk_checkpoint after-native-core-tests
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
    windows:core|macos:core)
        core_tests
        ;;
    windows:media|macos:media)
        media_check
        ;;
    macos:tauri-test)
        tauri_tests
        ;;
    windows:tauri-clippy|macos:tauri-clippy)
        tauri_clippy
        ;;
    *)
        echo "usage: $0 linux|macos|windows [core|media|tauri-test|tauri-clippy]" >&2
        exit 2
        ;;
esac
