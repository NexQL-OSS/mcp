# nexql-policy

Access modes, SQL AST validation, schema filtering, PII column protection, and audit safety for `nexql-mcp`.

## Features

- **Access Modes**: `read` (default), `write`, and `admin` modes.
- **`pg_query` AST Validator**: Full Postgres AST validation enforcing single-statement safety and mode boundaries without fragile string-prefix checks.
- **Policy Filters**: Schema allow/deny lists, table deny globs, and automatic PII column masking.
