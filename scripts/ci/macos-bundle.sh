#!/usr/bin/env bash
# Build and assemble the unsigned Apple Silicon Zerus Next app. Uses no
# secrets; the trusted macos-signing job signs the checked result.
set -euo pipefail
run_tests="${ZERUS_RUN_TESTS:-true}"
case "$run_tests" in
    true|false) ;;
    *) echo 'ZERUS_RUN_TESTS must be true or false.' >&2; exit 2 ;;
esac
if [ "$run_tests" = false ]; then
    if [[ "${ZERUS_RELEASE_CHANNEL:-}" = stable && "${ZERUS_MACOS_TESTS_IN_VERIFY:-}" = true ]]; then
        : # The caller must gate signing/sealing on its same-SHA full matrix.
    elif [[ "${ZERUS_RELEASE_CHANNEL:-}" != dev || ! "${ZERUS_DEV_BUILD_NUMBER:-}" =~ ^[1-9][0-9]*$ ]]; then
        echo 'Skipping bundle tests requires the Dev gate or same-SHA stable verification.' >&2
        exit 2
    fi
fi
cd "$(dirname "$0")/../.."
output="${ZERUS_MACOS_OUTPUT:-$PWD/artifacts/macos-unsigned}"
version="$(cat VERSION)"
# A nightly passes its release version; stable bundles use the product version.
release="${ZERUS_RELEASE_VERSION:-$version}"
if [ "$run_tests" = false ] && [ "${ZERUS_RELEASE_CHANNEL:-}" = dev ] && [ "$release" != "$(cat DEV_VERSION)-dev.$ZERUS_DEV_BUILD_NUMBER" ]; then
    echo 'Build-only bundles require the exact numbered Dev release.' >&2
    exit 2
fi
if [ "$run_tests" = false ] && [ "${ZERUS_RELEASE_CHANNEL:-}" = stable ] && [ "$release" != "$version" ]; then
    echo 'Stable build-only bundles require the exact product release.' >&2
    exit 2
fi
export CARGO_BUILD_JOBS="${ZERUS_BUILD_JOBS:-3}"
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="$PWD/target"
command -v tmux > /dev/null || { echo 'Install tmux before bundle self-tests.' >&2; exit 1; }
export MACOSX_DEPLOYMENT_TARGET=14.0
mkdir -p artifacts/test-results
cargo build --locked --release
cargo build --locked --release --manifest-path services/mobile/connector/Cargo.toml
rm -rf artifacts/macos-licenses artifacts/macos-client-licenses artifacts/macos-frontend-licenses
python3 scripts/collect-licenses.py artifacts/macos-licenses \
    --manifest-path Cargo.toml --manifest-path services/mobile/connector/Cargo.toml
switch_config="$PWD/target/macos-switch.conf.json"
python3 client/scripts/switch-config.py "$switch_config" --release "$release"
(cd client && npm exec -- tauri build --config "$switch_config" --bundles app --features video --ci --no-sign -- --locked)
python3 scripts/collect-licenses.py artifacts/macos-client-licenses \
    --manifest-path client/src-tauri/Cargo.toml --manifest-features client/src-tauri/Cargo.toml=video
python3 client/scripts/collect-notices.py artifacts/macos-frontend-licenses
if [ "$run_tests" = true ]; then
    (cd client && npm run check && npm test) 2>&1 | tee artifacts/test-results/macos-bundle-web.log
    cargo test --locked --release --manifest-path client/core/Cargo.toml \
        2>&1 | tee artifacts/test-results/macos-bundle-core.log
    # The drive-folder integration tests run the release CLI built above from
    # this source, never an inherited or installed one (zerus-h43u3).
    HGS_TEST_BIN="$CARGO_TARGET_DIR/release/hgs" \
    cargo test --locked --release --manifest-path client/src-tauri/Cargo.toml --features video \
        2>&1 | tee artifacts/test-results/macos-bundle-tauri.log
else
    echo 'Build-only bundle: regression tests belong to the caller gate; package checks remain required.'
fi
python3 scripts/package-macos.py --gui tauri --app target/release/bundle/macos/Zerus.app \
    --cli target/release/hgs --connector target/release/zerus-mobile-connector \
    --licenses artifacts/macos-licenses --version "$version" --release "$release" \
    --client-licenses artifacts/macos-client-licenses --frontend-licenses artifacts/macos-frontend-licenses \
    --output "$output"
python3 scripts/check-macos-bundle.py "$output/Zerus.app" --gui tauri --version "$version" --release "$release" --selftest --source-commit "$(git rev-parse HEAD)"
# ditto keeps the framework symlinks and permissions that artifact uploads drop.
ditto -c -k --keepParent "$output/Zerus.app" "$output/Zerus-unsigned.zip"
rm -rf "$output/Zerus.app"
