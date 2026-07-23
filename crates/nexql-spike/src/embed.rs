//! Local MiniLM embeddings via candle — same model as TS `localEmbedder.ts`.

use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use hf_hub::api::sync::Api;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer};

/// Matches Pro's `Xenova/all-MiniLM-L6-v2` / sentence-transformers equivalent.
pub const MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const MODEL_DIM: usize = 384;

static MODEL: OnceLock<Result<EmbeddingModel, String>> = OnceLock::new();

pub struct EmbeddingModel {
    tokenizer: Tokenizer,
    model: BertModel,
}

impl EmbeddingModel {
    /// Load (or return cached) MiniLM from the Hugging Face hub.
    pub fn load() -> Result<&'static EmbeddingModel> {
        let slot = MODEL.get_or_init(|| {
            Self::load_fresh().map_err(|e| format!("failed to load {MODEL_ID}: {e:#}"))
        });
        match slot {
            Ok(m) => Ok(m),
            Err(e) => bail!("{e}"),
        }
    }

    fn load_fresh() -> Result<Self> {
        let api = Api::new().context("hf-hub Api::new")?;
        let repo = api.model(MODEL_ID.to_string());
        let tokenizer_path: PathBuf = repo.get("tokenizer.json")?;
        let config_path: PathBuf = repo.get("config.json")?;
        // Prefer safetensors (PR #21 on the Hub has a single-file export).
        let weights_path: PathBuf = repo.get("model.safetensors").or_else(|_| {
            // Fall back via revision used by candle-examples/bert.
            let api = Api::new()?;
            let repo = api.repo(hf_hub::Repo::with_revision(
                MODEL_ID.to_string(),
                hf_hub::RepoType::Model,
                "refs/pr/21".to_string(),
            ));
            repo.get("model.safetensors")
        })?;

        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer load: {e}"))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        }));

        let device = Device::Cpu;
        let config: Config = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
        // SAFETY: mmaped safetensors — file lives for process lifetime; spike only.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)? };
        let model = BertModel::load(vb, &config)?;
        Ok(Self { tokenizer, model })
    }

    /// Mean-pool + L2-normalize a single sentence into a `MODEL_DIM` vector.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;
        let token_ids = Tensor::new(vec![encoding.get_ids().to_vec()], &self.model.device)?;
        let token_type_ids = token_ids.zeros_like()?;
        let embeddings = self.model.forward(&token_ids, &token_type_ids, None)?;
        // Mean pool over sequence length.
        let embeddings = (&embeddings.sum(1)? / (embeddings.dim(1)? as f64))?;
        let embeddings = embeddings.broadcast_div(&embeddings.sqr()?.sum_keepdim(1)?.sqrt()?)?;
        let vec = embeddings.squeeze(0)?.to_vec1::<f32>()?;
        if vec.len() != MODEL_DIM {
            bail!("expected dim {MODEL_DIM}, got {}", vec.len());
        }
        if vec.iter().any(|x| !x.is_finite()) {
            bail!("embedding contains NaN/Inf");
        }
        Ok(vec)
    }

    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosine::top_k;

    #[test]
    fn embed_fixed_string_has_expected_dim_and_finite() {
        let model = EmbeddingModel::load().expect("model load");
        let v = model.embed("public.users.email").expect("embed");
        assert_eq!(v.len(), MODEL_DIM);
        assert!(v.iter().all(|x| x.is_finite()));
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "expected ~unit norm, got {norm}");
    }

    #[test]
    fn cosine_search_ranks_user_email_in_top3() {
        let model = EmbeddingModel::load().expect("model load");
        // 100 object-like strings; include the target plus distractors.
        let mut labels: Vec<String> = Vec::with_capacity(100);
        labels.push("public.users.email".into());
        labels.push("public.users.id".into());
        labels.push("public.orders.user_id".into());
        for i in 0..97 {
            labels.push(format!("public.misc.col_{i}"));
        }
        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let corpus = model.embed_batch(&refs).expect("batch embed");
        assert_eq!(corpus.len(), 100);
        assert!(corpus.iter().all(|v| v.len() == MODEL_DIM));
        assert!(corpus.iter().flatten().all(|x| x.is_finite()));

        let query = model.embed("user email").expect("query embed");
        let ranked = top_k(&query, &corpus, 3);
        let top_labels: Vec<&str> = ranked.iter().map(|(i, _)| refs[*i]).collect();
        assert!(
            top_labels.contains(&"public.users.email"),
            "expected public.users.email in top 3, got {top_labels:?}"
        );
    }
}
