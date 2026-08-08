# PyPI + uv tool install

`uv tool install` pulls from [PyPI](https://pypi.org) — there is no separate tool registry in the [astral-sh/uv](https://github.com/astral-sh/uv) repo. Publishing `nexql-mcp` wheels to PyPI is all that is required for:

```bash
uv tool install nexql-mcp
uvx nexql-mcp --version
```

## Layout in this repo

| Path | Purpose |
|------|---------|
| [`pypi/pyproject.toml`](../pypi/pyproject.toml) | maturin `bindings = "bin"` — wraps `crates/nexql-mcp` |
| [`pypi/python/nexql_mcp/`](../pypi/python/nexql_mcp/) | minimal Python package (maturin requirement) |
| [`server.json`](../server.json) | MCP Registry `packages[]` entry (`registryType: pypi`) |

Same prebuilt-binary-per-platform model as npm: release CI builds the Rust binary on each matrix leg, then maturin packages it into a platform wheel.

## One-time setup

1. **PyPI project** — create `nexql-mcp` on [pypi.org](https://pypi.org) (or claim if reserved).
2. **Trusted publishing** — link the PyPI project to this GitHub repo (OIDC), same pattern as npm provenance.
3. **Secrets** — if not using trusted publishing, add `PYPI_API_TOKEN` to repo secrets.

## Release CI (`publish-pypi` job)

Add a job after the `build` matrix (reuses the same compiled binaries; no second Rust compile needed if you package from `target/`):

```yaml
publish-pypi:
  needs: build
  runs-on: ubuntu-latest
  permissions:
    id-token: write   # trusted publishing
  strategy:
    fail-fast: false
    matrix:
      include:
        - runner: ubuntu-latest
          triple: x86_64-unknown-linux-gnu
        - runner: ubuntu-24.04-arm
          triple: aarch64-unknown-linux-gnu
        - runner: macos-14
          triple: x86_64-apple-darwin
        - runner: macos-latest
          triple: aarch64-apple-darwin
        - runner: windows-latest
          triple: x86_64-pc-windows-msvc
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        targets: ${{ matrix.triple }}
    - name: Install clang (Linux)
      if: runner.os == 'Linux'
      run: sudo apt-get update && sudo apt-get install -y clang libclang-dev
    - uses: astral-sh/setup-uv@v5
    - name: Build wheel
      shell: bash
      run: |
        set -euo pipefail
        cargo build --release --locked -p nexql-mcp --target ${{ matrix.triple }}
        uv tool install "maturin>=1.9,<2.0"
        cd pypi
        maturin build --release --target ${{ matrix.triple }} -o dist
    - name: Publish wheel
      env:
        MATURIN_PYPI_TOKEN: ${{ secrets.PYPI_API_TOKEN }}
      run: uv publish pypi/dist/*.whl
```

Pin `pypi/pyproject.toml` `version` in the same `bump-version.sh` pass as `Cargo.toml` / `npm/package.json`.

## `server.json`

Add a PyPI package block (mirror the existing npm entry):

```json
{
  "registryType": "pypi",
  "registryBaseUrl": "https://pypi.org",
  "identifier": "nexql-mcp",
  "version": "0.2.1",
  "runtimeHint": "uvx",
  "transport": { "type": "stdio" }
}
```

Re-publish the MCP Registry entry after the first PyPI release lands.

## Verify locally

```bash
# Build a wheel for the host platform
cargo build --release -p nexql-mcp
uv tool install "maturin>=1.9,<2.0"
cd pypi && maturin build --release -o dist

# Install into a throwaway tool env
uv tool install --force dist/nexql_mcp-*.whl
nexql-mcp --version
nexql-mcp postgres://dev@localhost:5432/appdb doctor
```

## User-facing install (README)

Once PyPI is live, document:

```bash
# Install uv (if needed)
curl -LsSf https://astral.sh/uv/install.sh | sh          # macOS / Linux
# irm https://astral.sh/uv/install.ps1 | iex           # Windows

uv tool install nexql-mcp
uv tool update-shell    # once, if PATH warning appears
```

No PR to `astral-sh/uv` is required — uv discovers any PyPI package with a console entry point automatically.
