# Multi-stage release image for nexql-mcp (stdio MCP server).
# Builder needs clang — pg_query bindgen links libclang.
#
# Build:
#   docker build -t nexql-mcp:0.1.0 .
# Run (stdio):
#   docker run --rm -i nexql-mcp:0.1.0 postgres://user:pass@host.docker.internal:5432/db

FROM rust:1.88-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends clang libclang-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN LIBCLANG_PATH="$(dirname "$(find /usr -name 'libclang.so*' 2>/dev/null | head -1)")" \
    && export LIBCLANG_PATH \
    && cargo build --release -p nexql-mcp \
    && strip target/release/nexql-mcp

FROM gcr.io/distroless/cc-debian12

COPY --from=builder /src/target/release/nexql-mcp /usr/local/bin/nexql-mcp

# Connection string / flags via argv or env (NEXQL_MCP_*).
ENTRYPOINT ["/usr/local/bin/nexql-mcp"]
