// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Secret indirection — `password_command`, OS keyring, encrypted file fallback,
//! and user-managed `password_file`.

use std::path::Path;
use std::process::Command;

use crate::error::ConnError;
use crate::secret_encrypted::{
    resolve_encrypted_profile_password, store_encrypted_profile_password,
};

/// Runs an external password command. Injectable for tests.
pub trait CommandRunner: Send + Sync {
    fn run_stdout(&self, cmdline: &str) -> Result<String, ConnError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessCommandRunner;

impl CommandRunner for ProcessCommandRunner {
    fn run_stdout(&self, cmdline: &str) -> Result<String, ConnError> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmdline)
            .output()
            .map_err(|e| ConnError::PasswordCommand(e.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnError::PasswordCommand(format!(
                "exit {}: {stderr}",
                output.status
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return Err(ConnError::EmptyPasswordCommand);
        }
        Ok(trimmed.to_string())
    }
}

/// Expand `${env:VAR}` placeholders in a config string.
pub fn interpolate_env(input: &str, getenv: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if input[i..].starts_with("${env:")
            && let Some(end) = input[i + 6..].find('}')
        {
            let key = &input[i + 6..i + 6 + end];
            if let Some(val) = getenv(key) {
                out.push_str(&val);
            }
            i = i + 6 + end + 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Read a password from a user-managed file (trimmed; rejects empty).
pub fn read_password_file(path: &Path) -> Result<String, ConnError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| ConnError::Config(format!("password_file {}: {e}", path.display())))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ConnError::Config(format!(
            "password_file {} is empty",
            path.display()
        )));
    }
    Ok(trimmed.to_string())
}

/// Resolve password from an explicit `password_file` path in profile config.
pub fn resolve_profile_file_password(password_file: &Path) -> Result<String, ConnError> {
    read_password_file(password_file)
}

/// Resolve password from OS keyring (`nexql-mcp` service).
pub fn resolve_keyring_password(profile_name: &str) -> Result<String, ConnError> {
    let entry = keyring::Entry::new("nexql-mcp", profile_name)
        .map_err(|e| ConnError::PasswordCommand(format!("keyring init error: {e}")))?;
    entry
        .get_password()
        .map_err(|e| ConnError::PasswordCommand(format!("keyring lookup failed: {e}")))
}

/// Store password into OS keyring (`nexql-mcp` service).
pub fn store_keyring_password(profile_name: &str, password: &str) -> Result<(), ConnError> {
    let entry = keyring::Entry::new("nexql-mcp", profile_name)
        .map_err(|e| ConnError::PasswordCommand(format!("keyring init error: {e}")))?;
    entry
        .set_password(password)
        .map_err(|e| ConnError::PasswordCommand(format!("keyring store failed: {e}")))?;
    let round_trip = entry.get_password().map_err(|e| {
        ConnError::PasswordCommand(format!("keyring verify after store failed: {e}"))
    })?;
    if round_trip != password {
        return Err(ConnError::PasswordCommand(
            "keyring store succeeded but read-back mismatch — Secret Service may be unavailable"
                .into(),
        ));
    }
    Ok(())
}

pub const ENCRYPTED_FILE_PROVIDER: &str = "encrypted_file";

const KEYRING_UNAVAILABLE_HINT: &str = "Install and start a Secret Service (e.g. gnome-keyring on Linux/WSL), \
or configure `password_command` / an explicit `password_file` path you manage in config.toml.";

const ENCRYPTED_FILE_WARNING: &str = "OS keyring unavailable — password stored in an encrypted local file \
(not plaintext). Weaker than the OS keyring; prefer gnome-keyring when possible.";

/// User-facing warning when the encrypted file fallback was used.
pub fn encrypted_file_storage_warning() -> &'static str {
    ENCRYPTED_FILE_WARNING
}

/// Where a profile password was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredential {
    pub provider: String,
    /// Set when `provider` is `encrypted_file` (relative path under config dir).
    pub password_file: Option<String>,
}

/// Store a profile password in the OS keyring, falling back to an encrypted local file.
pub fn store_profile_password(
    profile_name: &str,
    password: &str,
) -> Result<StoredCredential, ConnError> {
    if let Ok(()) = store_keyring_password(profile_name, password) {
        return Ok(StoredCredential {
            provider: "keyring".into(),
            password_file: None,
        });
    }

    tracing::warn!(
        profile = %profile_name,
        "OS keyring unavailable; storing password in encrypted local file"
    );
    let rel = store_encrypted_profile_password(profile_name, password)?;
    Ok(StoredCredential {
        provider: ENCRYPTED_FILE_PROVIDER.into(),
        password_file: Some(rel),
    })
}

/// Result of routing an inline password out of TOML.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoutedCredential {
    pub password: Option<String>,
    pub credential_provider: Option<String>,
    pub password_file: Option<String>,
}

/// Route an inline password to the OS keyring or encrypted file (never plaintext TOML).
pub fn route_password_to_keyring(
    profile_name: &str,
    password: Option<&str>,
) -> Result<RoutedCredential, ConnError> {
    let Some(password) = password else {
        return Ok(RoutedCredential::default());
    };
    let stored = store_profile_password(profile_name, password)?;
    Ok(RoutedCredential {
        password: None,
        credential_provider: Some(stored.provider),
        password_file: stored.password_file,
    })
}

/// Resolve a stored profile password from keyring, encrypted file, or user-managed file.
pub fn resolve_stored_profile_password(
    profile_name: &str,
    password_file: Option<&Path>,
    credential_provider: Option<&str>,
) -> Result<String, ConnError> {
    match credential_provider {
        Some("file") => {
            let path = password_file.ok_or_else(|| {
                ConnError::PasswordCommand(format!(
                    "profile '{profile_name}' uses credential_provider=file but password_file is not set"
                ))
            })?;
            read_password_file(path).map_err(|e| {
                ConnError::PasswordCommand(format!(
                    "profile '{profile_name}' uses credential_provider=file but password_file could not be read: {e}"
                ))
            })
        }
        Some(ENCRYPTED_FILE_PROVIDER) => {
            let path = password_file.ok_or_else(|| {
                ConnError::PasswordCommand(format!(
                    "profile '{profile_name}' uses credential_provider=encrypted_file but password_file is not set"
                ))
            })?;
            resolve_encrypted_profile_password(profile_name, path)
        }
        Some("keyring") | Some("os_keyring") => resolve_keyring_password(profile_name).map_err(|e| {
            ConnError::PasswordCommand(format!(
                "profile '{profile_name}' uses credential_provider=keyring but no password was found: {e}. \
                 Run `nexql-mcp profile set-password \"{profile_name}\"` after enabling the OS keyring, \
                 or use password_command / an explicit password_file you manage. {KEYRING_UNAVAILABLE_HINT}"
            ))
        }),
        None => resolve_keyring_password(profile_name),
        Some(other) => Err(ConnError::PasswordCommand(format!(
            "unsupported credential_provider '{other}' for profile '{profile_name}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_encrypted::store_encrypted_profile_password_at;

    #[test]
    fn interpolates_env_placeholders() {
        let getenv = |k: &str| match k {
            "SECRET" => Some("hunter2".into()),
            _ => None,
        };
        assert_eq!(
            interpolate_env("pre-${env:SECRET}-post", &getenv),
            "pre-hunter2-post"
        );
        assert_eq!(interpolate_env("plain", &getenv), "plain");
    }

    #[test]
    fn explicit_password_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("managed.pass");
        std::fs::write(&path, "file-secret\n").unwrap();
        assert_eq!(
            resolve_profile_file_password(&path).unwrap(),
            "file-secret"
        );
    }

    #[test]
    fn store_profile_password_uses_keyring_or_encrypted_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("nexql-mcp");
        let rel =
            store_encrypted_profile_password_at(&config_dir, "SSP Dev", "wsl-secret").unwrap();
        let pw = crate::secret_encrypted::resolve_encrypted_profile_password_at(
            &config_dir,
            "SSP Dev",
            Path::new(&rel),
        )
        .unwrap();
        assert_eq!(pw, "wsl-secret");

        // Full store_profile_password may use keyring when available; encrypted path is covered above.
        let stored = store_profile_password("probe-only", "x").unwrap();
        assert!(
            stored.provider == "keyring" || stored.provider == ENCRYPTED_FILE_PROVIDER
        );
    }

    #[test]
    fn encrypted_provider_round_trip_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("nexql-mcp");
        let rel =
            store_encrypted_profile_password_at(&config_dir, "prod", "correct-horse").unwrap();
        let pw = crate::secret_encrypted::resolve_encrypted_profile_password_at(
            &config_dir,
            "prod",
            Path::new(&rel),
        )
        .unwrap();
        assert_eq!(pw, "correct-horse");
    }

    #[test]
    fn password_command_success() {
        let real = ProcessCommandRunner;
        let got = real.run_stdout("printf 'abc\\n'").unwrap();
        assert_eq!(got, "abc");
    }

    #[test]
    fn password_command_failure_surfaces() {
        let real = ProcessCommandRunner;
        let err = real.run_stdout("exit 7").unwrap_err();
        assert!(matches!(err, ConnError::PasswordCommand(_)));
    }
}
