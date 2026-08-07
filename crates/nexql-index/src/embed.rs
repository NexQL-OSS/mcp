// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Embedding surface for schema search + index build.
//!
//! The [`Embedder`] trait is always available so tests can inject fakes without
//! candle. The MiniLM implementation lives behind the `embeddings` Cargo feature.

use crate::error::IndexError;
use crate::model::{DbObjectKind, ObjectEntry};

/// Stored / queried local model id — matches TS `Xenova/all-MiniLM-L6-v2`.
pub const LOCAL_MODEL_ID: &str = "Xenova/all-MiniLM-L6-v2";

/// Hugging Face repo used to download weights (same as Phase 0 spike).
pub const HF_MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";

/// MiniLM embedding dimension.
pub const MODEL_DIM: usize = 384;

/// Pluggable text → vector embedder (object-safe for test injection).
pub trait Embedder: Send + Sync {
    /// Model id written into `embeddings-meta.json` / used for query cache keys.
    fn model_id(&self) -> &str;

    /// Output dimension (e.g. 384 for MiniLM).
    fn dim(&self) -> usize;

    /// Embed a single text; implementations L2-normalize when appropriate.
    fn embed(&self, text: &str) -> Result<Vec<f32>, IndexError>;

    /// Batch embed (default: sequential `embed`).
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, IndexError> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

/// Build the document string embedded for an object — matches TS `buildObjectDoc`.
pub fn build_object_doc(ref_: &str, entry: &ObjectEntry) -> String {
    let mut doc = format!("{}: {ref_}", entry.kind.as_str().to_uppercase());
    if let Some(comment) = entry.comment.as_deref() {
        doc.push_str("\nDescription: ");
        doc.push_str(comment);
    }
    let col_list: Vec<String> = entry
        .columns
        .iter()
        .map(|c| {
            let mut col_desc = format!("{} ({})", c.name, c.type_name);
            if let Some(cc) = c.comment.as_deref() {
                col_desc.push_str(" - ");
                col_desc.push_str(cc);
            }
            col_desc
        })
        .collect();
    doc.push_str("\nColumns: ");
    doc.push_str(&col_list.join(", "));
    if let Some(pk) = entry.primary_key.as_ref()
        && !pk.is_empty()
    {
        doc.push_str("\nPrimary Key: ");
        doc.push_str(&pk.join(", "));
    }
    doc
}

/// Whether this object kind is embedded (tables / views / matviews).
pub fn is_embeddable_kind(kind: DbObjectKind) -> bool {
    matches!(
        kind,
        DbObjectKind::Table | DbObjectKind::View | DbObjectKind::Matview
    )
}

/// Env / request helper: `NEXQL_MCP_EMBEDDINGS=local` (case-insensitive).
pub fn embeddings_env_local() -> bool {
    std::env::var("NEXQL_MCP_EMBEDDINGS")
        .map(|v| v.eq_ignore_ascii_case("local"))
        .unwrap_or(false)
}

#[cfg(feature = "embeddings")]
mod candle_minilm {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use candle_core::{Device, Tensor};
    use candle_nn::VarBuilder;
    use candle_transformers::models::bert::{BertModel, Config, DTYPE};
    use hf_hub::api::sync::Api;
    use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer};

    use super::{Embedder, HF_MODEL_ID, LOCAL_MODEL_ID, MODEL_DIM};
    use crate::error::IndexError;

    static MODEL: OnceLock<Result<MiniLmModel, String>> = OnceLock::new();

    struct MiniLmModel {
        tokenizer: Tokenizer,
        model: BertModel,
    }

    impl MiniLmModel {
        fn load_fresh() -> Result<Self, IndexError> {
            if std::env::var_os("NEXQL_SKIP_MODEL_DOWNLOAD").is_some() {
                return Err(IndexError::Build(
                    "NEXQL_SKIP_MODEL_DOWNLOAD set — skipping MiniLM load".into(),
                ));
            }
            let api = Api::new().map_err(|e| IndexError::Build(format!("hf-hub Api::new: {e}")))?;
            let repo = api.model(HF_MODEL_ID.to_string());
            let tokenizer_path: PathBuf = repo
                .get("tokenizer.json")
                .map_err(|e| IndexError::Build(format!("tokenizer.json: {e}")))?;
            let config_path: PathBuf = repo
                .get("config.json")
                .map_err(|e| IndexError::Build(format!("config.json: {e}")))?;
            let weights_path: PathBuf = repo.get("model.safetensors").or_else(|_| {
                let api = Api::new().map_err(|e| IndexError::Build(e.to_string()))?;
                let repo = api.repo(hf_hub::Repo::with_revision(
                    HF_MODEL_ID.to_string(),
                    hf_hub::RepoType::Model,
                    "refs/pr/21".to_string(),
                ));
                repo.get("model.safetensors")
                    .map_err(|e| IndexError::Build(format!("model.safetensors: {e}")))
            })?;

            let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| IndexError::Build(format!("tokenizer load: {e}")))?;
            tokenizer.with_padding(Some(PaddingParams {
                strategy: PaddingStrategy::BatchLongest,
                ..Default::default()
            }));

            let device = Device::Cpu;
            let config: Config = serde_json::from_str(
                &std::fs::read_to_string(&config_path)
                    .map_err(|e| IndexError::Build(format!("config read: {e}")))?,
            )
            .map_err(|e| IndexError::Build(format!("config parse: {e}")))?;
            // SAFETY: mmaped safetensors — file lives for process lifetime.
            let vb = unsafe {
                VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)
                    .map_err(|e| IndexError::Build(format!("safetensors: {e}")))?
            };
            let model = BertModel::load(vb, &config)
                .map_err(|e| IndexError::Build(format!("BertModel::load: {e}")))?;
            Ok(Self { tokenizer, model })
        }

        fn embed_inner(&self, text: &str) -> Result<Vec<f32>, IndexError> {
            let map_c = |e: candle_core::Error| IndexError::Build(e.to_string());
            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| IndexError::Build(format!("tokenize: {e}")))?;
            let token_ids = Tensor::new(vec![encoding.get_ids().to_vec()], &self.model.device)
                .map_err(map_c)?;
            let token_type_ids = token_ids.zeros_like().map_err(map_c)?;
            let embeddings = self
                .model
                .forward(&token_ids, &token_type_ids, None)
                .map_err(|e| IndexError::Build(format!("bert forward: {e}")))?;
            let seq_len = embeddings.dim(1).map_err(map_c)? as f64;
            let embeddings = (embeddings.sum(1).map_err(map_c)? / seq_len).map_err(map_c)?;
            let norm = embeddings
                .sqr()
                .map_err(map_c)?
                .sum_keepdim(1)
                .map_err(map_c)?
                .sqrt()
                .map_err(map_c)?;
            let embeddings = embeddings.broadcast_div(&norm).map_err(map_c)?;
            let vec = embeddings
                .squeeze(0)
                .map_err(map_c)?
                .to_vec1::<f32>()
                .map_err(map_c)?;
            if vec.len() != MODEL_DIM {
                return Err(IndexError::Build(format!(
                    "expected dim {MODEL_DIM}, got {}",
                    vec.len()
                )));
            }
            if vec.iter().any(|x| !x.is_finite()) {
                return Err(IndexError::Build("embedding contains NaN/Inf".into()));
            }
            Ok(vec)
        }
    }

    /// Local MiniLM embedder (candle) — feature `embeddings` only.
    pub struct MiniLmEmbedder {
        inner: &'static MiniLmModel,
    }

    impl MiniLmEmbedder {
        /// Load (or return cached) MiniLM from the Hugging Face hub.
        pub fn load() -> Result<Self, IndexError> {
            let slot = MODEL.get_or_init(|| {
                MiniLmModel::load_fresh().map_err(|e| format!("failed to load {HF_MODEL_ID}: {e}"))
            });
            match slot {
                Ok(m) => Ok(Self { inner: m }),
                Err(e) => Err(IndexError::Build(e.clone())),
            }
        }
    }

    impl Embedder for MiniLmEmbedder {
        fn model_id(&self) -> &str {
            LOCAL_MODEL_ID
        }

        fn dim(&self) -> usize {
            MODEL_DIM
        }

        fn embed(&self, text: &str) -> Result<Vec<f32>, IndexError> {
            self.inner.embed_inner(text)
        }
    }
}

#[cfg(feature = "embeddings")]
pub use candle_minilm::MiniLmEmbedder;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ColumnEntry;

    fn sample_entry() -> ObjectEntry {
        ObjectEntry {
            kind: DbObjectKind::Table,
            oid: 1,
            object_hash: "h".into(),
            comment: Some("people".into()),
            row_estimate: 1.0,
            size_bytes: 1,
            columns: vec![ColumnEntry {
                name: "email".into(),
                type_name: "text".into(),
                not_null: false,
                default_value: None,
                comment: Some("addr".into()),
                ordinal: 1,
                is_pk: None,
                profile: None,
                pii: None,
            }],
            primary_key: Some(vec!["id".into()]),
            foreign_keys: None,
            indexes: None,
            checks: None,
            excluded: None,
            definition: None,
            signature: None,
            language: None,
            volatility: None,
            body: None,
            values: None,
            base_type: None,
            constraint: None,
        }
    }

    #[test]
    fn build_object_doc_matches_ts_shape() {
        let doc = build_object_doc("public.users", &sample_entry());
        assert!(doc.starts_with("TABLE: public.users"));
        assert!(doc.contains("Description: people"));
        assert!(doc.contains("email (text) - addr"));
        assert!(doc.contains("Primary Key: id"));
    }

    #[cfg(feature = "embeddings")]
    #[test]
    fn embed_fixed_string_has_expected_dim_and_finite() {
        if std::env::var_os("NEXQL_SKIP_MODEL_DOWNLOAD").is_some() {
            eprintln!("skip: NEXQL_SKIP_MODEL_DOWNLOAD set");
            return;
        }
        let model = match MiniLmEmbedder::load() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("skip: model load failed (no network?): {e}");
                return;
            }
        };
        let v = model.embed("public.users.email").expect("embed");
        assert_eq!(v.len(), MODEL_DIM);
        assert!(v.iter().all(|x| x.is_finite()));
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "expected ~unit norm, got {norm}");
    }
}
