# Contributing to nexql-mcp

Thank you for your interest in contributing to `nexql-mcp`!

## Development Setup

1. **Toolchain**: Ensure Rust 1.85+ and Node.js 20+ are installed.
2. **Clone & Build**:
   ```bash
   cd mcp
   cargo build --workspace
   ```
3. **Run Code Quality Checks**:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

## Local Testing & Smoke Harness

- Run end-to-end stdio smoke tests:
  ```bash
  ./scripts/local_mcp_smoke.sh
  ```
- Check version consistency across manifests:
  ```bash
  ./scripts/bump-version.sh --check
  ```

## Pull Request Guidelines

- All commits must pass `cargo clippy`, `cargo fmt`, and `cargo test`.
- New tools or behavior changes must include unit/integration tests.
- Version bumps must use `./scripts/bump-version.sh <version>`.
