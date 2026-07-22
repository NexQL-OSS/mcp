//! Connection resolution and pooling.
//!
//! Precedence (highest first): CLI arg → profile → flags → DATABASE_URL → PG* env →
//! default_profile → ~/.pgpass → --env-file (opt-in).

pub mod error;
pub mod resolve;

pub use error::ConnError;
pub use resolve::ConnectionSource;
