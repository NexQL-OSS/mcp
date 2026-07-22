//! Wire-compatible index model types.
//!
//! Field names must match `nexql-pro/pro/src/features/dbindex/types.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexManifest {
    pub format_version: u32,
    pub indexed_at: String,
    pub database_fingerprint: String,
    pub object_count: u64,
}
