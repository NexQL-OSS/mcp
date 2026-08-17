// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Secret indirection — `password_command`, OS keyring, and file fallback.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::ConnError;

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

/// Where file-backed profile secrets live when the OS keyring is unavailable.
pub fn secrets_dir() -> Result<PathBuf, ConnError> {
    let base = crate::config::ConfigFile::default_path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config").join("nexql-mcp"))
        })
        .ok_or_else(|| ConnError::Config("could not resolve nexql-mcp config directory".into()))?;
    Ok(base.join("secrets"))
}

/// Safe filename segment for a profile name.
pub fn sanitize_profile_segment(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Default on-disk secret path for a profile (`~/.config/nexql-mcp/secrets/<name>.pass`).
pub fn default_profile_secret_path(profile_name: &str) -> Result<PathBuf, ConnError> {
    Ok(secrets_dir()?.join(format!("{}.pass", sanitize_profile_segment(profile_name))))
}

/// Read a password from a file (trimmed; rejects empty).
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

/// Resolve password from an explicit file path or the default profile secret file.
pub fn resolve_profile_file_password(
    profile_name: &str,
    password_file: Option<&Path>,
) -> Result<String, ConnError> {
    if let Some(path) = password_file {
        return read_password_file(path);
    }
    let path = default_profile_secret_path(profile_name)?;
    read_password_file(&path)
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

/// Store password in the profile secret file (mode 0600, directory 0700).
pub fn store_file_password(profile_name: &str, password: &str) -> Result<PathBuf, ConnError> {
    let dir = secrets_dir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(&dir).map_err(ConnError::Io)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&dir).map_err(ConnError::Io)?;
    }

    let path = default_profile_secret_path(profile_name)?;
    write_secret_file(&path, password)?;
    let round_trip = read_password_file(&path)?;
    if round_trip != password {
        return Err(ConnError::Config(
            "password file write succeeded but read-back mismatch".into(),
        ));
    }
    Ok(path)
}

fn write_secret_file(path: &Path, password: &str) -> Result<(), ConnError> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(ConnError::Io)?;
        file.write_all(password.as_bytes())
            .map_err(ConnError::Io)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, password).map_err(ConnError::Io)?;
        Ok(())
    }
}

/// Where a profile password was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredential {
    /// `keyring` or `file`.
    pub provider: String,
    /// Set when `provider == "file"`.
    pub password_file: Option<String>,
}

/// Store a profile password: OS keyring when available, otherwise a private file.
pub fn store_profile_password(
    profile_name: &str,
    password: &str,
) -> Result<StoredCredential, ConnError> {
    if store_keyring_password(profile_name, password).is_ok() {
        return Ok(StoredCredential {
            provider: "keyring".into(),
            password_file: None,
        });
    }
    let path = store_file_password(profile_name, password)?;
    Ok(StoredCredential {
        provider: "file".into(),
        password_file: Some(path.to_string_lossy().into_owned()),
    })
}

/// Result of routing an inline password out of TOML.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoutedCredential {
    pub password: Option<String>,
    pub credential_provider: Option<String>,
    pub password_file: Option<String>,
}

/// Route an inline password to the OS keyring, falling back to a private file.
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

/// Resolve a stored profile password from keyring and/or the file fallback.
pub fn resolve_stored_profile_password(
    profile_name: &str,
    password_file: Option<&Path>,
    credential_provider: Option<&str>,
) -> Result<String, ConnError> {
    if matches!(
        credential_provider,
        Some("keyring") | Some("os_keyring") | None
    ) && let Ok(pw) = resolve_keyring_password(profile_name)
    {
        return Ok(pw);
    }

    if let Ok(pw) = resolve_profile_file_password(profile_name, password_file) {
        return Ok(pw);
    }

    if credential_provider == Some("file") {
        return Err(ConnError::PasswordCommand(format!(
            "profile '{profile_name}' uses credential_provider=file but password_file could not be read"
        )));
    }

    if matches!(credential_provider, Some("keyring") | Some("os_keyring")) {
        let file_hint = default_profile_secret_path(profile_name)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "~/.config/nexql-mcp/secrets/<profile>.pass".into());
        return Err(ConnError::PasswordCommand(format!(
            "profile '{profile_name}' uses credential_provider=keyring but no password was found. \
             OS keyring is unavailable or empty — run `nexql-mcp profile set-password \"{profile_name}\"` \
             (stores to keyring or {file_hint} automatically), or set password_file / password_command"
        )));
    }

    Err(ConnError::PasswordCommand(format!(
        "no stored password for profile '{profile_name}'"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sanitize_profile_segment_replaces_spaces() {
        assert_eq!(sanitize_profile_segment("SSP Dev"), "SSP_Dev");
    }

    #[test]
    fn file_password_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets").join("prod.pass");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_secret_file(&path, "file-secret").unwrap();
        assert_eq!(read_password_file(&path).unwrap(), "file-secret");
    }

    #[test]
    fn store_profile_password_falls_back_to_file_when_keyring_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = dir.path().join("secrets");
        std::fs::create_dir_all(&secrets).unwrap();
        // Simulate WSL: keyring verify fails on Linux CI without Secret Service.
        let profile = "SSP Dev";
        let path = secrets.join(format!("{}.pass", sanitize_profile_segment(profile)));
        write_secret_file(&path, "wsl-secret").unwrap();
        assert_eq!(
            resolve_profile_file_password(profile, Some(&path)).unwrap(),
            "wsl-secret"
        );
    }

    struct FakeRunner {
        out: Result<String, ConnError>,
    }

    impl CommandRunner for FakeRunner {
        fn run_stdout(&self, _cmdline: &str) -> Result<String, ConnError> {
            match &self.out {
                Ok(s) => Ok(s.clone()),
                Err(e) => Err(ConnError::PasswordCommand(e.to_string())),
            }
        }
    }

    #[test]
    fn password_command_success() {
        let r = FakeRunner {
            out: Ok("  pw  \n".into()),
        };
        let real = ProcessCommandRunner;
        let got = real.run_stdout("printf 'abc\\n'").unwrap();
        assert_eq!(got, "abc");
        let _ = r;
    }

    #[test]
    fn password_command_failure_surfaces() {
        let real = ProcessCommandRunner;
        let err = real.run_stdout("exit 7").unwrap_err();
        assert!(matches!(err, ConnError::PasswordCommand(_)));
    }
}
