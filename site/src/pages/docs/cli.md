---
layout: ../../layouts/DocsLayout.astro
slug: cli
eyebrow: Guide · 08
title: Command line
description: Every nexql-mcp command, for setting things up, checking them, and working with your database from a terminal.
---

Most people never need a terminal: Claude Desktop installs from one file, and other apps run nexql-mcp for you. These commands are for setting up, checking, and the occasional quick query.

Commands are shown with `npx -y nexql-mcp@latest`, which needs only Node.js. If you installed nexql-mcp another way, type `nexql-mcp` instead.

A connection comes **before** the command: a URL, or `--profile name`.

```bash
npx -y nexql-mcp@latest --profile prod doctor
npx -y nexql-mcp@latest postgres://dev@localhost:5432/appdb doctor
```

## Setting up

| Command | What it does |
| --- | --- |
| `setup` | Opens a local browser page to add profiles and wire them into your apps. |
| `tui` | The same in the terminal: add, edit and test profiles, then wire up to seven apps at once. Running `nexql-mcp` with nothing to connect to opens it too. |
| `init <app>` | Adds NexQL to an app's config. Without `--apply` it prints what to paste; with `--apply` it writes the change itself and keeps a backup. |
| `profile add <name>` | Saves a profile. Leave out `--password` and you're asked for it without it showing. With `--host`, hosted databases usually need `--sslmode require` (`--sslrootcert`, `--sslcert` and `--sslkey` are there too); with `--url`, put `?sslmode=require` in the URL. |
| `profile set-password <name>` | Changes a saved password in the keyring. |

Apps `init` knows: `claude-desktop`, `claude-code`, `cursor`, `vscode`, `vscode-copilot`, `windsurf`, `zed`, `continue`, `jetbrains`, `openai-agents`. The last three are print-only.

Two lines take you from nothing to a wired app. The first asks for the password without showing it and stores it in the keyring; the second writes the app's config:

```bash
npx -y nexql-mcp@latest profile add shop --url 'postgres://reader@db.internal:5432/shop' --access-mode read
npx -y nexql-mcp@latest --profile shop init cursor --apply
```

## Checking

| Command | What it does |
| --- | --- |
| `doctor` | Connects and reports the source of the connection, server version, the role's permissions (warning on superusers), session guards and index status. |
| `profile list` | Lists saved profiles. |
| `profile test <name>` | Tests one profile's connection. |
| `--version` | Prints the version. |

## Working with the database

| Command | What it does |
| --- | --- |
| `query "<sql>"` | Runs one read-only query and prints a table. `-f json` or `-f csv` for other formats. |
| `diff <from> <to>` | Compares two schemas in the same database. `--migration` prints the SQL to get from one to the other instead. |
| `index build` | Builds the schema index. `--depth structure\|stats\|profiles`. |
| `index status` | When it was built and whether the schema has changed since. |
| `index refresh` | Rebuilds it with the last build's settings. |
| `index clear` | Deletes it. It's rebuilt next time it's needed. |

```bash
npx -y nexql-mcp@latest --profile shop query "SELECT status, count(*) FROM orders GROUP BY 1" -f csv
npx -y nexql-mcp@latest --profile shop diff public staging --migration
```

## Profiles and teams

| Command | What it does |
| --- | --- |
| `profile export` | Prints your profiles as TOML with every secret removed, for a project's `.nexql/config.toml`. |
| `profile import <path>` | Imports profiles from such a file. |
| `profile readonly-role` | Prints SQL for a least-privilege read-only role. |
| `profile migrate-secrets` | Moves any plain-text passwords in your config into the keyring. This also happens automatically. |

## Server options

These matter when an app starts nexql-mcp. They go in the app's config, in the `args` list.

| Option | Meaning |
| --- | --- |
| `--profile <name>` | Connect with a saved profile. |
| `--access-mode read\|write\|admin` | See [Access modes](/docs/safety). `--allow-writes` is the same as `write`. |
| `--tools query\|dba\|meta\|full` | See [Tool sets](/docs/tool-sets). |
| `--schemas a,b` | Only show these schemas, for profiles that don't set `schemas`. |
| `--max-rows <n>` | Row cap for this run. |
| `--embeddings local` | Rank schema search by meaning too. See [The schema index](/docs/schema-index). |
| `--i-know-what-im-doing` | Allow write or admin mode on a superuser connection. Avoid. |
| `--http` | Serve over HTTP instead of stdio. See [Configuration](/docs/configuration#running-over-http). |
