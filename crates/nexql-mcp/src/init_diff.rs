// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! The unified diff `nexql-mcp init <client> --apply` prints.
//!
//! The output usually lands in an AI agent's transcript, and a client config
//! can hold other servers' secrets next to the `nexql-mcp` entry. Values under
//! secret-looking keys, URL passwords and bearer tokens are redacted before
//! printing. The file itself is written unredacted.

use crate::setup::wire::WireDiff;

const REDACTED: &str = "<redacted>";

/// Substrings of a JSON key whose string value is not printed.
const SECRET_KEY_PARTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "pwd",
    "key",
    "auth",
    "credential",
    "cookie",
];

/// `--- old` / `+++ new` headers plus the hunks of `diff`, redacted. A file
/// that did not exist is diffed against `/dev/null`.
pub fn render(diff: &WireDiff, existed: bool) -> String {
    if diff.unified_diff.trim().is_empty() {
        return format!("  (no changes to {})\n", diff.config_path);
    }
    let old_label = if existed {
        diff.config_path.as_str()
    } else {
        "/dev/null"
    };
    let mut out = format!("--- {old_label}\n+++ {}\n", diff.config_path);
    for line in diff.unified_diff.lines() {
        out.push_str(&redact_line(line));
        out.push('\n');
    }
    out
}

fn redact_line(line: &str) -> String {
    let line = redact_secret_value(line);
    let line = redact_url_passwords(&line);
    redact_bearer(&line)
}

/// `  "API_KEY": "abc",` → `  "API_KEY": "<redacted>",`
fn redact_secret_value(line: &str) -> String {
    let Some(key_start) = line.find('"') else {
        return line.to_owned();
    };
    let Some(key_len) = string_len(&line[key_start + 1..]) else {
        return line.to_owned();
    };
    let key_end = key_start + 1 + key_len;
    let key = line[key_start + 1..key_end].to_ascii_lowercase();
    if !SECRET_KEY_PARTS.iter().any(|part| key.contains(part)) {
        return line.to_owned();
    }
    let after_key = &line[key_end + 1..];
    let Some(colon) = after_key.find(':') else {
        return line.to_owned();
    };
    let rest = &after_key[colon + 1..];
    let Some(value_start) = rest.find('"') else {
        return line.to_owned();
    };
    let Some(value_len) = string_len(&rest[value_start + 1..]) else {
        return line.to_owned();
    };
    let value_from = key_end + 1 + colon + 1 + value_start + 1;
    format!(
        "{}{REDACTED}{}",
        &line[..value_from],
        &line[value_from + value_len..]
    )
}

/// Byte length of a JSON string body up to its closing quote.
fn string_len(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return Some(i),
            _ => {}
        }
    }
    None
}

/// `postgres://user:pass@host` → `postgres://user:<redacted>@host`
fn redact_url_passwords(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(scheme_end) = rest.find("://") {
        let authority_from = scheme_end + 3;
        out.push_str(&rest[..authority_from]);
        rest = &rest[authority_from..];
        let authority_len = rest
            .find(|c: char| c == '/' || c == '"' || c == '?' || c.is_whitespace())
            .unwrap_or(rest.len());
        let authority = &rest[..authority_len];
        match authority.rfind('@') {
            Some(at) if authority[..at].contains(':') => {
                let colon = authority[..at].find(':').expect("checked above");
                out.push_str(&authority[..=colon]);
                out.push_str(REDACTED);
                out.push_str(&authority[at..]);
            }
            _ => out.push_str(authority),
        }
        rest = &rest[authority_len..];
    }
    out.push_str(rest);
    out
}

/// `"Authorization: Bearer abc"` → `"Authorization: Bearer <redacted>"`
fn redact_bearer(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let Some(at) = lower.find("bearer ") else {
        return line.to_owned();
    };
    let token_from = at + "bearer ".len();
    let token_len = line[token_from..]
        .find(|c: char| c == '"' || c.is_whitespace())
        .unwrap_or(line.len() - token_from);
    if token_len == 0 {
        return line.to_owned();
    }
    format!(
        "{}{REDACTED}{}",
        &line[..token_from],
        &line[token_from + token_len..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use similar::TextDiff;

    fn wire_diff(old: &str, new: &str) -> WireDiff {
        WireDiff {
            client_key: "claude-code".into(),
            display_name: "Claude Code".into(),
            config_path: "/work/.mcp.json".into(),
            unified_diff: TextDiff::from_lines(old, new)
                .unified_diff()
                .context_radius(2)
                .to_string(),
            new_content: new.into(),
            valid: true,
            error: None,
        }
    }

    #[test]
    fn new_file_is_diffed_against_dev_null() {
        let out = render(&wire_diff("", "{\n  \"mcpServers\": {}\n}\n"), false);
        assert!(
            out.starts_with("--- /dev/null\n+++ /work/.mcp.json\n@@"),
            "{out}"
        );
        assert!(out.contains("+  \"mcpServers\": {}"), "{out}");
    }

    #[test]
    fn existing_file_shows_removed_and_added_lines() {
        let old = "{\n  \"a\": 1,\n  \"command\": \"nexql-mcp\"\n}\n";
        let new = "{\n  \"a\": 1,\n  \"command\": \"npx\"\n}\n";
        let out = render(&wire_diff(old, new), true);
        assert!(
            out.starts_with("--- /work/.mcp.json\n+++ /work/.mcp.json\n"),
            "{out}"
        );
        assert!(out.contains("-  \"command\": \"nexql-mcp\""), "{out}");
        assert!(out.contains("+  \"command\": \"npx\""), "{out}");
    }

    #[test]
    fn unchanged_file_says_so() {
        let same = "{}\n";
        assert_eq!(
            render(&wire_diff(same, same), true),
            "  (no changes to /work/.mcp.json)\n"
        );
    }

    #[test]
    fn secrets_in_other_entries_are_redacted() {
        let new = r#"{
  "mcpServers": {
    "github": {
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_s3cret",
        "OPENAI_API_KEY": "sk-s3cret"
      },
      "headers": {
        "Authorization": "Bearer abc.s3cret"
      },
      "args": ["--db", "postgres://me:pa55-s3cret@db.example.com:5432/app"]
    },
    "nexql-mcp": {
      "command": "npx",
      "args": ["-y", "nexql-mcp@latest", "--profile", "mydb"]
    }
  }
}
"#;
        let out = render(&wire_diff("", new), false);
        assert!(!out.contains("s3cret"), "{out}");
        assert!(
            out.contains(r#""GITHUB_PERSONAL_ACCESS_TOKEN": "<redacted>","#),
            "{out}"
        );
        assert!(out.contains(r#""OPENAI_API_KEY": "<redacted>""#), "{out}");
        assert!(out.contains(r#""Authorization": "<redacted>""#), "{out}");
        assert!(
            out.contains("postgres://me:<redacted>@db.example.com:5432/app"),
            "{out}"
        );
        assert!(
            out.contains(r#""args": ["-y", "nexql-mcp@latest", "--profile", "mydb"]"#),
            "{out}"
        );
    }

    #[test]
    fn bearer_in_a_bare_argument_is_redacted() {
        assert_eq!(
            redact_line(r#"+  "--header", "Authorization: Bearer xyz","#),
            r#"+  "--header", "Authorization: Bearer <redacted>","#
        );
    }

    #[test]
    fn urls_without_a_password_are_kept() {
        let line = r#"+  "args": ["postgres://me@db:5432/app?sslmode=require"]"#;
        assert_eq!(redact_line(line), line);
    }
}
