//! Auto-detection of Postgres connection candidates from environment, workspace files, and local settings.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetectedCandidate {
    pub source: String,
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub dbname: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub sslmode: Option<String>,
    pub is_complete: bool,
}

impl DetectedCandidate {
    pub fn check_complete(&mut self) {
        if self.url.is_some() {
            self.is_complete = true;
            return;
        }
        self.is_complete = self.host.is_some()
            && self.port.is_some()
            && self.dbname.is_some()
            && self.user.is_some();
    }

    /// Secret-safe view for model-facing responses — never expose passwords or URL credentials.
    pub fn redacted_json(&self) -> serde_json::Value {
        use serde_json::json;
        let redacted_url = self.url.as_ref().map(|u| redact_url_credentials(u));
        json!({
            "source": self.source,
            "url": redacted_url,
            "host": self.host,
            "port": self.port,
            "dbname": self.dbname,
            "user": self.user,
            "password": self.password.as_ref().map(|_| "<redacted>"),
            "sslmode": self.sslmode,
            "isComplete": self.is_complete,
        })
    }
}

fn redact_url_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let Some(at_idx) = url[scheme_end + 3..].find('@') else {
        return url.to_string();
    };
    let cred_start = scheme_end + 3;
    let cred_end = cred_start + at_idx;
    let creds = &url[cred_start..cred_end];
    let Some(colon) = creds.find(':') else {
        return url.to_string();
    };
    let user = &creds[..colon];
    format!("{}://{}@{}", &url[..scheme_end], user, &url[cred_end + 1..])
}

pub struct ConnectionDetector;

impl ConnectionDetector {
    /// Detect all connection candidates across environment, ~/.pgpass, and workspace root.
    pub fn detect_all(workspace_root: Option<&Path>) -> Vec<DetectedCandidate> {
        let mut candidates = Vec::new();

        if let Some(cand) = Self::detect_env_vars() {
            candidates.push(cand);
        }

        candidates.extend(Self::detect_pgpass());

        if let Some(root) = workspace_root {
            candidates.extend(Self::detect_dotenv(root));
            candidates.extend(Self::detect_docker_compose(root));
        }

        candidates
    }

    /// Detect from environment variables (DATABASE_URL, POSTGRES_URL, PG*).
    pub fn detect_env_vars() -> Option<DetectedCandidate> {
        if let Ok(url) = std::env::var("DATABASE_URL").or_else(|_| std::env::var("POSTGRES_URL")) {
            if !url.trim().is_empty() {
                let mut cand = DetectedCandidate {
                    source: "environment (DATABASE_URL/POSTGRES_URL)".into(),
                    url: Some(url),
                    host: None,
                    port: None,
                    dbname: None,
                    user: None,
                    password: None,
                    sslmode: None,
                    is_complete: true,
                };
                cand.check_complete();
                return Some(cand);
            }
        }

        let host = std::env::var("PGHOST")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let port = std::env::var("PGPORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok());
        let dbname = std::env::var("PGDATABASE")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let user = std::env::var("PGUSER")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let password = std::env::var("PGPASSWORD")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let sslmode = std::env::var("PGSSLMODE")
            .ok()
            .filter(|s| !s.trim().is_empty());

        if host.is_some() || dbname.is_some() || user.is_some() {
            let mut cand = DetectedCandidate {
                source: "environment (PG*)".into(),
                url: None,
                host,
                port: port.or(Some(5432)),
                dbname,
                user,
                password,
                sslmode,
                is_complete: false,
            };
            cand.check_complete();
            return Some(cand);
        }

        None
    }

    /// Parse entries from ~/.pgpass (format: hostname:port:database:username:password).
    pub fn detect_pgpass() -> Vec<DetectedCandidate> {
        let mut candidates = Vec::new();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let Some(home_dir) = home else {
            return candidates;
        };
        let pgpass_path = home_dir.join(".pgpass");
        let Ok(content) = std::fs::read_to_string(&pgpass_path) else {
            return candidates;
        };

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() == 5 {
                let host = if parts[0] != "*" {
                    Some(parts[0].to_string())
                } else {
                    Some("127.0.0.1".to_string())
                };
                let port = if parts[1] != "*" {
                    parts[1].parse::<u16>().ok()
                } else {
                    Some(5432)
                };
                let dbname = if parts[2] != "*" {
                    Some(parts[2].to_string())
                } else {
                    None
                };
                let user = if parts[3] != "*" {
                    Some(parts[3].to_string())
                } else {
                    None
                };
                let password = if parts[4] != "*" {
                    Some(parts[4].to_string())
                } else {
                    None
                };

                let mut cand = DetectedCandidate {
                    source: "~/.pgpass".into(),
                    url: None,
                    host,
                    port,
                    dbname,
                    user,
                    password,
                    sslmode: None,
                    is_complete: false,
                };
                cand.check_complete();
                candidates.push(cand);
            }
        }

        candidates
    }

    /// Parse .env files under workspace root.
    pub fn detect_dotenv(root: &Path) -> Vec<DetectedCandidate> {
        let mut candidates = Vec::new();
        let env_files = [".env", ".env.local", ".env.development"];

        for filename in env_files {
            let path = root.join(filename);
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };

            let mut url = None;
            let mut host = None;
            let mut port = None;
            let mut dbname = None;
            let mut user = None;
            let mut password = None;
            let mut sslmode = None;

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim();
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    match k {
                        "DATABASE_URL" | "POSTGRES_URL" if !v.is_empty() => {
                            url = Some(v.to_string())
                        }
                        "PGHOST" | "DB_HOST" | "POSTGRES_HOST" if !v.is_empty() => {
                            host = Some(v.to_string())
                        }
                        "PGPORT" | "DB_PORT" | "POSTGRES_PORT" if !v.is_empty() => {
                            port = v.parse::<u16>().ok()
                        }
                        "PGDATABASE" | "DB_NAME" | "POSTGRES_DB" if !v.is_empty() => {
                            dbname = Some(v.to_string())
                        }
                        "PGUSER" | "DB_USER" | "POSTGRES_USER" if !v.is_empty() => {
                            user = Some(v.to_string())
                        }
                        "PGPASSWORD" | "DB_PASSWORD" | "POSTGRES_PASSWORD" if !v.is_empty() => {
                            password = Some(v.to_string())
                        }
                        "PGSSLMODE" | "DB_SSLMODE" if !v.is_empty() => {
                            sslmode = Some(v.to_string())
                        }
                        _ => {}
                    }
                }
            }

            if url.is_some() || host.is_some() || dbname.is_some() {
                let mut cand = DetectedCandidate {
                    source: format!("workspace {filename}"),
                    url,
                    host,
                    port: port.or(Some(5432)),
                    dbname,
                    user,
                    password,
                    sslmode,
                    is_complete: false,
                };
                cand.check_complete();
                candidates.push(cand);
            }
        }

        candidates
    }

    /// Parse docker-compose files under workspace root.
    pub fn detect_docker_compose(root: &Path) -> Vec<DetectedCandidate> {
        let mut candidates = Vec::new();
        let compose_files = ["docker-compose.yml", "docker-compose.yaml", "compose.yaml"];

        for filename in compose_files {
            let path = root.join(filename);
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };

            if !content.contains("postgres") && !content.contains("postgresql") {
                continue;
            }

            let mut dbname = None;
            let mut user = None;
            let mut password = None;
            let mut port = Some(5432);

            for line in content.lines() {
                let line = line.trim();
                if line.contains("POSTGRES_DB=") || line.contains("POSTGRES_DB:") {
                    dbname = line
                        .split(&['=', ':'][..])
                        .nth(1)
                        .map(|s| s.trim().trim_matches('"').to_string());
                } else if line.contains("POSTGRES_USER=") || line.contains("POSTGRES_USER:") {
                    user = line
                        .split(&['=', ':'][..])
                        .nth(1)
                        .map(|s| s.trim().trim_matches('"').to_string());
                } else if line.contains("POSTGRES_PASSWORD=") || line.contains("POSTGRES_PASSWORD:")
                {
                    password = line
                        .split(&['=', ':'][..])
                        .nth(1)
                        .map(|s| s.trim().trim_matches('"').to_string());
                } else if line.contains("5433:5432") {
                    port = Some(5433);
                }
            }

            let mut cand = DetectedCandidate {
                source: format!("workspace {filename}"),
                url: None,
                host: Some("127.0.0.1".into()),
                port,
                dbname,
                user,
                password,
                sslmode: Some("disable".into()),
                is_complete: false,
            };
            cand.check_complete();
            candidates.push(cand);
        }

        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotenv_candidate() {
        let temp_dir = tempfile::tempdir().unwrap();
        let env_path = temp_dir.path().join(".env");
        std::fs::write(
            &env_path,
            r#"
POSTGRES_HOST=127.0.0.1
POSTGRES_PORT=5432
POSTGRES_DB=testdb
POSTGRES_USER=testuser
POSTGRES_PASSWORD=secret
"#,
        )
        .unwrap();

        let cands = ConnectionDetector::detect_dotenv(temp_dir.path());
        assert_eq!(cands.len(), 1);
        let cand = &cands[0];
        assert_eq!(cand.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(cand.dbname.as_deref(), Some("testdb"));
        assert_eq!(cand.user.as_deref(), Some("testuser"));
        assert!(cand.is_complete);
    }
}
