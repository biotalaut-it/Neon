.PHONY: help fmt test check build run install ci

help:
	@echo "NEON project commands"
	@echo "  make fmt      - format Rust sources"
	@echo "  make test     - run workspace tests"
	@echo "  make check    - cargo check"
	@echo "  make build    - build release binary"
	@echo "  make run      - run neon CLI"
	@echo "  make install  - install neon binary locally"
	@echo "  make ci       - format + lint + test + release build"

fmt:
	cargo fmt --all

test:
	cargo test --workspace

check:
	cargo check --workspace

build:
	cargo build --release --workspace

run:
	cargo run -p neon -- chat

install:
	cargo install --path crates/neon --locked

ci:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace
	cargo build --release --workspace
