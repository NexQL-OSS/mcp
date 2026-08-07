// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Manifest format version and migration.
//!
//! Port of `pro/src/features/dbindex/indexFormat.ts`.
//! `formatVersion = 1` has no migration path yet.

use crate::error::IndexError;
use crate::model::IndexManifest;

/// Current on-disk format version — must match TS `CURRENT_FORMAT_VERSION`.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

/// Parse and migrate a manifest JSON string to [`CURRENT_FORMAT_VERSION`].
///
/// Version 1 is the only supported format; older versions have no migration
/// path (triggers rebuild in the TS extension). Newer versions are rejected.
pub fn migrate_manifest(raw_json: &str) -> Result<IndexManifest, IndexError> {
    let data: serde_json::Value = serde_json::from_str(raw_json)?;
    if !data.is_object() {
        return Err(IndexError::InvalidManifest(
            "Invalid manifest file: not an object".into(),
        ));
    }

    let version = data
        .get("formatVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IndexError::InvalidManifest("Invalid manifest file: missing formatVersion".into())
        })? as u32;

    if version > CURRENT_FORMAT_VERSION {
        return Err(IndexError::NeedsRebuild {
            reason: format!(
                "manifest format version {version} is newer than current {CURRENT_FORMAT_VERSION}"
            ),
        });
    }

    if version < CURRENT_FORMAT_VERSION {
        return Err(IndexError::NeedsRebuild {
            reason: format!(
                "no migration path from format version {version} to {CURRENT_FORMAT_VERSION}"
            ),
        });
    }

    serde_json::from_value(data).map_err(IndexError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1_manifest_json() -> String {
        serde_json::json!({
            "formatVersion": 1,
            "connectionId": "c1",
            "database": "db",
            "indexedAt": "2026-01-01T00:00:00.000Z",
            "buildMode": "guided",
            "buildDepth": "stats",
            "schemaFingerprint": "1|2|3|4|5",
            "pgVersion": "15.0",
            "environment": "production",
            "scope": {
                "includedSchemas": ["public"],
                "excludedObjects": [],
                "piiExcludedColumns": []
            },
            "counts": { "tables": 0, "views": 0, "functions": 0, "enums": 0 },
            "shards": [],
            "derived": { "tokens": "tokens.json", "joinGraph": "joinGraph.json" },
            "stats": { "buildMs": 1, "queriesRun": 1, "warnings": [] }
        })
        .to_string()
    }

    #[test]
    fn migrate_v1_is_identity() {
        let manifest = migrate_manifest(&v1_manifest_json()).expect("v1 ok");
        assert_eq!(manifest.format_version, CURRENT_FORMAT_VERSION);
        assert_eq!(manifest.connection_id, "c1");
        assert_eq!(manifest.build_mode, crate::model::BuildMode::Guided);
        assert_eq!(manifest.build_depth, crate::model::BuildDepth::Stats);
    }

    #[test]
    fn reject_newer_format_version() {
        let mut v: serde_json::Value = serde_json::from_str(&v1_manifest_json()).unwrap();
        v["formatVersion"] = serde_json::json!(99);
        let err = migrate_manifest(&v.to_string()).unwrap_err();
        assert!(matches!(err, IndexError::NeedsRebuild { .. }));
    }

    #[test]
    fn reject_missing_format_version() {
        let err = migrate_manifest(r#"{"connectionId":"x"}"#).unwrap_err();
        assert!(matches!(err, IndexError::InvalidManifest(_)));
    }

    #[test]
    fn no_migration_path_from_v0() {
        let mut v: serde_json::Value = serde_json::from_str(&v1_manifest_json()).unwrap();
        v["formatVersion"] = serde_json::json!(0);
        let err = migrate_manifest(&v.to_string()).unwrap_err();
        assert!(matches!(err, IndexError::NeedsRebuild { .. }));
    }
}
