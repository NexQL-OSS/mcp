# nexql-index

Offline schema index builder, query engine, lexical search, join graph builder, and vector embedder for `nexql-mcp`.

## Features

- **Schema Shards**: Sharded JSON storage of Postgres schema metadata with format migration support.
- **Lexical Search & RRF Fusion**: BM25/TF-IDF token index fused with vector embeddings via Reciprocal Rank Fusion.
- **Join Graph**: Automatically constructs join paths across primary keys, foreign keys, and inferred relationships.
- **Re-embedding Optimization**: Reuses existing object embeddings based on `object_hash` comparison to minimize rebuild time.
