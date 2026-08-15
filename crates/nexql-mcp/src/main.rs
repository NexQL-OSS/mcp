// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! CLI entrypoint for the standalone NexQL Postgres MCP server.

mod client_targets;
mod init_clients;
mod tui;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use async_trait::async_trait;
use clap::{Parser, Subcommand};
use nexql_conn::{
    ConfigFile, ConnectionParams, PoolOptions, ResolveInputs, ResolvedConnection,
    apply_session_guards, connect_once, resolve,
};
use nexql_index::{
    BuildDepth, BuildMode, BuildRequest, CatalogDb, IndexScope, IndexStore, PgCatalogDb,
    build_index,
};
use nexql_policy::{AccessMode, PolicyCaps, check_superuser_guard};
use nexql_proto::{
    CompletionBackend, HttpAuth, HttpServer, McpHandler, PromptBackend, ResourceBackend,
    RpcFailure, StdioServer, ToolAnnotations, ToolBackend, ToolCallResult, ToolDescriptor,
};
use nexql_tools::{
    CompletionsProvider, PromptCatalog, ResourceProvider, ToolRouter, ToolSession,
    default_index_root, policy_from_profile,
};
use serde_json::{Value, json};
use tracing_subscriber::{EnvFilter, Layer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
enum EmbeddingsMode {
    #[default]
    Off,
    Local,
}

impl EmbeddingsMode {
    fn is_local(self) -> bool {
        matches!(self, Self::Local)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
enum CliToolProfile {
    Query,
    Dba,
    Meta,
    #[default]
    Full,
}

impl From<CliToolProfile> for nexql_tools::ToolProfile {
    fn from(p: CliToolProfile) -> Self {
        match p {
            CliToolProfile::Query => nexql_tools::ToolProfile::Query,
            CliToolProfile::Dba => nexql_tools::ToolProfile::Dba,
            CliToolProfile::Meta => nexql_tools::ToolProfile::Meta,
            CliToolProfile::Full => nexql_tools::ToolProfile::Full,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "nexql-mcp",
    about = "Standalone Postgres MCP server with schema-aware tooling",
    version,
    after_help = "Every flag has a NEXQL_MCP_<UPPER_SNAKE> env equivalent."
)]
struct Cli {
    /// Connection string (postgres://…), highest-precedence source
    connection_string: Option<String>,

    #[command(flatten)]
    connection: ConnectionArgs,

    #[command(flatten)]
    transport: TransportArgs,

    #[command(flatten)]
    access: AccessArgs,

    /// Local MiniLM embeddings for index build + semantic search_schema (RRF).
    /// Env: `NEXQL_MCP_EMBEDDINGS=off|local` (default off).
    #[arg(long, value_enum, default_value_t = EmbeddingsMode::Off, env = "NEXQL_MCP_EMBEDDINGS")]
    embeddings: EmbeddingsMode,

    /// Tool surface profile: query (core query/discovery), dba (monitoring/admin), full (all 41 tools).
    /// Env: `NEXQL_MCP_TOOLS=query|dba|full` (default full).
    #[arg(long, value_enum, default_value_t = CliToolProfile::Full, env = "NEXQL_MCP_TOOLS")]
    tools: CliToolProfile,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Args, Default)]
struct ConnectionArgs {
    #[arg(long = "profile")]
    profiles: Vec<String>,

    #[arg(long, short = 'd')]
    dbname: Option<String>,

    #[arg(long)]
    host: Option<String>,

    #[arg(long, short = 'p')]
    port: Option<u16>,

    #[arg(long, short = 'U')]
    user: Option<String>,

    #[arg(long = "env-file")]
    env_file: Option<PathBuf>,

    #[arg(long)]
    sslmode: Option<String>,

    #[arg(long = "config")]
    config: Option<PathBuf>,

    /// Workspace root directory for project config discovery (.nexql/config.toml).
    #[arg(long = "workspace-root", env = "NEXQL_MCP_WORKSPACE_ROOT")]
    workspace_root: Option<PathBuf>,
}

#[derive(clap::Args, Default)]
struct TransportArgs {
    #[arg(long)]
    stdio: bool,

    #[arg(long)]
    http: bool,

    #[arg(long = "http-port", default_value_t = 8899)]
    http_port: u16,

    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Bearer token for HTTP transport. Env: `NEXQL_MCP_HTTP_TOKEN`.
    #[arg(long = "http-token", env = "NEXQL_MCP_HTTP_TOKEN")]
    http_token: Option<String>,

    /// Requests per 60s window per bearer token (or per client IP with no
    /// token). `0` disables the limit. Env: `NEXQL_MCP_HTTP_RATE_LIMIT`.
    #[arg(
        long = "http-rate-limit",
        env = "NEXQL_MCP_HTTP_RATE_LIMIT",
        default_value_t = 600
    )]
    http_rate_limit: u32,
}

#[derive(clap::Args, Default)]
struct AccessArgs {
    #[arg(long, default_value = "read")]
    access_mode: String,

    #[arg(long = "i-know-what-im-doing")]
    i_know_what_im_doing: bool,

    #[arg(long = "max-rows")]
    max_rows: Option<u32>,

    /// Managed extension mode: read-only, excludes setup/profile mutation tools.
    #[arg(long = "managed-extension", env = "NEXQL_MCP_MANAGED")]
    managed_extension: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
enum OutputFormat {
    #[default]
    Table,
    Json,
    Csv,
}

#[derive(Subcommand)]
enum Commands {
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    Doctor,
    /// Onboard to AI models / client setup (interactive wizard or snippet printing).
    Init {
        /// Target client (e.g. claude, cursor, zed, windsurf, vscode). If omitted, opens the TUI wizard.
        client: Option<String>,

        /// Force interactive TUI onboarding wizard.
        #[arg(long)]
        tui: bool,
    },
    /// Interactive AI model onboarding & client configuration wizard TUI.
    Onboarding,
    /// Interactive profile editor + multi-client wiring.
    Tui,
    /// Execute a read-only SELECT or WITH query and format results in the terminal.
    Query {
        /// SQL statement to execute
        sql: String,

        /// Output format (table, json, csv)
        #[arg(long, short, value_enum, default_value_t = OutputFormat::Table)]
        format: OutputFormat,
    },
    /// Compare two database schemas and display table, column, and index differences.
    Diff {
        /// Source schema name (e.g. public)
        source_schema: String,

        /// Target schema name (e.g. staging)
        target_schema: String,

        /// Emit step-by-step SQL migration script instead of structured diff
        #[arg(long)]
        migration: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
enum CliBuildDepth {
    #[default]
    Structure,
    Stats,
    Profiles,
}

impl From<CliBuildDepth> for BuildDepth {
    fn from(d: CliBuildDepth) -> Self {
        match d {
            CliBuildDepth::Structure => Self::Structure,
            CliBuildDepth::Stats => Self::Stats,
            CliBuildDepth::Profiles => Self::Profiles,
        }
    }
}

#[derive(Subcommand)]
enum IndexAction {
    Build {
        /// Index depth: structure | stats | profiles (profiles enables sample_values).
        /// Env: `NEXQL_MCP_INDEX_DEPTH`.
        #[arg(
            long,
            value_enum,
            default_value_t = CliBuildDepth::Structure,
            env = "NEXQL_MCP_INDEX_DEPTH"
        )]
        depth: CliBuildDepth,
    },
    Status,
    Refresh,
    Clear {
        /// Clear every index under the index root
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    List,
    Add {
        /// Profile name.
        name: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        dbname: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        password_command: Option<String>,
        #[arg(long)]
        password_file: Option<String>,
        #[arg(long)]
        credential_provider: Option<String>,
        #[arg(long)]
        access_mode: Option<String>,
        /// Set this profile as the default.
        #[arg(long)]
        set_default: bool,
        /// Skip connection test.
        #[arg(long)]
        no_test: bool,
    },
    SetPassword {
        name: String,
    },
    Test {
        name: Option<String>,
    },
    Export {
        /// Format: project (.nexql/config.toml) or full (sanitized config).
        #[arg(long, default_value = "project")]
        format: String,
        /// Output path (default: stdout).
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Import {
        /// File path to import.
        path: PathBuf,
    },
    /// Migrate legacy plaintext passwords to the OS keyring (also runs automatically on load).
    MigrateSecrets,
}

struct RouterBackend {
    router: ToolRouter,
}

#[async_trait]
impl ToolBackend for RouterBackend {
    async fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.router
            .specs()
            .iter()
            .map(|s| {
                let h = s.name.hints();
                ToolDescriptor {
                    name: s.name.as_str().to_string(),
                    description: s.description.to_string(),
                    input_schema: s.input_schema.clone(),
                    annotations: Some(ToolAnnotations {
                        read_only_hint: Some(h.read_only),
                        destructive_hint: Some(h.destructive),
                        idempotent_hint: Some(h.idempotent),
                        open_world_hint: Some(h.open_world),
                    }),
                }
            })
            .collect()
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> ToolCallResult {
        let out = self.router.call(name, arguments).await;
        ToolCallResult {
            text: out.text,
            structured: out.structured,
            is_error: out.is_error,
        }
    }
}

struct IndexResourceBackend {
    provider: ResourceProvider,
}

#[async_trait]
impl ResourceBackend for IndexResourceBackend {
    async fn list_resources(&self, cursor: Option<String>) -> Result<Value, RpcFailure> {
        let r = self
            .provider
            .list(cursor.as_deref())
            .map_err(|e| RpcFailure {
                code: e.code(),
                message: e.to_string(),
            })?;
        serde_json::to_value(r).map_err(|e| RpcFailure {
            code: -32603,
            message: e.to_string(),
        })
    }

    async fn read_resource(&self, uri: &str) -> Result<Value, RpcFailure> {
        let r = self.provider.read(uri).map_err(|e| RpcFailure {
            code: e.code(),
            message: e.to_string(),
        })?;
        serde_json::to_value(r).map_err(|e| RpcFailure {
            code: -32603,
            message: e.to_string(),
        })
    }

    fn list_templates(&self) -> Value {
        json!({
            "resourceTemplates": self.provider.list_templates()
        })
    }
}

struct StaticPromptBackend;

#[async_trait]
impl PromptBackend for StaticPromptBackend {
    async fn list_prompts(&self) -> Value {
        json!({ "prompts": PromptCatalog::list() })
    }

    async fn get_prompt(&self, name: &str, arguments: Value) -> Result<Value, RpcFailure> {
        let mut args = std::collections::HashMap::new();
        if let Some(obj) = arguments.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    args.insert(k.clone(), s.to_owned());
                } else if !v.is_null() {
                    args.insert(k.clone(), v.to_string());
                }
            }
        }
        let r = PromptCatalog::get(name, &args).map_err(|e| RpcFailure {
            code: e.code(),
            message: e.to_string(),
        })?;
        serde_json::to_value(r).map_err(|e| RpcFailure {
            code: -32603,
            message: e.to_string(),
        })
    }
}

struct IndexCompletionBackend {
    provider: CompletionsProvider,
    session: Arc<ToolSession>,
}

#[async_trait]
impl CompletionBackend for IndexCompletionBackend {
    async fn complete(&self, params: Value) -> Result<Value, RpcFailure> {
        let argument = params.get("argument").ok_or_else(|| RpcFailure {
            code: -32602,
            message: "missing argument".into(),
        })?;
        let name = argument.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = argument.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let (connection_id, database) = self.session.active_context().await;
        self.provider
            .complete_ref(&connection_id, &database, name, value)
            .map(|r| json!({ "completion": r }))
            .map_err(|e| RpcFailure {
                code: e.code(),
                message: e.to_string(),
            })
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    // MCP stdio: never log to stdout. File logging supported if NEXQL_MCP_LOG set or default path available.
    let log_path = std::env::var("NEXQL_MCP_LOG")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                PathBuf::from(h)
                    .join(".config")
                    .join("nexql-mcp")
                    .join("logs")
                    .join("nexql-mcp.log")
            })
        });

    if let Some(ref path) = log_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;

            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
                );

            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(Arc::new(file))
                .with_ansi(false)
                .with_filter(EnvFilter::new("info"));

            let _ = tracing_subscriber::registry()
                .with(stderr_layer)
                .with(file_layer)
                .try_init();
        } else {
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
                )
                .try_init();
        }
    } else {
        let _ = tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
            )
            .try_init();
    }

    let cli = Cli::parse();

    if let Some(ref cmd) = cli.command {
        match cmd {
            Commands::Doctor => match run_doctor(&cli).await {
                Ok(()) => return ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("doctor failed: {e}");
                    return ExitCode::FAILURE;
                }
            },
            Commands::Init { client, tui } => {
                if *tui || client.is_none() {
                    return match tui::run_onboarding(cli.connection.config.clone()).await {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(e) => {
                            eprintln!("onboarding tui error: {e}");
                            ExitCode::FAILURE
                        }
                    };
                }
                let client_name = client.as_deref().unwrap();
                match init_clients::init_snippet(client_name, cli.connection_string.as_deref()) {
                    Ok(snippet) => println!("{snippet}"),
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            Commands::Onboarding => {
                return match tui::run_onboarding(cli.connection.config.clone()).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("onboarding tui error: {e}");
                        ExitCode::FAILURE
                    }
                };
            }
            Commands::Index { action } => {
                return match run_index_action(&cli, action).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("{e}");
                        ExitCode::FAILURE
                    }
                };
            }
            Commands::Profile { action } => {
                return match run_profile_action(&cli, action).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("{e}");
                        ExitCode::FAILURE
                    }
                };
            }
            Commands::Tui => {
                return match tui::run(cli.connection.config.clone()).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("tui error: {e}");
                        ExitCode::FAILURE
                    }
                };
            }
            Commands::Query { sql, format } => {
                return match run_query_cmd(&cli, sql, *format).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("query failed: {e}");
                        ExitCode::FAILURE
                    }
                };
            }
            Commands::Diff {
                source_schema,
                target_schema,
                migration,
            } => {
                return match run_diff_cmd(&cli, source_schema, target_schema, *migration).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("diff failed: {e}");
                        ExitCode::FAILURE
                    }
                };
            }
        }
        return ExitCode::SUCCESS;
    }

    if cli.transport.http {
        match run_http_server(&cli).await {
            Ok(()) => return ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("server error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    use std::io::IsTerminal;
    let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let looks_bare = cli.connection_string.is_none()
        && cli.connection.profiles.is_empty()
        && cli.connection.host.is_none()
        && cli.connection.dbname.is_none()
        && cli.connection.user.is_none();
    if is_tty && looks_bare && resolve(&resolve_inputs(&cli)).is_err() {
        return match tui::run(cli.connection.config.clone()).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("tui error: {e}");
                ExitCode::FAILURE
            }
        };
    }

    match run_stdio_server(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("server error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn resolve_inputs(cli: &Cli) -> ResolveInputs {
    ResolveInputs {
        cli_url: cli.connection_string.clone(),
        profile_names: cli.connection.profiles.clone(),
        flags: ConnectionParams {
            host: cli.connection.host.clone(),
            port: cli.connection.port,
            dbname: cli.connection.dbname.clone(),
            user: cli.connection.user.clone(),
            sslmode: cli.connection.sslmode.clone(),
            ..Default::default()
        },
        env_file: cli.connection.env_file.clone(),
        config_path: cli.connection.config.clone(),
        workspace_root: cli.connection.workspace_root.clone(),
        env: None,
        ..Default::default()
    }
}

fn migrate_legacy_secrets(cli: &Cli) {
    if let Ok(path) = profile_config_path(cli) {
        if let Ok((_, report)) = ConfigFile::load_path_migrated(&path) {
            if report.any_changes() {
                emit_secret_migration_report(&report);
            }
        }
    }
}

async fn build_mcp_handler(cli: &Cli) -> Result<McpHandler, Box<dyn std::error::Error>> {
    migrate_legacy_secrets(cli);
    let resolved_list = nexql_conn::resolve_all(&resolve_inputs(cli))?;
    let active_resolved = resolved_list.first();

    let cli_mode: AccessMode = if cli.access.managed_extension {
        AccessMode::Read
    } else {
        cli.access.access_mode.parse()?
    };

    let mut default_caps = PolicyCaps::default();
    if let Some(n) = cli.access.max_rows {
        default_caps = default_caps.with_max_rows(n);
    }

    let active_id = active_resolved.and_then(|r| r.profile_name.clone());
    let connections: Vec<nexql_tools::ConnectionInfo> = resolved_list
        .iter()
        .map(|r| {
            let id = r.profile_name.clone().unwrap_or_else(|| "default".into());
            let mut policy =
                policy_from_profile(r.profile.as_ref(), cli_mode, default_caps.clone());
            if cli.access.managed_extension {
                policy.access_mode = AccessMode::Read;
            }
            if let Some(n) = cli.access.max_rows {
                policy.caps = policy.caps.with_max_rows(n);
            }
            nexql_tools::ConnectionInfo {
                id: id.clone(),
                name: id,
                host: r.params.host.clone(),
                port: r.params.port,
                database: r.params.dbname.clone(),
                params: r.params.clone(),
                policy,
            }
        })
        .collect();

    if let Some(resolved) = active_resolved {
        let client = connect_once(&resolved.params).await?;
        let row = client
            .query_one("SELECT current_setting('is_superuser')", &[])
            .await?;
        let is_super: String = row.get(0);
        let is_superuser = is_super.eq_ignore_ascii_case("on");
        let mode = connections
            .first()
            .map(|c| c.policy.access_mode)
            .unwrap_or(cli_mode);
        check_superuser_guard(mode, is_superuser, cli.access.i_know_what_im_doing)?;
    }

    let session = ToolSession::from_connections(connections, active_id).await?;

    if let Some(store) = session.index_store.clone()
        && let Some(resolved) = active_resolved
    {
        let (connection_id, database) = index_ids(resolved);
        let base = store.base_dir(&connection_id, &database);
        if store.read_manifest(&base)?.is_none() {
            let req = default_build_request(
                connection_id.clone(),
                database.clone(),
                BuildDepth::Structure,
                cli.embeddings.is_local(),
            );
            let resolved_for_index = resolved.clone();
            tokio::task::spawn_blocking(move || {
                eprintln!(
                    "index: no schema index for {connection_id}/{database} — building automatically"
                );
                let handle = tokio::runtime::Handle::current();
                if let Err(e) = handle.block_on(run_build_request(&resolved_for_index, req)) {
                    eprintln!(
                        "warning: automatic index build failed for {connection_id}/{database}: {e}"
                    );
                }
            });
        }
    }

    #[cfg(feature = "embeddings")]
    let embedder: Option<std::sync::Arc<dyn nexql_index::Embedder>> = if cli.embeddings.is_local() {
        match nexql_index::MiniLmEmbedder::load() {
            Ok(m) => Some(std::sync::Arc::new(m)),
            Err(e) => {
                eprintln!("warning: embeddings local requested but MiniLM load failed: {e}");
                None
            }
        }
    } else {
        None
    };
    #[cfg(not(feature = "embeddings"))]
    let embedder: Option<std::sync::Arc<dyn nexql_index::Embedder>> = {
        if cli.embeddings.is_local() {
            eprintln!(
                "warning: --embeddings local ignored (nexql-mcp built without `embeddings` feature)"
            );
        }
        None
    };

    let mut router = ToolRouter::new(session.clone())
        .with_profile(cli.tools.into())
        .with_semantic(cli.embeddings.is_local(), embedder);
    if cli.access.managed_extension {
        router = router.with_managed_extension(true);
    }

    let tools = Arc::new(RouterBackend { router });

    let mut handler = McpHandler::new(tools)
        .with_prompts(Arc::new(StaticPromptBackend))
        .with_server_title("NexQL Postgres MCP");

    if let Some(store) = session.index_store.clone() {
        handler = handler
            .with_resources(Arc::new(IndexResourceBackend {
                provider: ResourceProvider::new(store.clone()),
            }))
            .with_completions(Arc::new(IndexCompletionBackend {
                provider: CompletionsProvider::new(store),
                session: session.clone(),
            }));
    }

    Ok(handler)
}

async fn run_stdio_server(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let handler = build_mcp_handler(cli).await?;
    let server = StdioServer::from_handler(handler);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    server.serve(stdin, stdout).await?;
    Ok(())
}

async fn run_http_server(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if !is_loopback_bind(&cli.transport.bind) && cli.transport.http_token.is_none() {
        return Err(
            "refusing to bind non-loopback address without --http-token (set NEXQL_MCP_HTTP_TOKEN)"
                .into(),
        );
    }

    let handler = build_mcp_handler(cli).await?;
    let auth = HttpAuth {
        token: cli.transport.http_token.clone(),
    };
    if auth.token.is_none() {
        eprintln!(
            "warning: HTTP server has no bearer token; only safe on loopback (bind={})",
            cli.transport.bind
        );
    }

    let server = HttpServer::new(
        handler,
        cli.transport.bind.clone(),
        cli.transport.http_port,
        auth,
    )
    .with_rate_limit_per_min(cli.transport.http_rate_limit);
    server.serve().await?;
    Ok(())
}

fn is_loopback_bind(bind: &str) -> bool {
    matches!(bind, "127.0.0.1" | "localhost" | "::1")
}

async fn run_index_action(
    cli: &Cli,
    action: &IndexAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        IndexAction::Build { depth } => run_index_build(cli, (*depth).into()).await,
        IndexAction::Status => run_index_status(cli).await,
        IndexAction::Refresh => run_index_refresh(cli).await,
        IndexAction::Clear { all } => run_index_clear(cli, *all).await,
    }
}

fn index_ids(resolved: &ResolvedConnection) -> (String, String) {
    let database = resolved
        .params
        .dbname
        .clone()
        .unwrap_or_else(|| "postgres".into());
    let connection_id = resolved
        .profile_name
        .clone()
        .unwrap_or_else(|| "default".into());
    (connection_id, database)
}

fn default_build_request(
    connection_id: String,
    database: String,
    depth: BuildDepth,
    embeddings: bool,
) -> BuildRequest {
    BuildRequest {
        connection_id,
        database,
        scope: IndexScope {
            included_schemas: vec![],
            excluded_objects: vec![],
            pii_excluded_columns: vec![],
        },
        depth,
        build_mode: BuildMode::Guided,
        environment: "development".into(),
        embeddings,
    }
}

async fn run_build_request(
    resolved: &ResolvedConnection,
    req: BuildRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection_id = req.connection_id.clone();
    let database = req.database.clone();
    let client = connect_once(&resolved.params).await?;
    let store = IndexStore::new(default_index_root());
    let db = PgCatalogDb::new(&client);

    let mut on_progress = |ev: nexql_index::BuildProgress| {
        eprintln!("index build: {ev:?}");
    };

    #[cfg(feature = "embeddings")]
    let owned = if req.embeddings {
        Some(nexql_index::MiniLmEmbedder::load()?)
    } else {
        None
    };
    #[cfg(feature = "embeddings")]
    let embedder = owned.as_ref().map(|e| e as &dyn nexql_index::Embedder);
    #[cfg(not(feature = "embeddings"))]
    let embedder: Option<&dyn nexql_index::Embedder> = None;

    let manifest = build_index(&store, &db, &req, Some(&mut on_progress), None, embedder).await?;
    eprintln!(
        "index built: conn={connection_id} db={database} tables={} views={} functions={} enums={} shards={} fingerprint={} ({}ms, {} queries)",
        manifest.counts.tables,
        manifest.counts.views,
        manifest.counts.functions,
        manifest.counts.enums,
        manifest.shards.len(),
        manifest.schema_fingerprint,
        manifest.stats.build_ms,
        manifest.stats.queries_run,
    );
    if !manifest.stats.warnings.is_empty() {
        for w in &manifest.stats.warnings {
            eprintln!("warning: {w}");
        }
    }
    Ok(())
}

async fn run_index_build(cli: &Cli, depth: BuildDepth) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve(&resolve_inputs(cli))?;
    let (connection_id, database) = index_ids(&resolved);
    let req = default_build_request(connection_id, database, depth, cli.embeddings.is_local());
    run_build_request(&resolved, req).await
}

async fn run_index_status(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let store = IndexStore::new(default_index_root());
    let indexed = store.list_indexed_databases()?;
    if indexed.is_empty() {
        eprintln!("index status: no indexes under {}", store.root().display());
        return Ok(());
    }

    // Optional live fingerprint when a connection can be resolved + opened.
    let live: Option<(String, String, String)> = match resolve(&resolve_inputs(cli)) {
        Ok(resolved) => {
            let (connection_id, database) = index_ids(&resolved);
            match connect_once(&resolved.params).await {
                Ok(client) => {
                    let db = PgCatalogDb::new(&client);
                    match db.schema_fingerprint().await {
                        Ok(fp) => Some((connection_id, database, fp)),
                        Err(e) => {
                            eprintln!("index status: live fingerprint unavailable: {e}");
                            None
                        }
                    }
                }
                Err(e) => {
                    eprintln!("index status: connect skipped: {e}");
                    None
                }
            }
        }
        Err(_) => None,
    };

    for (conn_id, database) in &indexed {
        let base = store.base_dir(conn_id, database);
        let Some(manifest) = store.read_manifest(&base)? else {
            continue;
        };
        let drift = live.as_ref().and_then(|(live_conn, live_db, fp)| {
            if live_conn == &manifest.connection_id && live_db == &manifest.database {
                Some(fp != &manifest.schema_fingerprint)
            } else {
                None
            }
        });
        let drift_part = match drift {
            Some(d) => format!(" drift={d}"),
            None => String::new(),
        };
        eprintln!(
            "index: connectionId={} database={} indexed_at={} fingerprint={} tables={} views={} functions={} enums={} build_ms={}{drift_part}",
            manifest.connection_id,
            manifest.database,
            manifest.indexed_at,
            manifest.schema_fingerprint,
            manifest.counts.tables,
            manifest.counts.views,
            manifest.counts.functions,
            manifest.counts.enums,
            manifest.stats.build_ms,
        );
    }
    Ok(())
}

async fn run_index_refresh(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve(&resolve_inputs(cli))?;
    let (connection_id, database) = index_ids(&resolved);
    let store = IndexStore::new(default_index_root());
    let base = store.base_dir(&connection_id, &database);
    let req = match store.read_manifest(&base)? {
        Some(manifest) => {
            eprintln!(
                "index refresh: reusing scope/depth/mode from manifest (indexed_at={})",
                manifest.indexed_at
            );
            BuildRequest {
                connection_id,
                database,
                scope: manifest.scope,
                depth: manifest.build_depth,
                build_mode: manifest.build_mode,
                environment: manifest.environment,
                embeddings: cli.embeddings.is_local(),
            }
        }
        None => {
            eprintln!("index refresh: no manifest; using build defaults");
            default_build_request(
                connection_id,
                database,
                BuildDepth::Structure,
                cli.embeddings.is_local(),
            )
        }
    };
    run_build_request(&resolved, req).await
}

async fn run_index_clear(cli: &Cli, all: bool) -> Result<(), Box<dyn std::error::Error>> {
    let store = IndexStore::new(default_index_root());
    if all {
        let indexed = store.list_indexed_databases()?;
        if indexed.is_empty() {
            eprintln!("index clear: nothing to clear");
            return Ok(());
        }
        for (conn_id, database) in indexed {
            store.clear_index(&conn_id, &database)?;
            eprintln!("index cleared: conn={conn_id} db={database}");
        }
        return Ok(());
    }

    let resolved = resolve(&resolve_inputs(cli))?;
    let (connection_id, database) = index_ids(&resolved);
    store.clear_index(&connection_id, &database)?;
    eprintln!("index cleared: conn={connection_id} db={database}");
    Ok(())
}

fn profile_config_path(cli: &Cli) -> Result<PathBuf, Box<dyn std::error::Error>> {
    cli.connection
        .config
        .clone()
        .or_else(ConfigFile::default_path)
        .ok_or_else(|| "could not resolve a config path — set $HOME or $NEXQL_MCP_CONFIG".into())
}

fn emit_secret_migration_report(report: &nexql_conn::SecretMigrationReport) {
    for name in &report.migrated {
        eprintln!("✓ migrated plaintext credentials for profile '{name}' to OS keyring");
    }
    for (name, err) in &report.failed {
        eprintln!(
            "warning: could not migrate profile '{name}' ({err}) — plaintext password retained; \
             use password_command or password_file if keyring is unavailable"
        );
    }
    if let Some(ref backup) = report.backup {
        eprintln!(
            "  (previous config backed up to {})",
            backup.display()
        );
    }
}

fn load_profile_config(cli: &Cli) -> Result<(PathBuf, ConfigFile), Box<dyn std::error::Error>> {
    let path = profile_config_path(cli)?;
    let (config, report) = ConfigFile::load_path_migrated(&path)?;
    if report.any_changes() {
        emit_secret_migration_report(&report);
    }
    Ok((path, config))
}

async fn run_profile_action(
    cli: &Cli,
    action: &ProfileAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ProfileAction::List => {
            let (path, config) = load_profile_config(cli)?;
            if config.profiles.is_empty() {
                println!("no profiles under {} — try `nexql-mcp tui`", path.display());
                return Ok(());
            }
            let mut names: Vec<&String> = config.profiles.keys().collect();
            names.sort();
            for name in names {
                let marker = if config.default_profile.as_deref() == Some(name.as_str()) {
                    " (default)"
                } else {
                    ""
                };
                println!("{name}{marker}");
            }
            Ok(())
        }
        ProfileAction::Add {
            name,
            url,
            host,
            port,
            dbname,
            user,
            password,
            password_command,
            password_file,
            credential_provider,
            access_mode,
            set_default,
            no_test,
        } => {
            let profile = nexql_conn::ProfileConfig {
                url: url.clone(),
                host: host.clone(),
                port: *port,
                dbname: dbname.clone(),
                user: user.clone(),
                password: password.clone(),
                password_command: password_command.clone(),
                password_file: password_file.clone(),
                credential_provider: credential_provider.clone(),
                access_mode: access_mode.clone(),
                ..Default::default()
            };
            if !*no_test {
                let params = nexql_conn::resolve_profile(&profile)?;
                let report = nexql_conn::test_connection(&params).await?;
                println!(
                    "✓ Connection verified: {} (latency: {:.0}ms)",
                    report.server_version,
                    report.latency.as_secs_f64() * 1000.0
                );
            }
            let (config_path, mut config) = load_profile_config(cli)?;
            if *set_default {
                config.default_profile = Some(name.clone());
            }
            config.upsert_profile_prepared(name.clone(), profile)?;
            let backup = config.save(&config_path)?;
            println!("✓ Profile '{name}' saved to {}", config_path.display());
            if let Some(b) = backup {
                println!("  (previous config backed up to {})", b.display());
            }
            Ok(())
        }
        ProfileAction::SetPassword { name } => {
            let (path, mut config) = load_profile_config(cli)?;
            let mut profile = config
                .profiles
                .get(name)
                .cloned()
                .ok_or_else(|| format!("profile not found: {name}"))?;
            eprint!("password for '{name}' (visible — not hidden input): ");
            use std::io::Write;
            std::io::stderr().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            let password = line.trim_end_matches(['\r', '\n']).to_string();
            nexql_conn::store_keyring_password(name, &password)?;
            profile.password = None;
            profile.credential_provider = Some("keyring".into());
            config.upsert_profile(name.clone(), profile);
            config.save(&path)?;
            println!("password updated for '{name}' (stored in OS keyring)");
            Ok(())
        }
        ProfileAction::Test { name } => {
            let (_path, config) = load_profile_config(cli)?;
            let profile_name = name
                .clone()
                .or_else(|| config.default_profile.clone())
                .ok_or("no profile name given and no default_profile set")?;
            let params = if let Some(profile) = config.profiles.get(&profile_name) {
                nexql_conn::resolve_profile(profile)?
            } else {
                return Err(format!("profile not found: {profile_name}").into());
            };
            let report = nexql_conn::test_connection(&params).await?;
            println!(
                "connected in {:.0}ms: {} (superuser={})",
                report.latency.as_secs_f64() * 1000.0,
                report.server_version,
                report.is_superuser
            );
            Ok(())
        }
        ProfileAction::Export { format, output } => {
            let (_path, config) = load_profile_config(cli)?;
            let toml_str = if format == "full" {
                config.export_full_sanitized().to_toml_string()?
            } else {
                let proj = config.export_shareable();
                toml::to_string_pretty(&proj)
                    .map_err(|e| nexql_conn::ConnError::Config(e.to_string()))?
            };
            if let Some(out_path) = output {
                std::fs::write(out_path, &toml_str)?;
                println!(
                    "✓ Exported secret-sanitized config to {}",
                    out_path.display()
                );
            } else {
                println!("{toml_str}");
            }
            Ok(())
        }
        ProfileAction::Import { path } => {
            let (dest_path, mut config) = load_profile_config(cli)?;
            let raw = std::fs::read_to_string(path)?;
            let imported: nexql_conn::ConfigFile = toml::from_str(&raw).map_err(|e| {
                nexql_conn::ConnError::Config(format!(
                    "failed to parse TOML from {}: {e}",
                    path.display()
                ))
            })?;
            let mut count = 0;
            for (name, prof) in imported.profiles {
                let prepared = nexql_conn::prepare_profile_for_persist(&name, prof)?;
                config.upsert_profile(name, prepared);
                count += 1;
            }
            if imported.default_profile.is_some() {
                config.default_profile = imported.default_profile;
            }
            let backup = config.save(&dest_path)?;
            println!("✓ Imported {count} profile(s) into {}", dest_path.display());
            if let Some(b) = backup {
                println!("  (previous config backed up to {})", b.display());
            }
            Ok(())
        }
        ProfileAction::MigrateSecrets => {
            let path = profile_config_path(cli)?;
            if !path.exists() {
                println!("no config at {} — nothing to migrate", path.display());
                return Ok(());
            }
            let mut config = ConfigFile::load_path(&path)?;
            let report = nexql_conn::migrate_plaintext_secrets(&mut config);
            if !report.migrated.is_empty() {
                let backup = config.save(&path)?;
                let mut report = report;
                report.backup = backup;
                emit_secret_migration_report(&report);
                println!("✓ secret migration complete for {}", path.display());
            } else if report.failed.is_empty() {
                println!("no plaintext credentials found in {}", path.display());
            } else {
                emit_secret_migration_report(&report);
                eprintln!("secret migration failed — see warnings above");
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

async fn run_doctor(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mode: AccessMode = cli.access.access_mode.parse()?;
    let resolved = resolve(&resolve_inputs(cli))?;
    println!(
        "resolved source={:?} profile={:?}",
        resolved.source, resolved.profile_name
    );

    let client = connect_once(&resolved.params).await?;
    let version: String = client.query_one("SELECT version()", &[]).await?.get(0);
    println!(
        "connected: {}",
        version.split(',').next().unwrap_or(&version)
    );

    let row = client
        .query_one("SELECT current_setting('is_superuser')", &[])
        .await?;
    let is_super: String = row.get(0);
    let is_superuser = is_super.eq_ignore_ascii_case("on");
    check_superuser_guard(mode, is_superuser, cli.access.i_know_what_im_doing)?;
    println!("access_mode={mode:?} superuser={is_superuser}");

    let opts = PoolOptions::default();
    apply_session_guards(&client, &opts).await?;
    let ro: String = client
        .query_one("SHOW default_transaction_read_only", &[])
        .await?
        .get(0);
    let timeout: String = client
        .query_one("SHOW statement_timeout", &[])
        .await?
        .get(0);
    println!("session default_transaction_read_only={ro} statement_timeout={timeout}");

    // Best-effort catalog grants (non-fatal).
    match client
        .query_one("SELECT 1 FROM information_schema.tables LIMIT 1", &[])
        .await
    {
        Ok(_) => println!("grants: can SELECT information_schema.tables"),
        Err(e) => eprintln!("warning: cannot SELECT information_schema ({e})"),
    }

    // Best-effort pg_stat_statements presence (non-fatal).
    match client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_stat_statements')",
            &[],
        )
        .await
    {
        Ok(row) => {
            let present: bool = row.get(0);
            if present {
                println!("pg_stat_statements: present");
            } else {
                eprintln!(
                    "warning: pg_stat_statements extension not installed (slow_queries / index tools degraded)"
                );
            }
        }
        Err(e) => eprintln!("warning: could not check pg_stat_statements ({e})"),
    }

    // Best-effort local index freshness for this connection.
    let (connection_id, database) = index_ids(&resolved);
    let store = IndexStore::new(default_index_root());
    let base = store.base_dir(&connection_id, &database);
    match store.read_manifest(&base) {
        Ok(Some(manifest)) => {
            println!(
                "index: present indexed_at={} fingerprint={} (conn={} db={})",
                manifest.indexed_at,
                manifest.schema_fingerprint,
                manifest.connection_id,
                manifest.database
            );
        }
        Ok(None) => {
            eprintln!(
                "warning: no local index for conn={connection_id} db={database} under {} — run `nexql-mcp index build`",
                store.root().display()
            );
        }
        Err(e) => eprintln!("warning: index manifest read failed ({e})"),
    }

    // Warn about plaintext credentials in saved profiles (non-fatal).
    if let Ok((_, config)) = load_profile_config(cli) {
        for warning in nexql_conn::config_plaintext_secret_warnings(&config) {
            eprintln!("warning: {warning}");
        }
    }

    println!("doctor: ok");
    Ok(())
}

async fn build_session(cli: &Cli) -> Result<Arc<ToolSession>, Box<dyn std::error::Error>> {
    migrate_legacy_secrets(cli);
    let cli_mode: AccessMode = if cli.access.managed_extension {
        AccessMode::Read
    } else {
        cli.access.access_mode.parse()?
    };
    let resolved_list = nexql_conn::resolve_all(&resolve_inputs(cli))?;
    let mut default_caps = PolicyCaps::default();
    if let Some(n) = cli.access.max_rows {
        default_caps = default_caps.with_max_rows(n);
    }

    let active_id = resolved_list.first().and_then(|r| r.profile_name.clone());
    let connections: Vec<nexql_tools::ConnectionInfo> = resolved_list
        .iter()
        .map(|r| {
            let id = r.profile_name.clone().unwrap_or_else(|| "default".into());
            let mut policy =
                policy_from_profile(r.profile.as_ref(), cli_mode, default_caps.clone());
            if cli.access.managed_extension {
                policy.access_mode = AccessMode::Read;
            }
            nexql_tools::ConnectionInfo {
                id: id.clone(),
                name: id,
                host: r.params.host.clone(),
                port: r.params.port,
                database: r.params.dbname.clone(),
                params: r.params.clone(),
                policy,
            }
        })
        .collect();

    let session = ToolSession::from_connections(connections, active_id).await?;
    Ok(session)
}

async fn run_query_cmd(
    cli: &Cli,
    sql: &str,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = build_session(cli).await?;
    let router = ToolRouter::new(session);
    let outcome = router.call("run_select", json!({ "sql": sql })).await;
    if outcome.is_error {
        return Err(outcome.text.into());
    }

    match format {
        OutputFormat::Json => {
            if let Some(val) = outcome.structured {
                println!("{}", serde_json::to_string_pretty(&val)?);
            } else {
                println!("{}", outcome.text);
            }
        }
        OutputFormat::Csv => {
            if let Some(val) = outcome.structured {
                let empty = Vec::new();
                let rows = val.get("rows").and_then(|v| v.as_array()).unwrap_or(&empty);
                if !rows.is_empty() {
                    let mut keys = Vec::new();
                    if let Some(obj) = rows[0].as_object() {
                        keys = obj.keys().cloned().collect();
                        println!("{}", keys.join(","));
                    }
                    for row in rows {
                        if let Some(obj) = row.as_object() {
                            let vals: Vec<String> = keys
                                .iter()
                                .map(|k| {
                                    obj.get(k)
                                        .map(|v| v.to_string().replace('"', "\"\""))
                                        .unwrap_or_default()
                                })
                                .collect();
                            println!("{}", vals.join(","));
                        }
                    }
                }
            } else {
                println!("{}", outcome.text);
            }
        }
        OutputFormat::Table => {
            if let Some(val) = outcome.structured {
                let empty = Vec::new();
                let rows = val.get("rows").and_then(|v| v.as_array()).unwrap_or(&empty);
                if rows.is_empty() {
                    println!("(0 rows)");
                } else {
                    print_ascii_table(rows);
                }
            } else {
                println!("{}", outcome.text);
            }
        }
    }
    Ok(())
}

async fn run_diff_cmd(
    cli: &Cli,
    source_schema: &str,
    target_schema: &str,
    migration: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = build_session(cli).await?;
    let router = ToolRouter::new(session);

    if migration {
        let outcome = router
            .call(
                "generate_migration",
                json!({
                    "sourceSchema": source_schema,
                    "targetSchema": target_schema
                }),
            )
            .await;
        if outcome.is_error {
            return Err(outcome.text.into());
        }
        println!("{}", outcome.text);
    } else {
        let outcome = router
            .call(
                "schema_diff",
                json!({
                    "sourceSchema": source_schema,
                    "targetSchema": target_schema
                }),
            )
            .await;
        if outcome.is_error {
            return Err(outcome.text.into());
        }
        if let Some(val) = outcome.structured {
            println!("{}", serde_json::to_string_pretty(&val)?);
        } else {
            println!("{}", outcome.text);
        }
    }
    Ok(())
}

fn print_ascii_table(rows: &[Value]) {
    if rows.is_empty() {
        return;
    }
    let mut cols: Vec<String> = Vec::new();
    if let Some(first) = rows[0].as_object() {
        cols = first.keys().cloned().collect();
    }
    if cols.is_empty() {
        return;
    }

    let mut col_widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();
    let mut grid: Vec<Vec<String>> = Vec::new();

    for row in rows {
        let mut row_strs = Vec::new();
        if let Some(obj) = row.as_object() {
            for (idx, col) in cols.iter().enumerate() {
                let val_str = match obj.get(col) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => "NULL".to_string(),
                    Some(other) => other.to_string(),
                };
                col_widths[idx] = col_widths[idx].max(val_str.len());
                row_strs.push(val_str);
            }
        }
        grid.push(row_strs);
    }

    let header = cols
        .iter()
        .zip(&col_widths)
        .map(|(c, w)| format!("{:<width$}", c, width = w))
        .collect::<Vec<_>>()
        .join(" | ");
    let separator = col_widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("-+-");

    println!("{header}");
    println!("{separator}");
    for row in grid {
        let line = row
            .iter()
            .zip(&col_widths)
            .map(|(v, w)| format!("{:<width$}", v, width = w))
            .collect::<Vec<_>>()
            .join(" | ");
        println!("{line}");
    }
    println!("({} rows)", rows.len());
}

#[cfg(test)]
mod main_cli_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_subcommand_query_parse() {
        let cli =
            Cli::try_parse_from(["nexql-mcp", "query", "SELECT 1", "--format", "json"]).unwrap();
        match cli.command {
            Some(Commands::Query { sql, format }) => {
                assert_eq!(sql, "SELECT 1");
                assert_eq!(format, OutputFormat::Json);
            }
            _ => panic!("Expected Commands::Query"),
        }
    }

    #[test]
    fn cli_subcommand_diff_parse() {
        let cli =
            Cli::try_parse_from(["nexql-mcp", "diff", "public", "v2", "--migration"]).unwrap();
        match cli.command {
            Some(Commands::Diff {
                source_schema,
                target_schema,
                migration,
            }) => {
                assert_eq!(source_schema, "public");
                assert_eq!(target_schema, "v2");
                assert!(migration);
            }
            _ => panic!("Expected Commands::Diff"),
        }
    }

    #[test]
    fn cli_subcommand_init_optional_client_parse() {
        let cli = Cli::try_parse_from(["nexql-mcp", "init"]).unwrap();
        match cli.command {
            Some(Commands::Init { client, tui }) => {
                assert_eq!(client, None);
                assert!(!tui);
            }
            _ => panic!("Expected Commands::Init"),
        }

        let cli_with_client = Cli::try_parse_from(["nexql-mcp", "init", "claude"]).unwrap();
        match cli_with_client.command {
            Some(Commands::Init { client, tui }) => {
                assert_eq!(client.as_deref(), Some("claude"));
                assert!(!tui);
            }
            _ => panic!("Expected Commands::Init"),
        }

        let cli_tui = Cli::try_parse_from(["nexql-mcp", "init", "--tui"]).unwrap();
        match cli_tui.command {
            Some(Commands::Init { client, tui }) => {
                assert_eq!(client, None);
                assert!(tui);
            }
            _ => panic!("Expected Commands::Init"),
        }
    }

    #[test]
    fn cli_subcommand_onboarding_parse() {
        let cli = Cli::try_parse_from(["nexql-mcp", "onboarding"]).unwrap();
        match cli.command {
            Some(Commands::Onboarding) => {}
            _ => panic!("Expected Commands::Onboarding"),
        }
    }
}
