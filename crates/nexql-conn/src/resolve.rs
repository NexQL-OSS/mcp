//! Connection resolution ladder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use url::Url;

use crate::config::{ConfigFile, ProfileConfig};
use crate::error::ConnError;
use crate::pgpass;
use crate::secret::{CommandRunner, ProcessCommandRunner, interpolate_env};

/// Where the winning connection parameters came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    CliArg,
    Profile,
    Flags,
    DatabaseUrl,
    PgEnv,
    DefaultProfile,
    /// Password-only fill from pgpass (params already resolved).
    PgPass,
    EnvFile,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionParams {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub dbname: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub sslmode: Option<String>,
    /// Full URL if the source was a URL string.
    pub url: Option<String>,
}

impl ConnectionParams {
    pub fn merge_from(&mut self, other: &Self) {
        if self.host.is_none() {
            self.host = other.host.clone();
        }
        if self.port.is_none() {
            self.port = other.port;
        }
        if self.dbname.is_none() {
            self.dbname = other.dbname.clone();
        }
        if self.user.is_none() {
            self.user = other.user.clone();
        }
        if self.password.is_none() {
            self.password = other.password.clone();
        }
        if self.sslmode.is_none() {
            self.sslmode = other.sslmode.clone();
        }
        if self.url.is_none() {
            self.url = other.url.clone();
        }
    }

    /// Build a `postgres://` URL suitable for tokio-postgres.
    pub fn to_url(&self) -> Result<String, ConnError> {
        if let Some(ref u) = self.url {
            if self.password.is_some() || self.host.is_some() {
                // May need to inject password into existing URL.
                return inject_password_into_url(u, self.password.as_deref());
            }
            return Ok(u.clone());
        }
        let host = self.host.as_deref().ok_or(ConnError::NoSource)?;
        let port = self.port.unwrap_or(5432);
        let dbname = self.dbname.as_deref().unwrap_or("postgres");
        let user = self.user.as_deref().unwrap_or("postgres");
        let mut url = format!("postgres://{user}@{host}:{port}/{dbname}");
        if let Some(ref pw) = self.password {
            url = inject_password_into_url(&url, Some(pw))?;
        }
        if let Some(ref mode) = self.sslmode {
            let sep = if url.contains('?') { '&' } else { '?' };
            url.push(sep);
            url.push_str("sslmode=");
            url.push_str(mode);
        }
        Ok(url)
    }

    pub fn is_empty(&self) -> bool {
        self.url.is_none() && self.host.is_none() && self.dbname.is_none() && self.user.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedConnection {
    pub params: ConnectionParams,
    pub source: ConnectionSource,
    pub profile_name: Option<String>,
    pub profile: Option<ProfileConfig>,
}

/// Inputs for the resolution ladder. Tests supply a controlled env map.
#[derive(Debug, Clone, Default)]
pub struct ResolveInputs {
    pub cli_url: Option<String>,
    pub profile_names: Vec<String>,
    pub flags: ConnectionParams,
    pub env_file: Option<PathBuf>,
    pub config: Option<ConfigFile>,
    pub config_path: Option<PathBuf>,
    pub pgpass_path: Option<PathBuf>,
    /// Explicit env overlay (tests). When `None`, read process env.
    pub env: Option<HashMap<String, String>>,
}

pub fn resolve(inputs: &ResolveInputs) -> Result<ResolvedConnection, ConnError> {
    resolve_with_runner(inputs, &ProcessCommandRunner)
}

pub fn resolve_with_runner(
    inputs: &ResolveInputs,
    runner: &dyn CommandRunner,
) -> Result<ResolvedConnection, ConnError> {
    let mut env_map = collect_env(inputs)?;

    // Opt-in --env-file only (never implicit cwd .env).
    if let Some(ref path) = inputs.env_file {
        load_env_file(path, &mut env_map)?;
    }

    let getenv = |k: &str| env_map.get(k).cloned();

    // 1. CLI positional URL
    if let Some(ref url) = inputs.cli_url {
        let mut params = params_from_url(url)?;
        fill_password(&mut params, inputs, &getenv, runner, None)?;
        return Ok(ResolvedConnection {
            params,
            source: ConnectionSource::CliArg,
            profile_name: None,
            profile: None,
        });
    }

    let config = load_config(inputs)?;

    // 2. --profile (first name wins for single-connection resolve)
    if let Some(name) = inputs.profile_names.first() {
        let profile = config
            .as_ref()
            .and_then(|c| c.profiles.get(name))
            .cloned()
            .ok_or_else(|| ConnError::ProfileNotFound(name.clone()))?;
        let mut params = params_from_profile(&profile, &getenv)?;
        fill_password(
            &mut params,
            inputs,
            &getenv,
            runner,
            profile.password_command.as_deref(),
        )?;
        // Flags can fill holes only — profile wins for set fields.
        overlay_flags_as_fill(&mut params, &inputs.flags);
        return Ok(ResolvedConnection {
            params,
            source: ConnectionSource::Profile,
            profile_name: Some(name.clone()),
            profile: Some(profile),
        });
    }

    // 3. Explicit host/port/user/dbname flags (any of them)
    if flags_present(&inputs.flags) {
        let mut params = inputs.flags.clone();
        // DATABASE_URL / PG* can fill missing pieces at lower priority later —
        // but flags are the source of truth for what's set.
        fill_from_database_url_env(&mut params, &getenv)?;
        fill_from_pg_env(&mut params, &getenv);
        fill_password(&mut params, inputs, &getenv, runner, None)?;
        return Ok(ResolvedConnection {
            params,
            source: ConnectionSource::Flags,
            profile_name: None,
            profile: None,
        });
    }

    // 4. DATABASE_URL / POSTGRES_URL
    if let Some(url) = getenv("DATABASE_URL").or_else(|| getenv("POSTGRES_URL")) {
        let mut params = params_from_url(&url)?;
        fill_password(&mut params, inputs, &getenv, runner, None)?;
        return Ok(ResolvedConnection {
            params,
            source: ConnectionSource::DatabaseUrl,
            profile_name: None,
            profile: None,
        });
    }

    // 5. PGHOST / PGPORT / PGUSER / PGDATABASE / PGPASSWORD / PGSSLMODE
    if pg_env_present(&getenv) {
        let mut params = ConnectionParams::default();
        fill_from_pg_env(&mut params, &getenv);
        fill_password(&mut params, inputs, &getenv, runner, None)?;
        return Ok(ResolvedConnection {
            params,
            source: ConnectionSource::PgEnv,
            profile_name: None,
            profile: None,
        });
    }

    // 6. default_profile in config
    if let Some(ref cfg) = config {
        if let Some(ref name) = cfg.default_profile {
            let profile = cfg
                .profiles
                .get(name)
                .cloned()
                .ok_or_else(|| ConnError::ProfileNotFound(name.clone()))?;
            let mut params = params_from_profile(&profile, &getenv)?;
            fill_password(
                &mut params,
                inputs,
                &getenv,
                runner,
                profile.password_command.as_deref(),
            )?;
            return Ok(ResolvedConnection {
                params,
                source: ConnectionSource::DefaultProfile,
                profile_name: Some(name.clone()),
                profile: Some(profile),
            });
        }
    }

    Err(ConnError::NoSource)
}

fn collect_env(inputs: &ResolveInputs) -> Result<HashMap<String, String>, ConnError> {
    if let Some(ref map) = inputs.env {
        return Ok(map.clone());
    }
    Ok(std::env::vars().collect())
}

fn load_config(inputs: &ResolveInputs) -> Result<Option<ConfigFile>, ConnError> {
    if let Some(ref cfg) = inputs.config {
        return Ok(Some(cfg.clone()));
    }
    let path = inputs.config_path.clone().or_else(ConfigFile::default_path);
    if let Some(path) = path {
        if path.exists() {
            return Ok(Some(ConfigFile::load_path(&path)?));
        }
    }
    Ok(None)
}

fn load_env_file(path: &Path, env: &mut HashMap<String, String>) -> Result<(), ConnError> {
    let raw = std::fs::read_to_string(path).map_err(|e| ConnError::EnvFile(e.to_string()))?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim().trim_matches('"').trim_matches('\'');
        env.entry(k.to_string()).or_insert_with(|| v.to_string());
    }
    Ok(())
}

pub fn params_from_url(url_str: &str) -> Result<ConnectionParams, ConnError> {
    let normalized = if url_str.starts_with("postgresql://") {
        url_str.replacen("postgresql://", "postgres://", 1)
    } else {
        url_str.to_string()
    };
    let parsed = Url::parse(&normalized).map_err(|e| ConnError::InvalidUrl(e.to_string()))?;
    if parsed.scheme() != "postgres" && parsed.scheme() != "postgresql" {
        return Err(ConnError::InvalidUrl(format!(
            "unsupported scheme {}",
            parsed.scheme()
        )));
    }
    let host = parsed.host_str().map(str::to_owned);
    let port = parsed.port();
    let dbname = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let user = if parsed.username().is_empty() {
        None
    } else {
        Some(parsed.username().to_string())
    };
    let password = parsed.password().map(str::to_owned);
    let sslmode = parsed
        .query_pairs()
        .find(|(k, _)| k == "sslmode")
        .map(|(_, v)| v.into_owned());
    Ok(ConnectionParams {
        host,
        port,
        dbname,
        user,
        password,
        sslmode,
        url: Some(normalized),
    })
}

fn params_from_profile(
    profile: &ProfileConfig,
    getenv: &dyn Fn(&str) -> Option<String>,
) -> Result<ConnectionParams, ConnError> {
    if let Some(ref url) = profile.url {
        let expanded = interpolate_env(url, getenv);
        return params_from_url(&expanded);
    }
    Ok(ConnectionParams {
        host: profile.host.clone(),
        port: profile.port,
        dbname: profile.dbname.clone(),
        user: profile.user.clone(),
        password: profile
            .password
            .as_ref()
            .map(|p| interpolate_env(p, getenv)),
        sslmode: profile.sslmode.clone(),
        url: None,
    })
}

fn flags_present(flags: &ConnectionParams) -> bool {
    flags.host.is_some() || flags.port.is_some() || flags.dbname.is_some() || flags.user.is_some()
}

fn pg_env_present(getenv: &dyn Fn(&str) -> Option<String>) -> bool {
    getenv("PGHOST").is_some()
        || getenv("PGPORT").is_some()
        || getenv("PGUSER").is_some()
        || getenv("PGDATABASE").is_some()
}

fn fill_from_database_url_env(
    params: &mut ConnectionParams,
    getenv: &dyn Fn(&str) -> Option<String>,
) -> Result<(), ConnError> {
    if params.url.is_some() || params.host.is_some() {
        return Ok(());
    }
    if let Some(url) = getenv("DATABASE_URL").or_else(|| getenv("POSTGRES_URL")) {
        let from_url = params_from_url(&url)?;
        params.merge_from(&from_url);
    }
    Ok(())
}

fn fill_from_pg_env(params: &mut ConnectionParams, getenv: &dyn Fn(&str) -> Option<String>) {
    if params.host.is_none() {
        params.host = getenv("PGHOST");
    }
    if params.port.is_none() {
        params.port = getenv("PGPORT").and_then(|p| p.parse().ok());
    }
    if params.user.is_none() {
        params.user = getenv("PGUSER");
    }
    if params.dbname.is_none() {
        params.dbname = getenv("PGDATABASE");
    }
    if params.password.is_none() {
        params.password = getenv("PGPASSWORD");
    }
    if params.sslmode.is_none() {
        params.sslmode = getenv("PGSSLMODE");
    }
}

fn overlay_flags_as_fill(params: &mut ConnectionParams, flags: &ConnectionParams) {
    // Profile wins: only fill holes.
    params.merge_from(flags);
}

fn fill_password(
    params: &mut ConnectionParams,
    inputs: &ResolveInputs,
    getenv: &dyn Fn(&str) -> Option<String>,
    runner: &dyn CommandRunner,
    password_command: Option<&str>,
) -> Result<(), ConnError> {
    if params.password.is_some() {
        return Ok(());
    }
    if let Some(cmd) = password_command {
        let expanded = interpolate_env(cmd, getenv);
        params.password = Some(runner.run_stdout(&expanded)?);
        return Ok(());
    }
    if let Some(pw) = getenv("PGPASSWORD") {
        params.password = Some(pw);
        return Ok(());
    }
    // ~/.pgpass — password only
    let host = params.host.as_deref().unwrap_or("localhost");
    let port = params.port.unwrap_or(5432);
    let dbname = params.dbname.as_deref().unwrap_or("*");
    let user = params.user.as_deref().unwrap_or("*");
    let pgpass_path = inputs
        .pgpass_path
        .clone()
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".pgpass")));
    if let Some(path) = pgpass_path {
        if let Some(pw) = pgpass::lookup_password(&path, host, port, dbname, user)? {
            params.password = Some(pw);
        }
    }
    Ok(())
}

fn inject_password_into_url(url_str: &str, password: Option<&str>) -> Result<String, ConnError> {
    let Some(password) = password else {
        return Ok(url_str.to_string());
    };
    let mut parsed = Url::parse(url_str).map_err(|e| ConnError::InvalidUrl(e.to_string()))?;
    let _ = parsed.set_password(Some(password));
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::CommandRunner;
    use std::sync::Mutex;

    struct MapRunner(Mutex<HashMap<String, String>>);

    impl CommandRunner for MapRunner {
        fn run_stdout(&self, cmdline: &str) -> Result<String, ConnError> {
            self.0
                .lock()
                .unwrap()
                .get(cmdline)
                .cloned()
                .ok_or_else(|| ConnError::PasswordCommand(format!("unknown cmd: {cmdline}")))
        }
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn cli_url_wins_over_database_url() {
        let inputs = ResolveInputs {
            cli_url: Some("postgres://cli@localhost:5432/cli_db".into()),
            env: Some(env(&[(
                "DATABASE_URL",
                "postgres://env@localhost:5432/env_db",
            )])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::CliArg);
        assert_eq!(r.params.dbname.as_deref(), Some("cli_db"));
        assert_eq!(r.params.user.as_deref(), Some("cli"));
    }

    #[test]
    fn profile_wins_over_flags_and_database_url() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "prod".into(),
            ProfileConfig {
                host: Some("prod.example.com".into()),
                dbname: Some("app".into()),
                user: Some("ro".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            profile_names: vec!["prod".into()],
            flags: ConnectionParams {
                host: Some("flag-host".into()),
                ..Default::default()
            },
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(env(&[("DATABASE_URL", "postgres://env@h/db")])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::Profile);
        assert_eq!(r.params.host.as_deref(), Some("prod.example.com"));
        // flag fills nothing because host already set
        assert_eq!(r.profile_name.as_deref(), Some("prod"));
    }

    #[test]
    fn flags_win_over_database_url() {
        let inputs = ResolveInputs {
            flags: ConnectionParams {
                host: Some("flag".into()),
                dbname: Some("flagdb".into()),
                user: Some("flaguser".into()),
                port: Some(6543),
                ..Default::default()
            },
            env: Some(env(&[("DATABASE_URL", "postgres://env@h:5432/envdb")])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::Flags);
        assert_eq!(r.params.host.as_deref(), Some("flag"));
        assert_eq!(r.params.dbname.as_deref(), Some("flagdb"));
        assert_eq!(r.params.port, Some(6543));
    }

    #[test]
    fn database_url_wins_over_pg_env() {
        let inputs = ResolveInputs {
            env: Some(env(&[
                ("DATABASE_URL", "postgres://du@h:5432/dudb"),
                ("PGHOST", "pghost"),
                ("PGDATABASE", "pgdb"),
            ])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::DatabaseUrl);
        assert_eq!(r.params.dbname.as_deref(), Some("dudb"));
    }

    #[test]
    fn postgres_url_alias() {
        let inputs = ResolveInputs {
            env: Some(env(&[("POSTGRES_URL", "postgres://pu@h:5432/pudb")])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::DatabaseUrl);
        assert_eq!(r.params.user.as_deref(), Some("pu"));
    }

    #[test]
    fn pg_env_composes() {
        let inputs = ResolveInputs {
            env: Some(env(&[
                ("PGHOST", "pghost"),
                ("PGPORT", "5555"),
                ("PGUSER", "pguser"),
                ("PGDATABASE", "pgdb"),
                ("PGPASSWORD", "secret"),
                ("PGSSLMODE", "require"),
            ])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::PgEnv);
        assert_eq!(r.params.host.as_deref(), Some("pghost"));
        assert_eq!(r.params.port, Some(5555));
        assert_eq!(r.params.user.as_deref(), Some("pguser"));
        assert_eq!(r.params.dbname.as_deref(), Some("pgdb"));
        assert_eq!(r.params.password.as_deref(), Some("secret"));
        assert_eq!(r.params.sslmode.as_deref(), Some("require"));
    }

    #[test]
    fn default_profile_when_nothing_else() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "local".into(),
            ProfileConfig {
                url: Some("postgres://dev@localhost:5432/appdb".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            config: Some(ConfigFile {
                default_profile: Some("local".into()),
                profiles,
            }),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::DefaultProfile);
        assert_eq!(r.params.dbname.as_deref(), Some("appdb"));
    }

    #[test]
    fn missing_profile_errors() {
        let inputs = ResolveInputs {
            profile_names: vec!["nope".into()],
            config: Some(ConfigFile::default()),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let err = resolve(&inputs).unwrap_err();
        assert!(matches!(err, ConnError::ProfileNotFound(_)));
    }

    #[test]
    fn no_source_errors() {
        let inputs = ResolveInputs {
            env: Some(HashMap::new()),
            config: Some(ConfigFile::default()),
            ..Default::default()
        };
        assert!(matches!(resolve(&inputs).unwrap_err(), ConnError::NoSource));
    }

    #[test]
    fn cwd_dotenv_ignored_without_env_file_flag() {
        // Even if DATABASE_URL would come from a hypothetical .env, without
        // --env-file and without process env, we get NoSource.
        let inputs = ResolveInputs {
            env: Some(HashMap::new()),
            env_file: None,
            ..Default::default()
        };
        assert!(matches!(resolve(&inputs).unwrap_err(), ConnError::NoSource));
    }

    #[test]
    fn env_file_loads_vars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "DATABASE_URL=postgres://ef@h:5432/efdb\n").unwrap();
        let inputs = ResolveInputs {
            env_file: Some(path),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::DatabaseUrl);
        assert_eq!(r.params.dbname.as_deref(), Some("efdb"));
        // Source is DatabaseUrl after env-file merge; EnvFile is the mechanism.
        let _ = ConnectionSource::EnvFile;
    }

    #[test]
    fn pgpass_fills_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".pgpass");
        std::fs::write(&path, "localhost:5432:appdb:dev:frompgpass\n").unwrap();
        let inputs = ResolveInputs {
            flags: ConnectionParams {
                host: Some("localhost".into()),
                port: Some(5432),
                dbname: Some("appdb".into()),
                user: Some("dev".into()),
                ..Default::default()
            },
            pgpass_path: Some(path),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.params.password.as_deref(), Some("frompgpass"));
    }

    #[test]
    fn password_command_from_profile() {
        let runner = MapRunner(Mutex::new(env(&[("op read op://v/p", "cmd-secret")])));
        let mut profiles = HashMap::new();
        profiles.insert(
            "prod".into(),
            ProfileConfig {
                host: Some("h".into()),
                dbname: Some("d".into()),
                user: Some("u".into()),
                password_command: Some("op read op://v/p".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            profile_names: vec!["prod".into()],
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let r = resolve_with_runner(&inputs, &runner).unwrap();
        assert_eq!(r.params.password.as_deref(), Some("cmd-secret"));
    }

    #[test]
    fn password_command_failure() {
        let runner = MapRunner(Mutex::new(HashMap::new()));
        let mut profiles = HashMap::new();
        profiles.insert(
            "prod".into(),
            ProfileConfig {
                host: Some("h".into()),
                password_command: Some("missing".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            profile_names: vec!["prod".into()],
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let err = resolve_with_runner(&inputs, &runner).unwrap_err();
        assert!(matches!(err, ConnError::PasswordCommand(_)));
    }

    #[test]
    fn url_sslmode_query_param() {
        let p = params_from_url("postgres://u@h:5432/db?sslmode=require").unwrap();
        assert_eq!(p.sslmode.as_deref(), Some("require"));
    }

    #[test]
    fn postgresql_scheme_accepted() {
        let p = params_from_url("postgresql://u@h/db").unwrap();
        assert_eq!(p.user.as_deref(), Some("u"));
        assert!(p.url.as_ref().unwrap().starts_with("postgres://"));
    }

    #[test]
    fn env_interpolation_in_profile_url() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "x".into(),
            ProfileConfig {
                url: Some("postgres://u@h/${env:DB_NAME}".into()),
                ..Default::default()
            },
        );
        // interpolate happens on raw URL — path becomes the env value
        let inputs = ResolveInputs {
            profile_names: vec!["x".into()],
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(env(&[("DB_NAME", "interpolated")])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.params.dbname.as_deref(), Some("interpolated"));
    }

    #[test]
    fn to_url_builds_from_parts() {
        let p = ConnectionParams {
            host: Some("h".into()),
            port: Some(1),
            dbname: Some("d".into()),
            user: Some("u".into()),
            password: Some("p".into()),
            sslmode: Some("disable".into()),
            url: None,
        };
        let u = p.to_url().unwrap();
        assert!(u.contains("postgres://u:"));
        assert!(u.contains("@h:1/d"));
        assert!(u.contains("sslmode=disable"));
    }

    // --- expanded precedence matrix ---

    #[test]
    fn cli_wins_over_profile() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "p".into(),
            ProfileConfig {
                url: Some("postgres://prof@h/pdb".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            cli_url: Some("postgres://cli@h/cdb".into()),
            profile_names: vec!["p".into()],
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::CliArg);
        assert_eq!(r.params.dbname.as_deref(), Some("cdb"));
    }

    #[test]
    fn profile_wins_over_pg_env() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "p".into(),
            ProfileConfig {
                host: Some("ph".into()),
                dbname: Some("pdb".into()),
                user: Some("pu".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            profile_names: vec!["p".into()],
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(env(&[("PGHOST", "eh"), ("PGDATABASE", "edb")])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::Profile);
        assert_eq!(r.params.host.as_deref(), Some("ph"));
    }

    #[test]
    fn flags_fill_holes_from_pg_env() {
        let inputs = ResolveInputs {
            flags: ConnectionParams {
                host: Some("flaghost".into()),
                ..Default::default()
            },
            env: Some(env(&[
                ("PGDATABASE", "frompg"),
                ("PGUSER", "fromuser"),
                ("PGPORT", "9999"),
            ])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::Flags);
        assert_eq!(r.params.host.as_deref(), Some("flaghost"));
        assert_eq!(r.params.dbname.as_deref(), Some("frompg"));
        assert_eq!(r.params.user.as_deref(), Some("fromuser"));
        assert_eq!(r.params.port, Some(9999));
    }

    #[test]
    fn default_profile_loses_to_pg_env() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "local".into(),
            ProfileConfig {
                url: Some("postgres://dev@localhost/appdb".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            config: Some(ConfigFile {
                default_profile: Some("local".into()),
                profiles,
            }),
            env: Some(env(&[("PGHOST", "pgh"), ("PGDATABASE", "pgd")])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.source, ConnectionSource::PgEnv);
        assert_eq!(r.params.host.as_deref(), Some("pgh"));
    }

    #[test]
    fn unknown_profile_with_config_present() {
        let mut profiles = HashMap::new();
        profiles.insert("local".into(), ProfileConfig::default());
        let inputs = ResolveInputs {
            profile_names: vec!["missing".into()],
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        assert!(matches!(
            resolve(&inputs),
            Err(ConnError::ProfileNotFound(_))
        ));
    }

    #[test]
    fn invalid_url_errors() {
        let inputs = ResolveInputs {
            cli_url: Some("not-a-url".into()),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        assert!(matches!(resolve(&inputs), Err(ConnError::InvalidUrl(_))));
    }

    #[test]
    fn env_file_does_not_override_existing_env() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "DATABASE_URL=postgres://file@h/fdb\n").unwrap();
        let inputs = ResolveInputs {
            env_file: Some(path),
            env: Some(env(&[("DATABASE_URL", "postgres://proc@h/pdb")])),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.params.dbname.as_deref(), Some("pdb"));
    }

    #[test]
    fn profile_url_beats_profile_host_fields() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "p".into(),
            ProfileConfig {
                url: Some("postgres://u@fromurl/urldb".into()),
                host: Some("ignored".into()),
                dbname: Some("ignored".into()),
                ..Default::default()
            },
        );
        let inputs = ResolveInputs {
            profile_names: vec!["p".into()],
            config: Some(ConfigFile {
                profiles,
                ..Default::default()
            }),
            env: Some(HashMap::new()),
            ..Default::default()
        };
        let r = resolve(&inputs).unwrap();
        assert_eq!(r.params.dbname.as_deref(), Some("urldb"));
        assert_ne!(r.params.host.as_deref(), Some("ignored"));
    }
}
