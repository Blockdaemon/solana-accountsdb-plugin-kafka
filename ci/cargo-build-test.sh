#!/usr/bin/env bash

# Source:
# https://github.com/solana-labs/solana-accountsdb-plugin-postgres/blob/master/ci/cargo-build-test.sh

set -e
cd "$(dirname "$0")/.."

export RUSTFLAGS="-D warnings"
export RUSTBACKTRACE=1

set -x

# Build/test all host crates
rust_stable=$(rustc --version | awk '{print $2}')
cargo +"$rust_stable" build
cargo +"$rust_stable" test -- --nocapture

exit 0
