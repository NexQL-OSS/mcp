// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Minimal `completions/complete` for `ref` tool arguments from the schema index.

use nexql_index::IndexStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_SUGGESTIONS: usize = 50;

#[derive(Debug, Error)]
pub enum CompletionError {
    #[error("{0}")]
    InvalidParams(String),
    #[error("{0}")]
    Internal(String),
}

impl CompletionError {
    pub fn code(&self) -> i32 {
        match self {
            Self::InvalidParams(_) => -32602,
            Self::Internal(_) => -32603,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionValue {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResult {
    pub values: Vec<CompletionValue>,
    #[serde(rename = "total", skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(rename = "hasMore", skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

/// Suggests `schema.name` refs from indexed shards for tool `ref` arguments.
pub struct CompletionsProvider {
    store: IndexStore,
}

impl CompletionsProvider {
    pub fn new(store: IndexStore) -> Self {
        Self { store }
    }

    /// Complete when `argument_name` looks like a schema object ref.
    pub fn complete_ref(
        &self,
        connection_id: &str,
        database: &str,
        argument_name: &str,
        value_prefix: &str,
    ) -> Result<CompletionResult, CompletionError> {
        if !is_ref_argument(argument_name) {
            return Ok(CompletionResult {
                values: Vec::new(),
                total: Some(0),
                has_more: Some(false),
            });
        }

        let base = self.store.base_dir(connection_id, database);
        let Some(manifest) = self
            .store
            .read_manifest(&base)
            .map_err(|e| CompletionError::Internal(e.to_string()))?
        else {
            return Ok(CompletionResult {
                values: Vec::new(),
                total: Some(0),
                has_more: Some(false),
            });
        };

        let overrides = self
            .store
            .read_overrides(&base)
            .map_err(|e| CompletionError::Internal(e.to_string()))?;

        let prefix_lower = value_prefix.to_ascii_lowercase();
        let mut refs = Vec::new();
        for shard in &manifest.shards {
            let Some(entries) = self
                .store
                .read_shard_entries(&base, &shard.file)
                .map_err(|e| CompletionError::Internal(e.to_string()))?
            else {
                continue;
            };
            for (ref_, entry) in entries {
                if entry.excluded == Some(true) {
                    continue;
                }
                if let Some(objects) = overrides.as_ref().and_then(|o| o.objects.as_ref())
                    && objects.get(&ref_).and_then(|o| o.excluded) == Some(true)
                {
                    continue;
                }
                if !prefix_lower.is_empty() && !ref_.to_ascii_lowercase().starts_with(&prefix_lower)
                {
                    continue;
                }
                refs.push((ref_, entry.kind.as_str().to_owned()));
            }
        }
        refs.sort_by(|a, b| a.0.cmp(&b.0));
        let total = refs.len();
        let has_more = total > MAX_SUGGESTIONS;
        refs.truncate(MAX_SUGGESTIONS);

        Ok(CompletionResult {
            values: refs
                .into_iter()
                .map(|(value, kind)| CompletionValue {
                    value,
                    description: Some(kind),
                })
                .collect(),
            total: Some(total),
            has_more: Some(has_more),
        })
    }
}

fn is_ref_argument(name: &str) -> bool {
    matches!(name, "ref" | "table" | "from" | "to" | "a" | "b" | "object")
        || name.ends_with("_ref")
        || name.ends_with("Ref")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_argument_detection() {
        assert!(is_ref_argument("ref"));
        assert!(is_ref_argument("table"));
        assert!(!is_ref_argument("sql"));
        assert!(!is_ref_argument("limit"));
    }
}
