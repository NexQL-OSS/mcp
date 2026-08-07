// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Secret indirection — `password_command` and related.

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
        .map_err(|e| ConnError::PasswordCommand(format!("keyring store failed: {e}")))
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
        // trim happens in ProcessCommandRunner; Fake returns as-is — exercise Process:
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
