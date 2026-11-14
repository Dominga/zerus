#!/usr/bin/env bash
set -euo pipefail

cargo +1.99.1 fmt --check --manifest-path client/core/Cargo.toml
cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml --target-dir target --all-targets -- -D warnings
cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml --target-dir target
