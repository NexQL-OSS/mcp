// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Browser-based setup wizard — local HTTP API + embedded UI.

mod dto;
pub mod wire;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use dto::{
    ClientsApplyRequest, ClientsApplyResponse, ClientsPreviewRequest, ErrorBody, ExportResponse,
    ImportRequest, ImportResponse, MetaResponse, ProfileCreateRequest, ProfileListItem,
    ProfileResponse, ProfileUpsertRequest, SanitizedProfile, SetPasswordRequest,
    TestStartResponse, TestStatusResponse,
};
use nexql_conn::{ConfigFile, ProfileConfig};
use nexql_policy::AccessMode;
use nexql_tools::ConnectionDetector;
use serde_json::json;
use tokio::sync::oneshot;
use uuid::Uuid;
use wire::{apply_wire, list_clients, preview_wire};

const SETUP_HTML: &str = include_str!("../../assets/setup/index.html");

#[derive(Debug, Clone)]
pub struct SetupOptions {
    pub bind: String,
    pub port: u16,
    pub open_browser: bool,
    pub idle_timeout_secs: u64,
    pub config_path: PathBuf,
    pub workspace_root: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) enum TestJobState {
    Running,
    Done {
        server_version: Option<String>,
        is_superuser: Option<bool>,
        latency_ms: Option<f64>,
        error: Option<String>,
    },
}

pub struct SetupState {
    pub token: String,
    pub config_path: PathBuf,
    pub workspace_root: Option<PathBuf>,
    pub config: Mutex<ConfigFile>,
    pub last_activity: Mutex<Instant>,
    pub test_jobs: Mutex<HashMap<String, TestJobState>>,
    pub idle_timeout: Duration,
    pub shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl SetupState {
    fn touch(&self) {
        if let Ok(mut last) = self.last_activity.lock() {
            *last = Instant::now();
        }
    }
}

pub async fn run(options: SetupOptions) -> Result<(), Box<dyn std::error::Error>> {
    if !is_loopback_bind(&options.bind) {
        return Err(format!(
            "refusing to bind setup UI to non-loopback address: {}",
            options.bind
        )
        .into());
    }

    let config_path = options.config_path.clone();
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (config, _) = if config_path.exists() {
        ConfigFile::load_path_migrated(&config_path)?
    } else {
        (ConfigFile::default(), Default::default())
    };

    let token = Uuid::new_v4().to_string();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let state = Arc::new(SetupState {
        token: token.clone(),
        config_path: config_path.clone(),
        workspace_root: options.workspace_root.clone(),
        config: Mutex::new(config),
        last_activity: Mutex::new(Instant::now()),
        test_jobs: Mutex::new(HashMap::new()),
        idle_timeout: Duration::from_secs(options.idle_timeout_secs),
        shutdown_tx: Mutex::new(Some(shutdown_tx)),
    });

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/setup", get(serve_index))
        .route("/api/v1/meta", get(meta_handler))
        .route("/api/v1/profiles", get(list_profiles).post(create_profile))
        .route(
            "/api/v1/profiles/{name}",
            get(get_profile)
                .put(update_profile)
                .delete(delete_profile),
        )
        .route("/api/v1/profiles/{name}/default", post(set_default))
        .route("/api/v1/profiles/{name}/test", post(start_test))
        .route("/api/v1/profiles/{name}/test/{job_id}", get(test_status))
        .route(
            "/api/v1/profiles/{name}/set-password",
            post(set_password),
        )
        .route("/api/v1/profiles/{name}/export", get(export_profile))
        .route("/api/v1/profiles/import", post(import_profiles))
        .route("/api/v1/clients", get(list_clients_handler))
        .route("/api/v1/clients/preview", post(clients_preview))
        .route("/api/v1/clients/apply", post(clients_apply))
        .route("/api/v1/shutdown", post(shutdown_handler))
        .with_state(state.clone());

    let bind_addr: SocketAddr = format!("{}:{}", options.bind, options.port).parse()?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let actual = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}/?token={}", actual.port(), token);

    eprintln!("nexql-mcp setup UI listening on {url}");
    eprintln!("Authorization: Bearer {token}");

    if options.open_browser {
        let _ = open_browser(&url);
    }

    let idle_state = state.clone();
    let idle_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let last = idle_state
                .last_activity
                .lock()
                .map(|t| *t)
                .unwrap_or_else(|_| Instant::now());
            if last.elapsed() >= idle_state.idle_timeout {
                eprintln!(
                    "setup UI idle for {}s — shutting down",
                    idle_state.idle_timeout.as_secs()
                );
                if let Ok(mut guard) = idle_state.shutdown_tx.lock()
                    && let Some(tx) = guard.take()
                {
                    let _ = tx.send(());
                }
                break;
            }
        }
    });

    let serve = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    });

    serve.await?;
    idle_task.abort();
    Ok(())
}

fn is_loopback_bind(bind: &str) -> bool {
    matches!(bind, "127.0.0.1" | "localhost" | "::1")
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = url;
    }
    Ok(())
}

fn keyring_available() -> bool {
    nexql_conn::store_keyring_password("__setup_availability_probe__", "probe").is_ok()
}

fn password_storage_mode() -> (&'static str, Option<String>) {
    if keyring_available() {
        ("keyring", None)
    } else {
        let secrets = nexql_conn::secrets_dir()
            .ok()
            .map(|p| p.display().to_string());
        ("encrypted_file", secrets)
    }
}

fn probe_installed_version() -> Option<String> {
    let output = Command::new("nexql-mcp").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let out = String::from_utf8_lossy(&output.stdout);
    let tokens: Vec<_> = out.split_whitespace().collect();
    tokens.last().map(|s| s.to_string())
}

fn auth_ok(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == format!("Bearer {expected}"))
        || headers
            .get("x-setup-token")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == expected)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody {
            error: "missing or invalid setup token".into(),
        }),
    )
        .into_response()
}

async fn serve_index() -> Html<&'static str> {
    Html(SETUP_HTML)
}

async fn meta_handler(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let workspace = state.workspace_root.as_ref().map(|p| p.display().to_string());
    let detected = ConnectionDetector::detect_all(state.workspace_root.as_deref());
    let (password_storage, secrets_dir) = password_storage_mode();
    let body = MetaResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        config_path: state.config_path.display().to_string(),
        default_profile: config.default_profile.clone(),
        workspace_root: workspace,
        keyring_available: password_storage == "keyring",
        password_storage: password_storage.to_string(),
        secrets_dir,
        installed_binary_version: probe_installed_version(),
        launch_modes: vec!["path", "npx", "npx_latest"],
    };
    let mut resp = Json(body).into_response();
    if let Ok(value) = serde_json::to_value(&detected) {
        resp.headers_mut().insert(
            "x-detected-connections",
            header::HeaderValue::from_str(&value.to_string()).unwrap_or_else(|_| {
                header::HeaderValue::from_static("[]")
            }),
        );
    }
    resp
}

async fn list_profiles(State(state): State<Arc<SetupState>>, headers: HeaderMap) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let mut names: Vec<_> = config.profiles.keys().cloned().collect();
    names.sort();
    let items: Vec<ProfileListItem> = names
        .into_iter()
        .map(|name| {
            let profile = &config.profiles[&name];
            let is_default = config.default_profile.as_deref() == Some(name.as_str());
            ProfileListItem {
                name: name.clone(),
                is_default,
                access_mode: profile.access_mode.clone().unwrap_or_else(|| "read".into()),
                host_or_url: profile
                    .url
                    .clone()
                    .or_else(|| profile.host.clone())
                    .unwrap_or_else(|| "(not set)".into()),
            }
        })
        .collect();
    Json(items).into_response()
}

async fn get_profile(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let Some(profile) = config.profiles.get(&name) else {
        return api_error(StatusCode::NOT_FOUND, format!("profile not found: {name}"));
    };
    Json(ProfileResponse {
        name: name.clone(),
        profile: SanitizedProfile::from(profile),
        is_default: config.default_profile.as_deref() == Some(name.as_str()),
    })
    .into_response()
}

async fn create_profile(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    Json(body): Json<ProfileCreateRequest>,
) -> Response {
    upsert_profile_inner(state, headers, body.name, body.upsert, false).await
}

async fn update_profile(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<ProfileUpsertRequest>,
) -> Response {
    upsert_profile_inner(state, headers, name, body, true).await
}

async fn upsert_profile_inner(
    state: Arc<SetupState>,
    headers: HeaderMap,
    name: String,
    body: ProfileUpsertRequest,
    must_exist: bool,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();

    if let Some(mode_str) = body.profile.access_mode.as_deref()
        && let Ok(mode) = mode_str.parse::<AccessMode>()
        && mode.allows_writes()
        && !body.confirm_elevated_access.unwrap_or(false)
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!(
                "refusing to save profile \"{name}\" with access_mode \"{mode_str}\" — set confirm_elevated_access"
            ),
        );
    }

    let mut config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if must_exist && !config.profiles.contains_key(&name) {
        return api_error(StatusCode::NOT_FOUND, format!("profile not found: {name}"));
    }
    if !must_exist && config.profiles.contains_key(&name) {
        return api_error(
            StatusCode::CONFLICT,
            format!("profile already exists: {name}"),
        );
    }

    let mut profile: ProfileConfig = body.profile.into();
    if let Some(password) = body.password.filter(|p| !p.is_empty()) {
        profile.password = Some(password);
    }

    if let Err(e) = config.upsert_profile_prepared(name.clone(), profile) {
        return api_error(StatusCode::BAD_REQUEST, e.to_string());
    }
    if body.set_default.unwrap_or(false) {
        config.default_profile = Some(name.clone());
    }

    let backup = match config.save(&state.config_path) {
        Ok(b) => b,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let profile = config.profiles.get(&name).unwrap();
    let credential_warning = profile
        .credential_provider
        .as_deref()
        .filter(|p| *p == nexql_conn::ENCRYPTED_FILE_PROVIDER)
        .map(|_| nexql_conn::encrypted_file_storage_warning().to_string());
    let mut resp = json!({
        "name": name,
        "profile": SanitizedProfile::from(profile),
        "is_default": config.default_profile.as_deref() == Some(name.as_str()),
        "backup": backup.map(|b| b.display().to_string()),
    });
    if let Some(warning) = credential_warning {
        resp["credential_warning"] = json!(warning);
    }
    Json(resp).into_response()
}

async fn delete_profile(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let mut config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if config.remove_profile(&name).is_none() {
        return api_error(StatusCode::NOT_FOUND, format!("profile not found: {name}"));
    }
    let backup = match config.save(&state.config_path) {
        Ok(b) => b,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    Json(json!({ "deleted": name, "backup": backup.map(|b| b.display().to_string()) })).into_response()
}

async fn set_default(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let mut config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !config.profiles.contains_key(&name) {
        return api_error(StatusCode::NOT_FOUND, format!("profile not found: {name}"));
    }
    config.default_profile = Some(name.clone());
    let backup = match config.save(&state.config_path) {
        Ok(b) => b,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    Json(json!({ "default_profile": name, "backup": backup.map(|b| b.display().to_string()) }))
        .into_response()
}

async fn start_test(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();

    let params = {
        let config = match state.config.lock() {
            Ok(c) => c,
            Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
        let Some(profile) = config.profiles.get(&name) else {
            return api_error(StatusCode::NOT_FOUND, format!("profile not found: {name}"));
        };
        match nexql_conn::resolve_profile(&name, profile) {
            Ok(p) => p,
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e.to_string()),
        }
    };

    let job_id = Uuid::new_v4().to_string();
    {
        let mut jobs = match state.test_jobs.lock() {
            Ok(j) => j,
            Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
        jobs.insert(job_id.clone(), TestJobState::Running);
    }

    let jobs_ref = Arc::clone(&state);
    let job_id_spawn = job_id.clone();
    tokio::spawn(async move {
        let result = nexql_conn::test_connection(&params).await;
        let done = match result {
            Ok(report) => TestJobState::Done {
                server_version: Some(report.server_version),
                is_superuser: Some(report.is_superuser),
                latency_ms: Some(report.latency.as_secs_f64() * 1000.0),
                error: None,
            },
            Err(e) => TestJobState::Done {
                server_version: None,
                is_superuser: None,
                latency_ms: None,
                error: Some(e.to_string()),
            },
        };
        if let Ok(mut jobs) = jobs_ref.test_jobs.lock() {
            jobs.insert(job_id_spawn, done);
        }
    });

    Json(TestStartResponse {
        job_id,
        status: "running",
    })
    .into_response()
}

async fn test_status(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath((name, job_id)): AxumPath<(String, String)>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let _ = name;
    let jobs = match state.test_jobs.lock() {
        Ok(j) => j,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let Some(job) = jobs.get(&job_id) else {
        return api_error(StatusCode::NOT_FOUND, "test job not found".into());
    };
    match job {
        TestJobState::Running => Json(TestStatusResponse {
            job_id,
            status: "running",
            server_version: None,
            is_superuser: None,
            latency_ms: None,
            error: None,
        })
        .into_response(),
        TestJobState::Done {
            server_version,
            is_superuser,
            latency_ms,
            error,
        } => Json(TestStatusResponse {
            job_id,
            status: if error.is_some() {
                "error"
            } else {
                "ok"
            },
            server_version: server_version.clone(),
            is_superuser: *is_superuser,
            latency_ms: *latency_ms,
            error: error.clone(),
        })
        .into_response(),
    }
}

async fn set_password(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<SetPasswordRequest>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let stored = match nexql_conn::store_profile_password(&name, &body.password) {
        Ok(s) => s,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e.to_string()),
    };
    let mut config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let Some(mut profile) = config.profiles.get(&name).cloned() else {
        return api_error(StatusCode::NOT_FOUND, format!("profile not found: {name}"));
    };
    profile.password = None;
    let used_encrypted = stored.provider == nexql_conn::ENCRYPTED_FILE_PROVIDER;
    profile.credential_provider = Some(stored.provider);
    profile.password_file = stored.password_file;
    config.upsert_profile(name.clone(), profile);
    let backup = match config.save(&state.config_path) {
        Ok(b) => b,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let mut resp = json!({ "status": "ok", "backup": backup.map(|b| b.display().to_string()) });
    if used_encrypted {
        resp["credential_warning"] = json!(nexql_conn::encrypted_file_storage_warning());
    }
    Json(resp).into_response()
}

async fn export_profile(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !config.profiles.contains_key(&name) {
        return api_error(StatusCode::NOT_FOUND, format!("profile not found: {name}"));
    }
    let mut single = ConfigFile {
        default_profile: Some(name.clone()),
        ..Default::default()
    };
    if let Some(p) = config.profiles.get(&name) {
        single.profiles.insert(name.clone(), p.clone());
    }
    let sanitized = single.export_full_sanitized();
    let toml = match sanitized.to_toml_string() {
        Ok(s) => s,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    Json(ExportResponse {
        format: "full".into(),
        toml,
    })
    .into_response()
}

async fn import_profiles(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    Json(body): Json<ImportRequest>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let imported: ConfigFile = match toml::from_str(&body.toml) {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, format!("invalid TOML: {e}")),
    };
    let mut config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let mut count = 0usize;
    for (name, prof) in imported.profiles {
        match nexql_conn::prepare_profile_for_persist(&name, prof) {
            Ok(prepared) => {
                config.upsert_profile(name, prepared);
                count += 1;
            }
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e.to_string()),
        }
    }
    if imported.default_profile.is_some() {
        config.default_profile = imported.default_profile;
    }
    let backup = match config.save(&state.config_path) {
        Ok(b) => b,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    Json(ImportResponse {
        imported: count,
        backup: backup.map(|b| b.display().to_string()),
    })
    .into_response()
}

async fn list_clients_handler(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    Json(list_clients(state.workspace_root.as_deref())).into_response()
}

async fn clients_preview(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    Json(body): Json<ClientsPreviewRequest>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let config = match state.config.lock() {
        Ok(c) => c,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let preview = preview_wire(
        &config,
        &body.profile_names,
        &body.client_keys,
        &body.copy_only_keys,
        &body.launch,
        state.workspace_root.as_deref(),
    );
    Json(preview).into_response()
}

async fn clients_apply(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
    Json(body): Json<ClientsApplyRequest>,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    state.touch();
    let results = apply_wire(&body.preview, &body.apply_keys);
    Json(ClientsApplyResponse { results }).into_response()
}

async fn shutdown_handler(
    State(state): State<Arc<SetupState>>,
    headers: HeaderMap,
) -> Response {
    if !auth_ok(&headers, &state.token) {
        return unauthorized();
    }
    if let Ok(mut guard) = state.shutdown_tx.lock()
        && let Some(tx) = guard.take()
    {
        let _ = tx.send(());
    }
    Json(json!({ "status": "shutting_down" })).into_response()
}

fn api_error(status: StatusCode, message: String) -> Response {
    (status, Json(ErrorBody { error: message })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_guard() {
        assert!(is_loopback_bind("127.0.0.1"));
        assert!(!is_loopback_bind("0.0.0.0"));
    }
}
