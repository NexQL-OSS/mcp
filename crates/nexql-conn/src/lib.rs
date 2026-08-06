//! Connection resolution and pooling.
//!
//! Precedence (highest first): CLI arg → profile → flags → DATABASE_URL → PG* env →
//! default_profile → ~/.pgpass → --env-file (opt-in).

pub mod config;
pub mod error;
pub mod pgpass;
pub mod pool;
pub mod resolve;
pub mod secret;
pub mod tls;

pub use config::{
    ConfigFile, ProfileConfig, ProjectConfigFile, find_project_config, load_project_config,
    write_with_backup,
};
pub use error::ConnError;
pub use pool::{
    ConnectionReport, PoolOptions, apply_session_guards, checkout_guarded, connect_once,
    create_pool, test_connection,
};
pub use resolve::{
    ConnectionParams, ConnectionSource, DbEngine, ResolveInputs, ResolvedConnection,
    params_from_url, resolve, resolve_all, resolve_profile,
};
pub use secret::{CommandRunner, ProcessCommandRunner};
