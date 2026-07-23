//! Safety policy — access modes, schema/table filters, row caps, SQL validation.

pub mod access;
pub mod caps;
pub mod error;
pub mod filter;
pub mod sql;

pub use access::{AccessMode, check_superuser_guard};
pub use caps::{DEFAULT_MAX_RESULT_CHARS, DEFAULT_MAX_ROWS, PolicyCaps, clamp_max_rows};
pub use error::PolicyError;
pub use filter::{ObjectRef, PolicyFilter, is_pii_column};
pub use sql::{SqlDecision, validate_readonly_sql, validate_write_sql};
