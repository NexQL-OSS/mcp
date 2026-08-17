// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Encrypted on-disk password storage when the OS keyring is unavailable.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use sha2::{Digest, Sha256};

use crate::config::ConfigFile;
use crate::error::ConnError;

const MASTER_KEY_FILE: &str = ".master-key";
const SECRETS_SUBDIR: &str = "secrets";
const FILE_MAGIC: &[u8; 7] = b"NQENC1\0";
const NONCE_LEN: usize = 12;
const MASTER_KEY_LEN: usize = 32;

/// NexQL config directory (`~/.config/nexql-mcp` or parent of `NEXQL_MCP_CONFIG`).
pub fn nexql_config_dir() -> Result<PathBuf, ConnError> {
    ConfigFile::default_path()
        .and_then(|p| p.parent().map(|parent| parent.to_path_buf()))
        .ok_or_else(|| ConnError::Config("could not resolve nexql config directory".into()))
}

/// Directory holding per-profile encrypted password blobs.
pub fn secrets_dir() -> Result<PathBuf, ConnError> {
    let dir = nexql_config_dir()?.join(SECRETS_SUBDIR);
    fs::create_dir_all(&dir).map_err(|e| {
        ConnError::Config(format!("could not create secrets directory {}: {e}", dir.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

fn profile_secret_filename(profile_name: &str) -> String {
    let digest = Sha256::digest(profile_name.as_bytes());
    let short = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes"));
    let slug: String = profile_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() {
        "profile".to_string()
    } else {
        slug.chars().take(32).collect()
    };
    format!("{slug}-{short:016x}.enc")
}

/// Relative path (under config dir) for a profile's encrypted secret file.
pub fn secret_relative_path(profile_name: &str) -> String {
    format!("{SECRETS_SUBDIR}/{}", profile_secret_filename(profile_name))
}

fn resolve_secret_path(config_dir: &Path, password_file: &Path) -> PathBuf {
    if password_file.is_absolute() {
        password_file.to_path_buf()
    } else {
        config_dir.join(password_file)
    }
}

fn load_or_create_master_key(config_dir: &Path) -> Result<[u8; MASTER_KEY_LEN], ConnError> {
    let path = config_dir.join(MASTER_KEY_FILE);
    if path.exists() {
        let bytes = fs::read(&path).map_err(|e| {
            ConnError::Config(format!("could not read master key {}: {e}", path.display()))
        })?;
        if bytes.len() != MASTER_KEY_LEN {
            return Err(ConnError::Config(format!(
                "master key {} has invalid length (expected {MASTER_KEY_LEN} bytes)",
                path.display()
            )));
        }
        let mut key = [0u8; MASTER_KEY_LEN];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }

    fs::create_dir_all(config_dir).map_err(|e| {
        ConnError::Config(format!(
            "could not create config directory {}: {e}",
            config_dir.display()
        ))
    })?;

    let mut key = [0u8; MASTER_KEY_LEN];
    getrandom::fill(&mut key).map_err(|e| {
        ConnError::Config(format!("could not generate master key entropy: {e}"))
    })?;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            ConnError::Config(format!("could not create master key {}: {e}", path.display()))
        })?;
    file.write_all(&key).map_err(|e| {
        ConnError::Config(format!("could not write master key {}: {e}", path.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

fn encrypt_password(key: &[u8; MASTER_KEY_LEN], password: &str) -> Result<Vec<u8>, ConnError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, password.as_bytes())
        .map_err(|e| ConnError::Config(format!("password encryption failed: {e}")))?;
    let mut out = Vec::with_capacity(FILE_MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(FILE_MAGIC);
    out.extend_from_slice(&nonce);
    out.extend(ciphertext);
    Ok(out)
}

fn decrypt_password(key: &[u8; MASTER_KEY_LEN], blob: &[u8]) -> Result<String, ConnError> {
    let header_len = FILE_MAGIC.len() + NONCE_LEN;
    if blob.len() < header_len || &blob[..FILE_MAGIC.len()] != FILE_MAGIC {
        return Err(ConnError::Config(
            "encrypted password file is corrupt or not a NexQL secret".into(),
        ));
    }
    let nonce = Nonce::from_slice(&blob[FILE_MAGIC.len()..header_len]);
    let cipher = ChaCha20Poly1305::new(key.into());
    let plaintext = cipher
        .decrypt(nonce, &blob[header_len..])
        .map_err(|e| ConnError::Config(format!("password decryption failed: {e}")))?;
    let password = String::from_utf8(plaintext).map_err(|e| {
        ConnError::Config(format!("decrypted password is not valid UTF-8: {e}"))
    })?;
    if password.is_empty() {
        return Err(ConnError::Config("decrypted password is empty".into()));
    }
    Ok(password)
}

/// Store a profile password in an encrypted file under the NexQL config directory.
pub fn store_encrypted_profile_password(
    profile_name: &str,
    password: &str,
) -> Result<String, ConnError> {
    let config_dir = nexql_config_dir()?;
    store_encrypted_profile_password_at(&config_dir, profile_name, password)
}

/// Store a profile password in an encrypted file (injectable config dir for tests).
pub fn store_encrypted_profile_password_at(
    config_dir: &Path,
    profile_name: &str,
    password: &str,
) -> Result<String, ConnError> {
    let rel = secret_relative_path(profile_name);
    let abs = resolve_secret_path(config_dir, Path::new(&rel));
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            ConnError::Config(format!(
                "could not create secrets directory {}: {e}",
                parent.display()
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let key = load_or_create_master_key(config_dir)?;
    let blob = encrypt_password(&key, password)?;
    fs::write(&abs, &blob).map_err(|e| {
        ConnError::Config(format!(
            "could not write encrypted password {}: {e}",
            abs.display()
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&abs, fs::Permissions::from_mode(0o600));
    }

    let round_trip = resolve_encrypted_profile_password_at(config_dir, profile_name, Path::new(&rel))?;
    if round_trip != password {
        return Err(ConnError::Config(
            "encrypted password store succeeded but read-back mismatch".into(),
        ));
    }
    Ok(rel)
}

/// Read and decrypt a profile password from its encrypted file.
pub fn resolve_encrypted_profile_password(
    profile_name: &str,
    password_file: &Path,
) -> Result<String, ConnError> {
    let config_dir = nexql_config_dir()?;
    resolve_encrypted_profile_password_at(&config_dir, profile_name, password_file)
}

/// Read and decrypt a profile password (injectable config dir for tests).
pub fn resolve_encrypted_profile_password_at(
    config_dir: &Path,
    profile_name: &str,
    password_file: &Path,
) -> Result<String, ConnError> {
    let abs = resolve_secret_path(config_dir, password_file);
    let blob = fs::read(&abs).map_err(|e| {
        ConnError::PasswordCommand(format!(
            "profile '{profile_name}' encrypted password file {} could not be read: {e}",
            abs.display()
        ))
    })?;
    let key = load_or_create_master_key(config_dir)?;
    decrypt_password(&key, &blob)
}

/// Remove a profile's encrypted secret file if present (best-effort).
pub fn delete_encrypted_profile_password(password_file: Option<&Path>) {
    let Some(password_file) = password_file else {
        return;
    };
    if let Ok(config_dir) = nexql_config_dir() {
        let abs = resolve_secret_path(&config_dir, password_file);
        let _ = fs::remove_file(abs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_password_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("nexql-mcp");
        let rel = store_encrypted_profile_password_at(&config_dir, "Ecom DB", "wsl-secret").unwrap();
        assert!(rel.starts_with("secrets/"));
        assert_eq!(
            resolve_encrypted_profile_password_at(&config_dir, "Ecom DB", Path::new(&rel)).unwrap(),
            "wsl-secret"
        );
    }

    #[test]
    fn encrypted_password_rejects_corrupt_blob() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("nexql-mcp");
        fs::create_dir_all(config_dir.join("secrets")).unwrap();
        let rel = secret_relative_path("prod");
        let path = config_dir.join(&rel);
        fs::write(&path, b"not-a-nexql-secret").unwrap();
        load_or_create_master_key(&config_dir).unwrap();
        let err = resolve_encrypted_profile_password_at(&config_dir, "prod", Path::new(&rel)).unwrap_err();
        assert!(matches!(err, ConnError::Config(_)));
    }
}
