---
layout: ../../layouts/DocsLayout.astro
slug: configuration
eyebrow: Guide · 09
title: Configuration
description: The config file, every profile field, environment variables, and running nexql-mcp over HTTP.
---

You can use NexQL without ever opening a config file: the setup wizard, `profile add` and `add_database` write it for you. This page is the reference for when you want to look or tweak.

## Where things live

| What | Where | Change with |
| --- | --- | --- |
| Your config | `~/.config/nexql-mcp/config.toml` | `NEXQL_MCP_CONFIG`, `--config` |
| A project's config | `.nexql/config.toml` in the project | `--workspace-root` |
| Schema index | `~/.local/share/nexql-mcp` | `NEXQL_MCP_INDEX_DIR` |
| Log file | `~/.config/nexql-mcp/logs/nexql-mcp.log` | `NEXQL_MCP_LOG` |
| Passwords | Your OS keyring | See [Connections](/docs/connections#where-passwords-live) |

## A config file

```toml
default_profile = "local"

[profiles.local]
url = "postgres://dev@localhost:5432/appdb"
access_mode = "read"
personal_data_heuristics = false   # fake data locally; see it unmasked

[profiles.prod]
host = "prod.example.com"
port = 5432
dbname = "app"
user = "readonly_agent"
password_command = "op read op://vault/pg/password"
sslmode = "verify-full"
access_mode = "read"
schemas = ["public", "billing"]
deny_tables = ["auth.*"]
pii_columns = ["public.users.notes"]
max_rows = 200
statement_timeout_ms = 10000
```

## Profile fields

| Field | Meaning |
| --- | --- |
| `url` | A full `postgres://` URL, without the password. Or use the fields below. |
| `host`, `port`, `dbname`, `user` | Where to connect and who as. |
| `sslmode` | `disable`, `require`, `verify-ca` or `verify-full`. Hosted databases usually need `require`. |
| `sslrootcert`, `sslcert`, `sslkey` | Certificate files, for `verify-full` or client-certificate login. |
| `password_command` | A command that prints the password. |
| `password_file` | A file containing only the password. |
| `access_mode` | `read` (default), `write` or `admin`. See [Access modes](/docs/safety). |
| `schemas` | Only these schemas are visible. |
| `deny_schemas` | These schemas are never visible. |
| `deny_tables` | Tables to hide and refuse, e.g. `auth.*` or `public.api_keys`. |
| `pii_columns` | Extra columns to mask, as `schema.table.column`. See [Masking](/docs/privacy). |
| `personal_data_heuristics` | `false` shows emails, names, phones and so on unmasked. Secrets stay masked. |
| `sensitive_name_heuristics` | `false` also stops masking passwords, tokens and other secrets by name. |
| `max_rows` | Most rows a query may return (default 500, up to 10,000). |
| `statement_timeout_ms` | Time limit per query, in milliseconds (default 30,000). |

A project's `.nexql/config.toml` picks a `default_profile` and can only **tighten** it: a lower `max_rows`, fewer schemas, more denied tables.

## Environment variables

| Variable | Same as |
| --- | --- |
| `DATABASE_URL` / `POSTGRES_URL` | The connection, when no profile or URL is given. |
| `PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`, `PGPASSWORD`, `PGSSLMODE` | The standard Postgres variables, used the same way. |
| `NEXQL_MCP_ACCESS_MODE` | `--access-mode` |
| `NEXQL_MCP_ALLOW_WRITES` | `--allow-writes` |
| `NEXQL_MCP_SCHEMAS` | `--schemas` |
| `NEXQL_MCP_TOOLS` | `--tools` |
| `NEXQL_MCP_EMBEDDINGS` | `--embeddings` (`off` or `local`) |
| `NEXQL_MCP_CONFIG` | `--config` |
| `NEXQL_MCP_WORKSPACE_ROOT` | `--workspace-root` |
| `NEXQL_MCP_INDEX_DIR` | Where the schema index is kept. |
| `NEXQL_MCP_INDEX_DEPTH` | Default `index build --depth`. |
| `NEXQL_MCP_MAX_TIMEOUT_MS` | Ceiling for a write call's raised `timeout_ms` (default 600,000). |
| `NEXQL_MCP_OPEN_BROWSER` | `0` stops `add_database` opening its page by itself; you get the link instead. |
| `NEXQL_MCP_LOG` | Log file path. |
| `NEXQL_MCP_MANAGED` | Read-only "managed" mode without the profile-editing tools, for apps that manage connections themselves. |
| `NEXQL_MCP_HTTP_TOKEN`, `NEXQL_MCP_HTTP_RATE_LIMIT` | See below. |

## Running over HTTP

By default your app starts nexql-mcp and talks to it directly (stdio). To run it as a shared service instead:

```bash
NEXQL_MCP_HTTP_TOKEN=a-long-random-string npx -y nexql-mcp@latest --profile prod --http
```

- It listens on `127.0.0.1:8899` and serves MCP at `/mcp`. Change with `--bind` and `--http-port`.
- Clients send the token as `Authorization: Bearer <token>`.
- It refuses to listen on anything but loopback without a token.
- Each token (or each address, without one) may make 600 requests a minute. `--http-rate-limit 0` turns the limit off.

Over HTTP on another machine, `add_database` can't show you a page, so it replies with a `nexql-mcp profile add …` command to run on that machine instead.
