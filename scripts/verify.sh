#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --workspace

echo "NEON verification passed."
