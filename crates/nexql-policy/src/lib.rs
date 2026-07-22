//! Safety policy — access modes, schema/table filters, row caps, audit.

pub mod access;
pub mod error;

pub use access::AccessMode;
pub use error::PolicyError;
