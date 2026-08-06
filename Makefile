# Makefile for nexql-mcp
# Export LIBCLANG_PATH for bindgen / pg_query crate build dependency
export LIBCLANG_PATH := /usr/lib

.PHONY: all help check build release test test-nocapture fmt fmt-check clippy lint doctor run clean install smoke perf

all: help

## help: Display available quick access targets
help:
	@echo "Usage: make <target>"
	@echo ""
	@echo "Development:"
	@echo "  check         Run cargo check on workspace"
	@echo "  build         Build debug binaries for workspace"
	@echo "  release       Build release binaries with optimizations"
	@echo "  test          Run all workspace unit & integration tests"
	@echo "  test-nocapture Run tests showing stdout"
	@echo "  clean         Remove build target artifacts"
	@echo "  install       Install nexql-mcp binary locally via cargo"
	@echo ""
	@echo "Linting & Quality:"
	@echo "  fmt           Format code with cargo fmt"
	@echo "  fmt-check     Check code formatting without modifying"
	@echo "  clippy        Run clippy linter with strict warning checks"
	@echo "  lint          Run fmt-check and clippy"
	@echo ""
	@echo "CLI & Verification:"
	@echo "  doctor        Run nexql-mcp doctor check"
	@echo "  run           Run nexql-mcp server"
	@echo "  smoke         Run local MCP smoke test script"
	@echo "  perf          Run performance smoke benchmark"

## check: Run cargo check on workspace
check:
	cargo check --workspace

## build: Build debug binaries for workspace
build:
	cargo build --workspace

## release: Build optimized release binaries
release:
	cargo build --release --workspace

## test: Run all workspace unit & integration tests
test:
	cargo test --workspace

## test-nocapture: Run tests showing stdout
test-nocapture:
	cargo test --workspace -- --nocapture

## fmt: Format code with cargo fmt
fmt:
	cargo fmt --all

## fmt-check: Check formatting compliance
fmt-check:
	cargo fmt --all -- --check

## clippy: Run clippy with strict warnings
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

## lint: Run format check and clippy
lint: fmt-check clippy

## doctor: Run nexql-mcp doctor command
doctor:
	cargo run -p nexql-mcp -- doctor

## run: Run nexql-mcp binary
run:
	cargo run -p nexql-mcp --

## install: Install nexql-mcp binary to cargo bin directory
install:
	cargo install --path crates/nexql-mcp

## clean: Remove cargo build target directory
clean:
	cargo clean

## smoke: Run local MCP smoke test script
smoke:
	./scripts/local_mcp_smoke.sh

## perf: Run performance smoke benchmark
perf:
	./scripts/perf_smoke.sh
