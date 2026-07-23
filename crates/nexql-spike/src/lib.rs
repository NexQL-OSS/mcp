//! Phase 0 spike — not shipped. Validates tokio-postgres + candle MiniLM ergonomics.
//!
//! Delete or keep behind `default-members` exclusion once Phase 1 lands.

pub mod catalog;
pub mod cosine;
pub mod embed;
pub mod pg;

pub use catalog::{COLUMNS_QUERY, CONSTRAINTS_QUERY, RELATIONS_QUERY};
pub use cosine::{cosine_similarity, top_k};
pub use embed::{EmbeddingModel, MODEL_DIM, MODEL_ID};
pub use pg::{connect, connect_url, seed_users_orders};
