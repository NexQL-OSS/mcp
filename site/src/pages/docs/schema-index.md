---
layout: ../../layouts/DocsLayout.astro
slug: schema-index
eyebrow: Guide · 05
title: The schema index
description: How NexQL knows your tables before it writes any SQL, and how to keep that knowledge fresh.
---

The most common way an AI gets SQL wrong is by inventing tables and columns that look plausible but don't exist. NexQL's answer is a **schema index**: a local, searchable map of your database that the assistant checks before it writes a query.

## What's in it

- Every table, view, materialized view and function the profile is allowed to see, with columns, types, primary keys, comments and row estimates.
- A **join graph**: the declared foreign keys, plus inferred ones where a column like `customer_id` matches a primary key but no constraint was declared.
- Enum values, and with a deeper build, a few sample values per column so the assistant can match your wording to real data. Columns listed in `pii_columns` are never sampled.

It's stored per connection and database under `~/.local/share/nexql-mcp` (or `NEXQL_MCP_INDEX_DIR`). It holds the structure, plus at most a few sample values per column at the deeper levels, never whole tables.

## When it's built

- **Automatically**, the first time nexql-mcp meets a database with no index.
- **On first search** for other databases, when [`search_all_databases`](/docs/tools/search-all-databases) reaches them.
- **On request**, with [`rebuild_index`](/docs/tools/rebuild-index) / [`refresh_index`](/docs/tools/refresh-index), or from a terminal:

```bash
npx -y nexql-mcp@latest index build
npx -y nexql-mcp@latest index status
```

Only one build per database runs at a time. A second request waits for the first.

## How search ranks results

[`search_schema`](/docs/tools/search-schema) scores objects by how well their names, columns and comments match your words (TF-IDF over the index).

Turn on **local embeddings** and it also ranks by meaning, then merges the two rankings (reciprocal rank fusion). That's how "money customers still owe us" can find `invoices` with no word in common:

```bash
npx -y nexql-mcp@latest --embeddings local
```

The embedding model (MiniLM) runs on your computer. Nothing is sent to an embeddings API.

## Keeping it fresh

The index records a **fingerprint** of the schema. [`get_index_status`](/docs/tools/get-index-status) compares it with the live database and says whether they've drifted.

- After a change made through [`apply_ddl`](/docs/tools/apply-ddl), the index is marked stale so the assistant knows to refresh it.
- After changes made elsewhere (migrations, other tools), ask *"refresh the schema index"* or run `nexql-mcp index refresh`.

## Depth

How much the index reads is its **depth**:

| From | Options | Default |
| --- | --- | --- |
| The automatic first build | structure only | |
| [`rebuild_index`](/docs/tools/rebuild-index) | `shallow` (structure) or `full` (structure and sample values) | `full` |
| `nexql-mcp index build --depth` | `structure`, `stats`, or `profiles` (adds sample values) | `structure` |

Sample values help the assistant match your wording to real data, but take longer to collect on big tables. `NEXQL_MCP_INDEX_DEPTH` sets the command-line default.
