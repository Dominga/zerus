#!/usr/bin/env bash
set -euo pipefail

rustup component add --toolchain 1.99.1 rustfmt clippy
cargo +1.99.1 fmt --check --manifest-path crates/replica/Cargo.toml
cargo +1.99.1 clippy --locked --manifest-path crates/replica/Cargo.toml --target-dir target --all-targets -- -D warnings
cargo +1.99.1 test --locked --manifest-path crates/replica/Cargo.toml --target-dir target
cargo +1.99.1 test --locked --manifest-path crates/replica/Cargo.toml --target-dir target --features test-without-team
cargo +1.99.1 fmt --check --manifest-path crates/replica-ffi/Cargo.toml
cargo +1.99.1 clippy --locked --manifest-path crates/replica-ffi/Cargo.toml --target-dir target --all-targets -- -D warnings
cargo +1.99.1 test --locked --manifest-path crates/replica-ffi/Cargo.toml --target-dir target
