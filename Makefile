.PHONY: all build build-release install install-release clean test check fmt lint run demo release

LEVEL ?= minor

# Default target
all: check build test

# Build debug version
build:
	cargo build

# Build release version
build-release:
	cargo build --release

# Install debug binary to ~/.cargo/bin
install:
	CARGO_INCREMENTAL=0 cargo install --path . --locked --bins --debug --force

# Install release binary to ~/.cargo/bin
install-release:
	CARGO_INCREMENTAL=0 cargo install --path . --locked --bins --force

# Clean build artifacts
clean:
	cargo clean

# Run tests
#
# nextest runs tests in one parallel pool and surfaces slow cases. It does not
# run doctests, so those still go through cargo — together they match `cargo test`.
# Fall back when cargo-nextest is not installed.
test:
	@if command -v cargo-nextest >/dev/null 2>&1; then \
		cargo nextest run && cargo test --doc; \
	else \
		echo "cargo-nextest not found; using cargo test. Install it with:"; \
		echo "    cargo install cargo-nextest --locked"; \
		cargo test; \
	fi

# Type-check and lint
#
# Clippy uses the same front end as `cargo check` and adds lints on top.
check:
	cargo clippy --all-targets -- -D warnings

# Format code
fmt:
	cargo fmt --all

# Lint (check formatting)
lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

# Run with arguments (usage: make run ARGS="search ubuntu")
run:
	cargo run -- $(ARGS)

# Quick demo
demo: install
	@echo "=== pirata demo ==="
	pirata --help

# Bump version, finalize CHANGELOG.md, tag, publish, and push (requires cargo-release)
release:
	cargo release $(LEVEL) --execute --no-confirm
