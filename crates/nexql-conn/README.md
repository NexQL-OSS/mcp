# nexql-conn

Connection resolution ladder, connection pools, credential providers, and TLS handling for the `nexql-mcp` Postgres server.

## Capabilities

- **7-step Precedence Resolution**: DSN -> named profile -> env vars (`DATABASE_URL`, `POSTGRES_URL`) -> `~/.pgpass` -> defaults.
- **Credential Providers**: Secrets resolution via `${env:VAR}`, `password_command`, `password_file`, and OS keyring backend (`keyring` crate).
- **Deadpool Connection Pools**: Pooled connections with TLS support via `rustls-pki-types`.
- **Live Connection Probe**: Connectivity test function shared by `doctor`, TUI, and `profile test`.
