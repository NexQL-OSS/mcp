// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! `add_database` — connect another database mid-session without the password
//! passing through the model.
//!
//! The agent supplies only where the database is. The human checks those
//! details on a one-time local page (see `crate::password_page`) and types the
//! password there; the server tests it, stores it in the OS keyring (or the
//! encrypted file fallback), saves a read-only profile, and adds the
//! connection to the running session. The agent never sees the password and
//! cannot choose anything above read access.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nexql_conn::{ConfigFile, ConnectionParams, ProfileConfig};
use nexql_policy::AccessMode;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::exec::ToolOutcome;
use crate::password_page::{PasswordForm, SubmitPassword};
use crate::session::ToolSession;

#[derive(Debug, Clone)]
enum SetupState {
    Waiting { last_error: Option<String> },
    Connected { server_version: String },
}

#[derive(Debug, Clone)]
struct Setup {
    name: String,
    url: String,
    expires_at: Instant,
    state: SetupState,
}

/// Setups started by `add_database` in this server process, by setup id.
#[derive(Clone, Default)]
pub(crate) struct DatabaseSetups {
    inner: Arc<Mutex<HashMap<String, Setup>>>,
}

impl DatabaseSetups {
    fn get(&self, id: &str) -> Option<Setup> {
        self.inner.lock().ok()?.get(id).cloned()
    }

    fn insert(&self, id: String, setup: Setup) {
        if let Ok(mut map) = self.inner.lock() {
            map.insert(id, setup);
        }
    }

    fn set_state(&self, id: &str, state: SetupState) {
        if let Ok(mut map) = self.inner.lock()
            && let Some(setup) = map.get_mut(id)
        {
            setup.state = state;
        }
    }

    /// A setup for `name` that is still waiting on the human, so a repeated
    /// call points at the page already open instead of opening another.
    fn waiting_for(&self, name: &str) -> Option<(String, Setup)> {
        let map = self.inner.lock().ok()?;
        map.iter()
            .find(|(_, s)| {
                s.name == name
                    && s.expires_at > Instant::now()
                    && matches!(s.state, SetupState::Waiting { .. })
            })
            .map(|(id, s)| (id.clone(), s.clone()))
    }
}

/// True when a postgres:// URL carries a password, in the userinfo or as a
/// `password=` query parameter.
fn url_has_password(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    let in_userinfo = authority
        .rsplit_once('@')
        .is_some_and(|(userinfo, _)| userinfo.contains(':'));
    in_userinfo || url.contains("password=")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The terminal route: the same profile, saved by the human with a hidden
/// password prompt, for when no page can be opened.
fn terminal_command(name: &str, profile: &ProfileConfig) -> String {
    let mut cmd = format!("nexql-mcp profile add {name}");
    if let Some(url) = &profile.url {
        cmd.push_str(&format!(" --url '{url}'"));
    } else {
        if let Some(h) = &profile.host {
            cmd.push_str(&format!(" --host {h}"));
        }
        if let Some(p) = profile.port {
            cmd.push_str(&format!(" --port {p}"));
        }
        if let Some(d) = &profile.dbname {
            cmd.push_str(&format!(" --dbname {d}"));
        }
        if let Some(u) = &profile.user {
            cmd.push_str(&format!(" --user {u}"));
        }
    }
    cmd.push_str(" --access-mode read");
    cmd
}

fn status_json(id: &str, setup: &Setup) -> Value {
    match &setup.state {
        SetupState::Connected { server_version } => json!({
            "status": "connected",
            "setupId": id,
            "connectionId": setup.name,
            "accessMode": "read",
            "serverVersion": server_version,
            "message": format!(
                "The user connected \"{}\". Query it by passing connectionId \"{}\", or switch_connection to it.",
                setup.name, setup.name
            ),
        }),
        SetupState::Waiting { .. } if setup.expires_at <= Instant::now() => json!({
            "status": "expired",
            "setupId": id,
            "message": "The page expired before the user finished. Call add_database again to open a new one.",
        }),
        SetupState::Waiting { last_error } => json!({
            "status": "waiting_for_user",
            "setupId": id,
            "url": setup.url,
            "lastError": last_error,
            "message": "The user hasn't finished on the page yet. Wait for them to say they're done, then check again.",
        }),
    }
}

/// Test the typed password, store it, save the profile, and add the
/// connection to the session. Runs when the human submits the page.
async fn complete_setup(
    session: &Arc<ToolSession>,
    name: &str,
    profile: &ProfileConfig,
    password: String,
) -> Result<String, String> {
    let password = (!password.is_empty()).then_some(password);
    let params = ConnectionParams {
        url: profile.url.clone(),
        host: profile.host.clone(),
        port: profile.port,
        dbname: profile.dbname.clone(),
        user: profile.user.clone(),
        password: password.clone(),
        sslmode: profile.sslmode.clone(),
        ..Default::default()
    };
    let report = nexql_conn::test_connection(&params)
        .await
        .map_err(|e| format!("Could not connect: {e}"))?;

    let path = ConfigFile::default_path().ok_or("could not find the nexql-mcp config folder")?;
    let mut cfg = ConfigFile::load_path_migrated(&path)
        .map(|(c, _)| c)
        .unwrap_or_default();
    if cfg.profiles.contains_key(name) {
        return Err(format!(
            "a saved connection named \"{name}\" already exists"
        ));
    }
    let routed = nexql_conn::route_password_to_keyring(name, password.as_deref())
        .map_err(|e| format!("Could not store the password securely: {e}"))?;
    let saved = ProfileConfig {
        password: routed.password,
        credential_provider: routed.credential_provider,
        password_file: routed.password_file,
        ..profile.clone()
    };
    // Straight insert, not `upsert_profile`: adding a database must not make
    // it the default for the next launch.
    cfg.profiles.insert(name.to_string(), saved.clone());
    cfg.save(&path).map_err(|e| e.to_string())?;
    session
        .register_profile(name, &saved, AccessMode::Read, session.caps())
        .map_err(|e| e.to_string())?;
    Ok(report.server_version)
}

pub(crate) async fn add_database(
    session: &Arc<ToolSession>,
    setups: &DatabaseSetups,
    args: &Value,
) -> Result<ToolOutcome, ToolError> {
    if args.get("password").is_some() {
        return Err(ToolError::InvalidArgs(
            "add_database never takes a password: the user types it on the page it opens. Call it again without one, and don't ask the user for the password in chat.".into(),
        ));
    }

    if let Some(id) = str_arg(args, "setupId") {
        let setup = setups
            .get(id)
            .ok_or_else(|| ToolError::InvalidArgs(format!("no setup with id {id}")))?;
        return Ok(ToolOutcome::ok_json(status_json(id, &setup)));
    }

    let url = str_arg(args, "url").map(String::from);
    if url.as_deref().is_some_and(url_has_password) {
        return Err(ToolError::InvalidArgs(
            "remove the password from the URL; the user types it on the page instead".into(),
        ));
    }
    let profile = ProfileConfig {
        url: url.clone(),
        host: str_arg(args, "host").map(String::from),
        port: args.get("port").and_then(|v| v.as_u64()).map(|n| n as u16),
        dbname: str_arg(args, "dbname").map(String::from),
        user: str_arg(args, "user").map(String::from),
        sslmode: str_arg(args, "sslmode").map(String::from),
        access_mode: Some("read".into()),
        ..Default::default()
    };
    if url.is_none() {
        let missing: Vec<&str> = [
            ("host", profile.host.is_none()),
            ("dbname", profile.dbname.is_none()),
            ("user", profile.user.is_none()),
        ]
        .into_iter()
        .filter_map(|(field, absent)| absent.then_some(field))
        .collect();
        if !missing.is_empty() {
            return Ok(ToolOutcome::ok_json(json!({
                "status": "needs_input",
                "missingFields": missing,
                "message": "Ask the user for these details (not the password), then call again.",
            })));
        }
    }

    let name = str_arg(args, "name")
        .or(profile.dbname.as_deref())
        .unwrap_or("database")
        .to_string();
    if !valid_name(&name) {
        return Err(ToolError::InvalidArgs(format!(
            "invalid name \"{name}\": use letters, digits, _ or -"
        )));
    }
    if session.connections().iter().any(|c| c.id == name) {
        return Err(ToolError::InvalidArgs(format!(
            "a connection named \"{name}\" is already open — pick another name"
        )));
    }
    let saved = ConfigFile::default_path()
        .and_then(|p| ConfigFile::load_path_migrated(&p).ok())
        .is_some_and(|(cfg, _)| cfg.profiles.contains_key(&name));
    if saved {
        return Err(ToolError::InvalidArgs(format!(
            "a saved connection named \"{name}\" already exists — pick another name, or ask the user to restart nexql-mcp to load it"
        )));
    }

    if let Some((id, setup)) = setups.waiting_for(&name) {
        return Ok(ToolOutcome::ok_json(status_json(&id, &setup)));
    }

    let terminal = terminal_command(&name, &profile);
    let Some(page) = session.password_page() else {
        return Ok(ToolOutcome::ok_json(json!({
            "status": "needs_user",
            "terminalCommand": terminal,
            "message": "This server can't open a page for the user. Ask them to run terminalCommand in their own terminal (it asks for the password without showing it), then restart nexql-mcp so it loads the new connection. Don't ask for the password in chat.",
        })));
    };

    let id = uuid::Uuid::new_v4().simple().to_string();
    let submit: SubmitPassword = {
        let session = session.clone();
        let setups = setups.clone();
        let id = id.clone();
        let name = name.clone();
        let profile = profile.clone();
        Arc::new(move |password: String| {
            let session = session.clone();
            let setups = setups.clone();
            let id = id.clone();
            let name = name.clone();
            let profile = profile.clone();
            Box::pin(async move {
                match complete_setup(&session, &name, &profile, password).await {
                    Ok(server_version) => {
                        setups.set_state(&id, SetupState::Connected { server_version });
                        Ok(format!(
                            "\"{name}\" is connected, read-only. You can close this page and go back to your chat."
                        ))
                    }
                    Err(e) => {
                        setups.set_state(
                            &id,
                            SetupState::Waiting {
                                last_error: Some(e.clone()),
                            },
                        );
                        Err(e)
                    }
                }
            })
        })
    };

    let mut details = vec![("Name".to_string(), name.clone())];
    if let Some(u) = &profile.url {
        details.push(("URL".into(), u.clone()));
    }
    for (label, value) in [
        ("Host", profile.host.clone()),
        ("Port", Some(profile.port.unwrap_or(5432).to_string())),
        ("Database", profile.dbname.clone()),
        ("User", profile.user.clone()),
        ("SSL mode", profile.sslmode.clone()),
    ] {
        if let Some(v) = value
            && (profile.url.is_none() || label != "Port")
        {
            details.push((label.into(), v));
        }
    }
    details.push(("Access".into(), "Read-only".into()));

    let opened = page
        .open(PasswordForm {
            title: format!("Connect \"{name}\""),
            details,
            submit,
        })
        .await
        .map_err(|e| ToolError::Execution(format!("could not open the password page: {e}")))?;

    setups.insert(
        id.clone(),
        Setup {
            name: name.clone(),
            url: opened.url.clone(),
            expires_at: Instant::now() + Duration::from_secs(opened.expires_in_secs),
            state: SetupState::Waiting { last_error: None },
        },
    );

    let how = if opened.opened_browser {
        "A page has opened in the user's browser."
    } else {
        "Give the user this link to open in their browser."
    };
    Ok(ToolOutcome::ok_json(json!({
        "status": "waiting_for_user",
        "setupId": id,
        "connectionId": name,
        "url": opened.url,
        "openedBrowser": opened.opened_browser,
        "expiresInSeconds": opened.expires_in_secs,
        "terminalCommand": terminal,
        "message": format!(
            "{how} Tell the user to check the details on it and type the password there, not in this chat. When they say they're done, call add_database with setupId to confirm. If they can't use the page, they can run terminalCommand in their own terminal instead, then restart nexql-mcp."
        ),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_password_is_detected() {
        assert!(url_has_password("postgres://u:secret@h/db"));
        assert!(url_has_password("postgresql://u:@h:5432/db"));
        assert!(url_has_password("postgres://u@h/db?password=x"));
        assert!(!url_has_password("postgres://u@h:5432/db"));
        assert!(!url_has_password("postgres://h/db?sslmode=require"));
    }

    #[test]
    fn names_are_plain_identifiers() {
        assert!(valid_name("billing"));
        assert!(valid_name("billing-eu_2"));
        assert!(!valid_name(""));
        assert!(!valid_name("a b"));
        assert!(!valid_name("../x"));
    }

    #[test]
    fn terminal_command_is_read_only_and_has_no_password() {
        let profile = ProfileConfig {
            host: Some("localhost".into()),
            port: Some(5433),
            dbname: Some("billing".into()),
            user: Some("billing_reader".into()),
            ..Default::default()
        };
        assert_eq!(
            terminal_command("billing", &profile),
            "nexql-mcp profile add billing --host localhost --port 5433 --dbname billing --user billing_reader --access-mode read"
        );
    }
}
