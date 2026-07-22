//! Offline-built schema index (dbindex port).
//!
//! On-disk format must stay byte-compatible with the TS extension index
//! (JSON shards + flat f32 `.bin` + manifest).
//!
//! Reference: `nexql-pro/pro/src/features/dbindex/`

pub mod error;
pub mod model;

pub use error::IndexError;
