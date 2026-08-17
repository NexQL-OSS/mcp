// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

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
pub mod secret_encrypted;
pub mod tls;

pub use config::{
    ConfigFile, ProfileConfig, ProjectConfigFile, SecretMigrationReport,
    config_plaintext_secret_warnings, find_project_config, load_path_migrated,
    load_project_config, log_secret_migration_report, migrate_plaintext_secrets,
    prepare_profile_for_persist, profile_has_plaintext_secret, write_with_backup,
};
pub use error::{ConnError, format_postgres_error};
pub use pool::{
    ConnectionReport, PoolOptions, apply_session_guards, checkout_guarded, connect_once,
    create_pool, test_connection,
};
pub use resolve::{
    ConnectionParams, ConnectionSource, DbEngine, ResolveInputs, ResolvedConnection,
    params_from_url, resolve, resolve_all, resolve_profile,
};
pub use secret::{
    CommandRunner, ProcessCommandRunner, RoutedCredential, StoredCredential,
    encrypted_file_storage_warning, read_password_file, resolve_keyring_password,
    resolve_profile_file_password, resolve_stored_profile_password, route_password_to_keyring,
    store_keyring_password, store_profile_password, ENCRYPTED_FILE_PROVIDER,
};
pub use secret_encrypted::{
    delete_encrypted_profile_password, nexql_config_dir, resolve_encrypted_profile_password,
    secrets_dir, store_encrypted_profile_password,
};
