#!/usr/bin/env bash
# Build and assemble the unsigned Apple Silicon Zerus.app against the pinned
# official Qt. Uses no secrets; the macos-signing job signs the checked result.
set -euo pipefail
run_tests="${ZERUS_RUN_TESTS:-true}"
case "$run_tests" in
    true|false) ;;
    *) echo 'ZERUS_RUN_TESTS must be true or false.' >&2; exit 2 ;;
esac
if [ "$run_tests" = false ]; then
    if [[ "${ZERUS_RELEASE_CHANNEL:-}" != dev || ! "${ZERUS_DEV_BUILD_NUMBER:-}" =~ ^[1-9][0-9]*$ ]]; then
        echo 'Skipping full bundle tests requires the Dev channel and a positive Dev build number.' >&2
        exit 2
    fi
fi
cd "$(dirname "$0")/../.."
output="${ZERUS_MACOS_OUTPUT:-$PWD/artifacts/macos-unsigned}"
version="$(cat VERSION)"
# A nightly passes its release version; stable bundles use the product version.
release="${ZERUS_RELEASE_VERSION:-$version}"
if [ "$run_tests" = false ] && [ "$release" != "$(cat DEV_VERSION)-dev.$ZERUS_DEV_BUILD_NUMBER" ]; then
    echo 'Build-only bundles require the exact numbered Dev release.' >&2
    exit 2
fi
qt="${ZERUS_QT_PREFIX:?set ZERUS_QT_PREFIX to the prefix from install-qt-macos.py}"
jobs="${ZERUS_BUILD_JOBS:-3}"
command -v tmux > /dev/null || { echo 'Install tmux before bundle self-tests.' >&2; exit 1; }
export MACOSX_DEPLOYMENT_TARGET=14.0
mkdir -p artifacts/test-results
cargo build --locked --release
cargo build --locked --release --manifest-path services/mobile/connector/Cargo.toml
rm -rf artifacts/macos-licenses
python3 scripts/collect-licenses.py artifacts/macos-licenses \
    --manifest-path Cargo.toml --manifest-path services/mobile/connector/Cargo.toml
cmake_args=(-G Ninja -DCMAKE_BUILD_TYPE=Release "-DCMAKE_PREFIX_PATH=$qt"
            -DCMAKE_OSX_DEPLOYMENT_TARGET=14.0 -DCMAKE_OSX_ARCHITECTURES=arm64)
if [ -n "${ZERUS_DEV_BUILD_NUMBER:-}" ]; then
    cmake_args+=("-DZERUS_DEV_BUILD_NUMBER=$ZERUS_DEV_BUILD_NUMBER")
fi
# Release bundles always configure from scratch: a cached build directory can name
# an SDK that an Xcode update has since removed.
rm -rf .ci-build/macos-tests
# Full regression tests remain the default, against exactly the bundled Qt.
build_testing=ON
if [ "$run_tests" = false ]; then build_testing=OFF; fi
cmake -S tray -B .ci-build/macos-tests "${cmake_args[@]}" "-DBUILD_TESTING=$build_testing"
cmake --build .ci-build/macos-tests --parallel "$jobs"
if [ "$run_tests" = true ]; then
    QT_QPA_PLATFORM=offscreen \
        ctest --test-dir .ci-build/macos-tests --parallel "$jobs" --timeout 240 --output-on-failure \
        --output-junit "$PWD/artifacts/test-results/macos-bundle-qt.xml"
else
    echo 'Dev build-only bundle: full Qt regression tests were not run; package checks remain required.'
fi
python3 scripts/package-macos.py --qt "$qt" --app .ci-build/macos-tests/hgs-tray.app \
    --cli target/release/hgs --connector services/mobile/connector/target/release/zerus-mobile-connector \
    --licenses artifacts/macos-licenses --version "$version" --release "$release" \
    --output "$output"
python3 scripts/check-macos-bundle.py "$output/Zerus.app" --version "$version" --release "$release" --selftest --source-commit "$(git rev-parse HEAD)"
# ditto keeps the framework symlinks and permissions that artifact uploads drop.
ditto -c -k --keepParent "$output/Zerus.app" "$output/Zerus-unsigned.zip"
rm -rf "$output/Zerus.app"
