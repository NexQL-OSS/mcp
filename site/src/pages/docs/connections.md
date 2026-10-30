---
layout: ../../layouts/DocsLayout.astro
slug: connections
eyebrow: Guide · 04
title: Connections and passwords
description: Saved profiles, where passwords live, adding databases from the chat, and moving between them.
---

A **profile** is a saved connection: where the database is, who to log in as, and the rules for it (access mode, visible schemas, row cap). Profiles live in `~/.config/nexql-mcp/config.toml`. Passwords never do.

## Adding a database

Pick whichever fits. All four keep the password out of the chat and out of plain-text files.

### From the chat (easiest once NexQL is running)

Ask your assistant, for example:

> “Also connect our billing database: host db.internal, database billing, user billing_reader.”

It calls [`add_database`](/docs/tools/add-database) with those details. nexql-mcp opens a one-time page on your computer at `127.0.0.1`. You check the host and user, type the password, and press **Test and connect**. The connection is tested, the password goes to your OS keyring, and a **read-only** profile is saved and available straight away, with no restart.

The page only answers on your own machine, uses a secret link, accepts five attempts and expires after ten minutes. Set `NEXQL_MCP_OPEN_BROWSER=0` if you'd rather open the link yourself.

### In a terminal

Leave out the password and nexql-mcp asks for it without showing what you type:

```bash
npx -y nexql-mcp@latest profile add billing --host db.internal --dbname billing --user billing_reader --access-mode read
```

### In a browser wizard

```bash
npx -y nexql-mcp@latest setup
```

Opens a local page for adding profiles and wiring them into your apps. `nexql-mcp tui` is the same in the terminal.

### In Claude Desktop

The install form asks for a database URL. That one connection is passed in as `DATABASE_URL`. To add more, ask in the chat as above.

## Where passwords live

nexql-mcp looks for a profile's password in this order:

1. `password_command`: a command that prints it, such as `op read op://vault/pg/password` for 1Password.
2. `password_file`: a file that holds only the password.
3. Your **OS keyring** (macOS Keychain, Windows Credential Manager, Secret Service on Linux). This is where `add_database`, `profile add` and the setup wizard put it.

For a one-off connection without a profile, `PGPASSWORD` in the environment works too.

On Linux without a running keyring, nexql-mcp falls back to an encrypted file under its config folder. An old config with a plain `password = "…"` line is moved into the keyring automatically the first time it loads. To change a stored password:

```bash
npx -y nexql-mcp@latest profile set-password billing
```

## Which database it connects to

When nexql-mcp starts, the first of these that's present wins:

1. A URL on the command line: `nexql-mcp postgres://dev@localhost:5432/appdb`
2. `--profile name`
3. `--host`, `--port`, `--dbname`, `--user` flags
4. `DATABASE_URL` (or `POSTGRES_URL`)
5. `PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`
6. The `default_profile` in a project's `.nexql/config.toml`
7. The `default_profile` in your own config

`nexql-mcp doctor` prints which one it used and whether it can connect.

## Moving between databases

- [`list_connections`](/docs/tools/list-connections) shows your saved profiles; [`get_current_context`](/docs/tools/get-current-context) shows which one is active.
- [`switch_connection`](/docs/tools/switch-connection) moves the session to another profile or database.
- Most query tools also take `connectionId` and `database` for a single call, without switching.
- [`search_all_databases`](/docs/tools/search-all-databases) finds which database holds a table, and [`resolve_target`](/docs/tools/resolve-target) picks a connection from a hint like "staging".

So you can ask *"Compare last month's orders in shop with paid invoices in billing"* and the assistant reads each part from the right place.

## Sharing with a team

Commit a `.nexql/config.toml` to your project to pick a profile and tighten its rules for everyone. It holds no secrets:

```toml
default_profile = "staging"
access_mode = "read"
schemas = ["public", "billing"]
deny_tables = ["auth.*"]
max_rows = 200
```

Each person keeps the actual connection, and their own password, in their personal config. [`export_profile`](/docs/tools/export-profile) writes a shareable version of a profile with credentials stripped, and [`import_profile`](/docs/tools/import-profile) brings one in.

## Use a role that can only read

The safest setup is a database role that can't change anything, whatever the access mode says. On PostgreSQL 14 and later:

```sql
CREATE ROLE nexql_readonly WITH LOGIN PASSWORD 'replace-with-a-secret';
GRANT pg_read_all_data TO nexql_readonly;
```
