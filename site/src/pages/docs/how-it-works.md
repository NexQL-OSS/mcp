---
layout: ../../layouts/DocsLayout.astro
slug: how-it-works
eyebrow: Guide · 01
title: How a question travels
description: What happens between "Which customers churned last month?" and the answer, and where each safety check sits.
---

NexQL MCP is a small program that runs **on your computer**. Your AI app (Claude Desktop, Cursor, VS Code, …) starts it in the background and talks to it using the [Model Context Protocol](https://modelcontextprotocol.io). NexQL connects to your database directly. There is no NexQL cloud service in between.

## The route, stop by stop

### 1. Your app starts nexql-mcp

When the app launches, it starts `nexql-mcp` as a local process and talks to it over standard input and output. nexql-mcp works out which database to use, from a URL, a saved profile or environment variables (see [Connections](/docs/connections)), and opens a small pool of connections.

In the default **read** mode, every connection is opened read-only by Postgres itself (`default_transaction_read_only = on`), on top of NexQL's own checks.

### 2. It learns your schema

If it has never seen this database, nexql-mcp builds a **schema index**: a compact local copy of your tables, columns, keys, joins and comments, stored under `~/.local/share/nexql-mcp`. Searches run against this index, so finding the right table costs no queries. See [The schema index](/docs/schema-index).

### 3. It tells the assistant how to behave

Along with the tool list, nexql-mcp sends the assistant short instructions. The most important ones:

- Find tables with `search_schema` (or `orient` for broad questions) and confirm columns with `describe_object` **before** writing SQL.
- If a table isn't found, it doesn't exist. Ask the user instead of guessing.
- To connect another database, use `add_database`. Never ask for a password in the chat.
- If a column comes back as `<redacted>`, tell the user rather than working around it.

### 4. You ask; the assistant picks tools

For "Which customers churned last month?" a typical sequence is:

1. [`search_schema`](/docs/tools/search-schema) with "customers churn subscriptions" → finds `public.customers` and `billing.subscriptions`.
2. [`describe_object`](/docs/tools/describe-object) on each → exact column names and types.
3. [`get_join_path`](/docs/tools/get-join-path) if the link isn't obvious.
4. [`run_select`](/docs/tools/run-select) with the finished query.

### 5. Every query is checked before it runs

The SQL is parsed with **Postgres's own parser** (`libpg_query`), not matched against text patterns. Then:

- `run_select` accepts exactly one `SELECT` or `WITH` statement. Anything else is refused.
- Functions that read server files, open network links or run SQL hidden inside a string (`pg_read_file`, `dblink`, `query_to_xml`, …) are refused in every mode.
- Tables listed in the profile's `deny_tables`, and schemas outside `schemas`, are refused.

See [Access modes and guardrails](/docs/safety) for the full list.

### 6. The database runs it, within limits

The query runs under a statement timeout (30 seconds unless the profile says otherwise) and returns at most the row cap (50 by default, up to the profile's `max_rows`).

### 7. Results are masked, then returned

Before anything goes back to the assistant, nexql-mcp traces every result column to the table column it came from and replaces personal data and secrets with `<redacted>`. Renaming a column (`email AS contact`) or wrapping it (`lower(email)`) doesn't get around this. See [Masking personal data](/docs/privacy).

Long results are cut at 20,000 characters, with a note saying so.

## What leaves your computer

nexql-mcp sends nothing anywhere except to your database. But whatever the assistant receives (table descriptions and query results) becomes part of your conversation, so **your AI provider sees it** like anything else you type. Masking is there so that personal data doesn't make that trip.

> Never paste a database password into the chat. Chats are stored by your AI provider. If you already did, change that password.
