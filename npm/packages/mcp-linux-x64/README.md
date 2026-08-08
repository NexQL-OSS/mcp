# @nexql/mcp-linux-x64

Platform binary package for `nexql-mcp` on **linux/x64**.

Requires **glibc 2.35+** (Ubuntu 22.04, Debian 12, RHEL 9). If you see `GLIBC_2.39 not found`, upgrade `nexql-mcp` past v0.2.1, run `cargo install nexql-mcp`, or use the Docker image.

The `bin/nexql-mcp` executable is filled in by release CI. This stub exists so the parent `nexql-mcp` npm package can declare `optionalDependencies`.
