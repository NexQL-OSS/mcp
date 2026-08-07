// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! `~/.pgpass` password lookup (libpq format).

use std::fs;
use std::path::Path;

use crate::error::ConnError;

/// Match host/port/db/user against a pgpass file; return password if found.
pub fn lookup_password(
    path: &Path,
    host: &str,
    port: u16,
    dbname: &str,
    user: &str,
) -> Result<Option<String>, ConnError> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(|e| ConnError::PgPass(e.to_string()))?;
    Ok(lookup_password_str(&raw, host, port, dbname, user))
}

pub fn lookup_password_str(
    contents: &str,
    host: &str,
    port: u16,
    dbname: &str,
    user: &str,
) -> Option<String> {
    let port_s = port.to_string();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // hostname:port:database:username:password — password may contain ':'
        let parts: Vec<&str> = line.splitn(5, ':').collect();
        if parts.len() != 5 {
            continue;
        }
        if !field_match(parts[0], host) {
            continue;
        }
        if !field_match(parts[1], &port_s) {
            continue;
        }
        if !field_match(parts[2], dbname) {
            continue;
        }
        if !field_match(parts[3], user) {
            continue;
        }
        return Some(unescape_pgpass(parts[4]));
    }
    None
}

fn field_match(pattern: &str, value: &str) -> bool {
    pattern == "*" || pattern == value
}

fn unescape_pgpass(s: &str) -> String {
    // libpq: backslash escapes \: and \\
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_and_wildcards() {
        let file = "localhost:5432:appdb:dev:s3cret\n\
                    *:5432:app:*:fallback\n";
        assert_eq!(
            lookup_password_str(file, "localhost", 5432, "appdb", "dev").as_deref(),
            Some("s3cret")
        );
        assert_eq!(
            lookup_password_str(file, "other", 5432, "app", "x").as_deref(),
            Some("fallback")
        );
        assert_eq!(
            lookup_password_str(file, "localhost", 5432, "other", "dev"),
            None
        );
    }

    #[test]
    fn unescapes_colon_in_password() {
        let file = "h:1:d:u:p\\:ass\n";
        assert_eq!(
            lookup_password_str(file, "h", 1, "d", "u").as_deref(),
            Some("p:ass")
        );
    }
}
