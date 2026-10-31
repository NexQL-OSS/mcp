// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! CLI entrypoint for the standalone NexQL Postgres MCP server.

mod client_targets;
mod init_clients;
mod launch;
mod password_page;
mod setup;
mod tui;
mod update_check;

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
use nexql_policy::{AccessMode, PolicyCaps, superuser_write_notice};
use nexql_proto::{
    ClientRequester, CompletionBackend, HttpAuth, HttpServer, McpHandler, PromptBackend,
    ResourceBackend, RpcFailure, StdioServer, ToolAnnotations, ToolBackend, ToolCallResult,
    ToolDescriptor,
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
    #[default]
    Query,
    Dba,
    Meta,
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

    /// Tool surface profile: query (core query/discovery), dba (monitoring/admin), full (all tools).
    /// Env: `NEXQL_MCP_TOOLS=query|dba|meta|full` (default query).
    #[arg(
        long,
        value_enum,
        default_value_t = CliToolProfile::Query,
        env = "NEXQL_MCP_TOOLS",
        long_help = format!(
            "Tool surface profile: query (core query/discovery, {} tools), dba (monitoring/admin), meta (minimal + discover_tools), full (all {} tools).\nWrite and admin tools are listed only when a connection's access mode allows them.\nEnv: `NEXQL_MCP_TOOLS=query|dba|meta|full` (default query).",
            nexql_tools::ToolName::QUERY_PROFILE.len(),
            nexql_tools::ToolName::ACTIVE.len(),
        )
    )]
    tools: CliToolProfile,

    /// Skip the once-a-day check for a newer nexql-mcp release. Also off with
    /// `update_check = false` in config.toml, `DO_NOT_TRACK=1`, or under CI.
    /// Env: `NEXQL_MCP_NO_UPDATE_CHECK`.
    #[arg(
        long,
        env = "NEXQL_MCP_NO_UPDATE_CHECK",
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    no_update_check: bool,

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
    /// read (default), write or admin. Env: `NEXQL_MCP_ACCESS_MODE`.
    #[arg(long, default_value = "read", env = "NEXQL_MCP_ACCESS_MODE")]
    access_mode: String,

    /// Same as `--access-mode write`, for installers that only offer an
    /// on/off switch (the Claude Desktop bundle). Env: `NEXQL_MCP_ALLOW_WRITES`.
    #[arg(long = "allow-writes")]
    allow_writes: bool,

    /// Only expose these schemas (comma-separated) on connections whose
    /// profile sets no `schemas`. Env: `NEXQL_MCP_SCHEMAS`.
    #[arg(long, value_delimiter = ',', env = "NEXQL_MCP_SCHEMAS")]
    schemas: Vec<String>,

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
        #[arg(help = init_clients::client_arg_help())]
        client: Option<String>,

        /// Force interactive TUI onboarding wizard.
        #[arg(long)]
        tui: bool,

        /// Write the merged config to the client's config file directly — no
        /// browser, no TTY, nothing to hand-parse. For AI coding agents
        /// setting this up on a user's behalf. A positional connection URL is
        /// saved as a profile first (password goes to the keyring, never the
        /// client JSON) and then wired. Without a URL, pass --profile or have
        /// a default profile. Only supported for clients with a real mergeable
        /// config file (i.e. not the copy/paste-only ones: continue, goose,
        /// opencode, jetbrains, openai-agents). Prints the diff applied and any backup path.
        #[arg(long)]
        apply: bool,
    },
    /// Interactive AI model onboarding & client configuration wizard TUI.
    Onboarding,
    /// Interactive profile editor + multi-client wiring.
    Tui,
    /// Browser-based profile setup and client wiring wizard.
    Setup {
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
        /// `0` picks an ephemeral port.
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Open the setup UI in the default browser.
        #[arg(long, default_value_t = true, action = clap::ArgAction::SetTrue)]
        #[arg(long = "no-open", action = clap::ArgAction::SetFalse)]
        open: bool,
        /// Shut down after this many seconds without API activity.
        #[arg(long = "idle-timeout", default_value_t = 900)]
        idle_timeout_secs: u64,
        /// Workspace root for project-scoped client configs (.cursor/mcp.json, etc.).
        #[arg(long = "workspace-root")]
        workspace_root: Option<PathBuf>,
        /// Profile name for non-interactive setup (required with `--dsn`).
        #[arg(long)]
        name: Option<String>,
        /// Connection string for non-interactive setup (`NEXQL_MCP_DSN` env).
        #[arg(long, env = "NEXQL_MCP_DSN")]
        dsn: Option<String>,
        /// Client to wire when using `--apply` (repeatable).
        #[arg(long, action = clap::ArgAction::Append)]
        client: Vec<String>,
        /// Write merged client config in-process (requires `--client`).
        #[arg(long)]
        apply: bool,
        /// Run headless pipeline and exit without starting the HTTP server.
        #[arg(long)]
        exit: bool,
        /// Skip connection probe during headless setup (CI / air-gapped).
        #[arg(long)]
        skip_test: bool,
    },
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
        /// Ends up in shell history; leave it out to be asked without the
        /// password showing.
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
    /// Print SQL that creates a least-privilege `nexql_readonly` login.
    ReadonlyRole,
}

struct RouterBackend {
    router: ToolRouter,
    /// Update notice for the first successful tool result (see `update_check`).
    update: Arc<update_check::PendingNotice>,
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
        let mut out = self.router.call(name, arguments).await;
        if !out.is_error
            && let Some((text, structured)) = self.update.take()
        {
            out.text
                .push_str(&format!("\n\n[nexql-mcp update: {text}]"));
            if let Some(obj) = out.structured.as_mut().and_then(Value::as_object_mut) {
                obj.insert("updateNotice".into(), structured);
            }
        }
        ToolCallResult {
            text: out.text,
            structured: out.structured,
            is_error: out.is_error,
        }
    }
}

/// Bridges `nexql_proto`'s `ClientRequester` (real MCP `elicitation/create`)
/// to `nexql_tools::ElicitationRequester` — the seam that lets `apply_ddl`'s
/// destructive-confirm gate ask the human directly instead of only accepting
/// an agent-settable `confirm_destructive` argument. Lives here (not in
/// `nexql-tools`) because `nexql-tools` must never depend on `nexql-proto`.
struct McpElicitationAdapter {
    handler: McpHandler,
}

#[async_trait]
impl nexql_tools::ElicitationRequester for McpElicitationAdapter {
    fn supports_elicitation(&self) -> bool {
        self.handler.client_capabilities().elicitation
    }

    async fn request_confirmation(
        &self,
        prompt: &str,
    ) -> Result<nexql_tools::ConfirmationOutcome, String> {
        // Minimal schema: MCP elicitation requires a `requestedSchema`, but
        // the decision here hinges on the response's `action`
        // (accept/decline/cancel), not the form content — a single boolean
        // is enough for a client that renders this as a yes/no prompt.
        let schema = json!({
            "type": "object",
            "properties": {
                "confirm": {
                    "type": "boolean",
                    "title": "Proceed?",
                }
            },
            "required": ["confirm"]
        });
        let result = self.handler.request_elicitation(prompt, schema).await?;
        let action = result.get("action").and_then(|v| v.as_str()).unwrap_or("");
        Ok(if action == "accept" {
            nexql_tools::ConfirmationOutcome::Confirmed
        } else {
            nexql_tools::ConfirmationOutcome::Declined
        })
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
            Commands::Init { client, tui, apply } => {
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
                if *apply {
                    if let Err(e) = run_init_apply(&cli, client_name) {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                } else if let Some(url) = cli.connection_string.as_deref() {
                    let snippet = if init_clients::url_contains_password(url) {
                        match persist_profile_from_url(&cli, url) {
                            Ok(name) => {
                                eprintln!(
                                    "saved profile '{name}' (password stored in the keyring or encrypted file, not in client config)"
                                );
                                init_clients::init_snippet_with_profile(client_name, &name)
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        init_clients::init_snippet(client_name, Some(url))
                    };
                    match snippet {
                        Ok(snippet) => println!("{snippet}"),
                        Err(e) => {
                            eprintln!("{e}");
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    match init_clients::init_snippet(client_name, None) {
                        Ok(snippet) => println!("{snippet}"),
                        Err(e) => {
                            eprintln!("{e}");
                            return ExitCode::FAILURE;
                        }
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
            Commands::Setup {
                bind,
                port,
                open,
                idle_timeout_secs,
                workspace_root,
                name,
                dsn,
                client,
                apply,
                exit,
                skip_test,
            } => {
                let config_path = match profile_config_path(&cli) {
                    Ok(path) => path,
                    Err(e) => {
                        eprintln!("setup error: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                let workspace = workspace_root
                    .clone()
                    .or_else(|| cli.connection.workspace_root.clone());

                let headless = dsn.is_some() || *exit;
                if headless {
                    let profile_name = match name.as_deref() {
                        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                        _ => {
                            eprintln!(
                                "setup error: --name is required for headless setup (with --dsn or --exit)"
                            );
                            return ExitCode::FAILURE;
                        }
                    };
                    let connection_dsn = match dsn.as_deref() {
                        Some(d) if !d.trim().is_empty() => d.trim().to_string(),
                        _ => {
                            eprintln!(
                                "setup error: headless setup requires --dsn or NEXQL_MCP_DSN"
                            );
                            return ExitCode::FAILURE;
                        }
                    };
                    return match setup::headless::run(setup::headless::HeadlessOptions {
                        name: profile_name,
                        dsn: connection_dsn,
                        clients: client.clone(),
                        apply: *apply,
                        skip_test: *skip_test,
                        config_path,
                        workspace_root: workspace,
                    })
                    .await
                    {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(e) => {
                            eprintln!("setup error: {e}");
                            ExitCode::FAILURE
                        }
                    };
                }

                return match setup::run(setup::SetupOptions {
                    bind: bind.clone(),
                    port: *port,
                    open_browser: *open,
                    idle_timeout_secs: *idle_timeout_secs,
                    config_path,
                    workspace_root: workspace,
                })
                .await
                {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("setup error: {e}");
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
    if let Ok(path) = profile_config_path(cli)
        && let Ok((_, report)) = ConfigFile::load_path_migrated(&path)
        && report.any_changes()
    {
        emit_secret_migration_report(&report);
    }
}

/// Most permissive access mode across the configured connections, or the CLI
/// mode when none is configured yet. Tools above it are left out of `tools/list`.
fn access_ceiling(cli_mode: AccessMode, connections: &[nexql_tools::ConnectionInfo]) -> AccessMode {
    let modes = connections.iter().map(|c| c.policy.access_mode);
    if connections.is_empty() {
        cli_mode
    } else if modes.clone().any(AccessMode::allows_admin) {
        AccessMode::Admin
    } else if modes.clone().any(AccessMode::allows_writes) {
        AccessMode::Write
    } else {
        AccessMode::Read
    }
}

/// Access mode from `--access-mode`, `--allow-writes` and `--managed-extension`.
fn cli_access_mode(access: &AccessArgs) -> Result<AccessMode, Box<dyn std::error::Error>> {
    if access.managed_extension {
        return Ok(AccessMode::Read);
    }
    let mode: AccessMode = access.access_mode.parse()?;
    let allow_writes = access.allow_writes || env_flag("NEXQL_MCP_ALLOW_WRITES");
    Ok(if allow_writes && mode == AccessMode::Read {
        AccessMode::Write
    } else {
        mode
    })
}

/// Read by hand rather than through clap: an installer sends "true", "false"
/// or an empty string, and clap rejects an empty value for a switch.
fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| is_truthy(&v))
}

fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// `--schemas` for a connection whose profile does not choose its own.
fn apply_cli_schemas(access: &AccessArgs, policy: &mut nexql_tools::ConnectionPolicy) {
    // An installer that leaves an optional field unset may pass its
    // placeholder through verbatim; never let that hide every schema.
    let schemas: Vec<String> = access
        .schemas
        .iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.contains("${"))
        .collect();
    if !schemas.is_empty() && policy.filter.allow_schemas.is_empty() {
        policy.filter.allow_schemas = schemas;
    }
}

/// Connections for a session. Launch settings (`--allow-writes`, `--schemas`
/// and their env vars) belong to the connection the launch named, a URL or
/// `DATABASE_URL`. Saved profiles loaded beside it keep their own settings
/// and default to read-only, so ticking "Allow changes" in an installer's
/// form for one database never opens up another. With no such connection the
/// launch settings apply to every profile, as before.
fn connection_infos(
    access: &AccessArgs,
    resolved: &[nexql_conn::ResolvedConnection],
    cli_mode: AccessMode,
    default_caps: &PolicyCaps,
) -> Vec<nexql_tools::ConnectionInfo> {
    let launched = resolved.iter().any(|r| r.profile_name.is_none());
    let launch_id = if resolved
        .iter()
        .any(|r| r.profile_name.as_deref() == Some("default"))
    {
        "database_url"
    } else {
        "default"
    };
    resolved
        .iter()
        .map(|r| {
            let id = r.profile_name.clone().unwrap_or_else(|| launch_id.into());
            let launch_settings = !launched || r.profile_name.is_none();
            let mode = if launch_settings {
                cli_mode
            } else {
                AccessMode::Read
            };
            let mut policy = policy_from_profile(r.profile.as_ref(), mode, default_caps.clone());
            if access.managed_extension {
                policy.access_mode = AccessMode::Read;
            }
            if launch_settings {
                apply_cli_schemas(access, &mut policy);
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
        .collect()
}

async fn build_mcp_handler(cli: &Cli) -> Result<McpHandler, Box<dyn std::error::Error>> {
    migrate_legacy_secrets(cli);
    let resolved_list = nexql_conn::resolve_all(&resolve_inputs(cli))?;
    let active_resolved = resolved_list.first();

    let cli_mode = cli_access_mode(&cli.access)?;

    let mut default_caps = PolicyCaps::default();
    if let Some(n) = cli.access.max_rows {
        default_caps = default_caps.with_max_rows(n);
    }

    let active_id = active_resolved.and_then(|r| r.profile_name.clone());
    let mut connections = connection_infos(&cli.access, &resolved_list, cli_mode, &default_caps);
    if let Some(n) = cli.access.max_rows {
        for c in &mut connections {
            c.policy.caps = c.policy.caps.clone().with_max_rows(n);
        }
    }

    let mut access_notice = None;
    if let Some(resolved) = active_resolved {
        let client = connect_once(&resolved.params).await?;
        let row = client
            .query_one("SELECT current_setting('is_superuser')", &[])
            .await?;
        let is_super: String = row.get(0);
        let is_superuser = is_super.eq_ignore_ascii_case("on");
        if let Some(active) = connections.first_mut()
            && let Some(notice) = superuser_write_notice(
                active.policy.access_mode,
                is_superuser,
                cli.access.i_know_what_im_doing,
            )
        {
            eprintln!("warning: {notice} (--i-know-what-im-doing overrides this)");
            active.policy.access_mode = AccessMode::Read;
            access_notice = Some((active.id.clone(), notice));
        }
    }

    let access_ceiling = access_ceiling(cli_mode, &connections);
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
            let key = format!("{connection_id}\0{database}");
            if let nexql_index::GateRole::Leader(lease) = session.index_builds.enter(&key).await {
                let resolved_for_index = resolved.clone();
                tokio::task::spawn_blocking(move || {
                    eprintln!(
                        "index: no schema index for {connection_id}/{database} — building automatically"
                    );
                    let handle = tokio::runtime::Handle::current();
                    let result = handle.block_on(run_build_request(&resolved_for_index, req));
                    if let Err(e) = &result {
                        eprintln!(
                            "warning: automatic index build failed for {connection_id}/{database}: {e}"
                        );
                    }
                    lease.finish(result.map(|_| ()).map_err(|e| e.to_string()));
                });
            }
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
        .with_access_ceiling(access_ceiling)
        .with_semantic(cli.embeddings.is_local(), embedder);
    if cli.access.managed_extension {
        router = router.with_managed_extension(true);
    }
    if let Some((connection_id, notice)) = access_notice {
        router = router.with_access_notice(connection_id, notice);
    }

    let (update_line, update) = if update_check_enabled(cli) {
        let remote = cli.transport.http && !is_loopback_bind(&cli.transport.bind);
        update_check::start(remote)
    } else {
        (None, Arc::default())
    };

    let tools = Arc::new(RouterBackend { router, update });

    let mut handler = McpHandler::new(tools)
        .with_prompts(Arc::new(StaticPromptBackend))
        .with_server_title("NexQL Postgres MCP");
    if let Some(line) = update_line {
        handler = handler.with_instructions_note(line);
    }

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

    // Wire real human-in-the-loop confirmation for destructive DDL
    // (apply_ddl's CRITICAL-risk gate) — `handler.clone()` is a cheap shared
    // handle (see McpHandler's Clone impl doc comment); it keeps working
    // once the transport loop calls `set_outbound_tx` on the original,
    // since both share the same underlying state.
    session.set_elicitation_backend(Arc::new(McpElicitationAdapter {
        handler: handler.clone(),
    }));

    // add_database asks for the password on a page on 127.0.0.1, so only
    // offer it where the human is on this machine: stdio, or HTTP on loopback.
    if !cli.transport.http || is_loopback_bind(&cli.transport.bind) {
        let open_browser = std::env::var("NEXQL_MCP_OPEN_BROWSER").map_or(true, |v| v != "0");
        session.set_password_page(Arc::new(password_page::LocalPasswordPage { open_browser }));
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

fn update_check_enabled(cli: &Cli) -> bool {
    let config_off = profile_config_path(cli)
        .ok()
        .and_then(|p| ConfigFile::load_path(&p).ok())
        .and_then(|c| c.update_check)
        == Some(false);
    update_check::enabled(cli.no_update_check, config_off)
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

/// Save `url` as a profile. A password in the URL is stored via
/// [`nexql_conn::prepare_profile_for_persist`] (keyring, or encrypted file)
/// and is not left in the TOML.
///
/// Re-running with the same target updates the saved connection but keeps
/// that profile's policy (`pii_columns`, `deny_tables`, `access_mode`, …). A
/// different target that would collide on the derived name gets a suffix
/// instead of overwriting it. An unreadable config is an error, never reset.
fn persist_profile_from_url(cli: &Cli, url: &str) -> Result<String, String> {
    let params = nexql_conn::params_from_url(url).map_err(|e| {
        format!(
            "{e}. Percent-encode reserved characters in the password \
             (for example # as %23, / as %2F, ? as %3F) and retry."
        )
    })?;
    let user = params.user.clone().unwrap_or_else(|| "nexql".into());
    let db = params.dbname.clone().unwrap_or_else(|| "postgres".into());
    let mut base: String = format!("{user}_{db}")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    if base.is_empty() || base.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        base.insert_str(0, "p_");
    }

    let path = profile_config_path(cli).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut config = ConfigFile::load_path_migrated(&path)
        .map(|(c, _)| c)
        .map_err(|e| {
            format!(
                "could not read {} ({e}); fix or move it before saving a profile",
                path.display()
            )
        })?;

    let mut name = base.clone();
    let mut suffix = 2;
    let existing = loop {
        match config.profiles.get(&name) {
            None => break None,
            Some(profile) if profile_targets(profile, &params) => break Some(profile.clone()),
            Some(_) => {
                name = format!("{base}_{suffix}");
                suffix += 1;
            }
        }
    };

    let url_has_password = params.password.as_deref().is_some_and(|p| !p.is_empty());
    let profile = match existing {
        Some(mut kept) => {
            kept.url = Some(url.to_owned());
            kept.host = None;
            kept.port = None;
            kept.dbname = None;
            kept.user = None;
            if url_has_password {
                kept.password = None;
                kept.password_command = None;
                kept.password_file = None;
                kept.credential_provider = None;
            }
            kept
        }
        None => nexql_conn::ProfileConfig {
            url: Some(url.to_owned()),
            ..Default::default()
        },
    };
    let prepared =
        nexql_conn::prepare_profile_for_persist(&name, profile).map_err(|e| e.to_string())?;
    if prepared.password.is_some()
        || prepared
            .url
            .as_deref()
            .is_some_and(init_clients::url_contains_password)
    {
        return Err(format!(
            "refusing to save profile '{name}': the password could not be moved out of the config"
        ));
    }
    // `upsert_profile` also does this; set it here so the first saved URL
    // stays the default even if that helper changes.
    if config.default_profile.is_none() {
        config.default_profile = Some(name.clone());
    }
    config.upsert_profile(name.clone(), prepared);
    config.save(&path).map_err(|e| e.to_string())?;

    // Re-read what landed on disk and check the saved fields. A substring
    // search of the raw TOML for the password misfires whenever the password
    // also appears in the URL, user, or database name (`postgres:postgres`).
    let saved = ConfigFile::load_path(&path).map_err(|e| e.to_string())?;
    if saved.profiles.get(&name).is_some_and(|profile| {
        profile.password.is_some()
            || profile
                .url
                .as_deref()
                .is_some_and(init_clients::url_contains_password)
    }) {
        return Err(format!(
            "refusing to save profile '{name}': password was written to {}",
            path.display()
        ));
    }
    Ok(name)
}

/// True when `profile` already points at the same server, database, and
/// user as `params`, so saving `params` again should update it in place.
fn profile_targets(
    profile: &nexql_conn::ProfileConfig,
    params: &nexql_conn::ConnectionParams,
) -> bool {
    let (host, port, dbname, user) = match profile
        .url
        .as_deref()
        .and_then(|url| nexql_conn::params_from_url(url).ok())
    {
        Some(saved) => (saved.host, saved.port, saved.dbname, saved.user),
        None => (
            profile.host.clone(),
            profile.port,
            profile.dbname.clone(),
            profile.user.clone(),
        ),
    };
    host == params.host
        && port.unwrap_or(5432) == params.port.unwrap_or(5432)
        && dbname == params.dbname
        && user == params.user
}

/// `nexql-mcp init <client> --apply` — reuses the same merge machinery
/// `nexql-mcp setup`'s HTTP API (`setup::wire::preview_wire`/`apply_wire`)
/// already exercises, in-process instead of over HTTP. Turns "hand-parse and
/// merge JSON" into one non-interactive, no-browser, no-TTY command — the
/// path an AI coding agent should use to set this up on a user's behalf.
fn run_init_apply(cli: &Cli, client_name: &str) -> Result<(), String> {
    let key = client_name.trim().to_ascii_lowercase();
    // "claude" is init_snippet's copy-paste alias for Claude Desktop; the
    // mergeable target is keyed "claude-desktop".
    let key = if key == "claude" {
        "claude-desktop".to_string()
    } else {
        key
    };

    let targets = client_targets::mergeable_targets();
    if !targets.iter().any(|t| t.key == key.as_str()) {
        let mergeable_list = targets.iter().map(|t| t.key).collect::<Vec<_>>().join(", ");
        return Err(format!(
            "--apply is only supported for clients with a mergeable config file: {mergeable_list}. \
             \"{client_name}\" is copy/paste-only — run `nexql-mcp init {client_name}` (without --apply) to print its snippet."
        ));
    }

    let created_from_url = if let Some(url) = cli.connection_string.as_deref() {
        Some(persist_profile_from_url(cli, url)?)
    } else {
        None
    };

    let config_path = profile_config_path(cli).map_err(|e| e.to_string())?;
    let config = ConfigFile::load_path_migrated(&config_path)
        .map(|(c, _)| c)
        .unwrap_or_default();

    let profile_names: Vec<String> = if !cli.connection.profiles.is_empty() {
        cli.connection.profiles.clone()
    } else if let Some(name) = created_from_url {
        vec![name]
    } else if let Some(ref default) = config.default_profile {
        vec![default.clone()]
    } else {
        return Err(
            "no saved profile to wire — pass a postgres URL (a profile is created \
             automatically) or --profile <name>."
                .into(),
        );
    };
    for name in &profile_names {
        if !config.profiles.contains_key(name) {
            return Err(format!(
                "profile \"{name}\" not found in {}",
                config_path.display()
            ));
        }
    }

    let launch = setup::wire::LaunchConfig::default();
    let preview = setup::wire::preview_wire(
        &config,
        &profile_names,
        std::slice::from_ref(&key),
        &[],
        &launch,
        cli.connection.workspace_root.as_deref(),
    );
    let results = setup::wire::apply_wire(&preview, std::slice::from_ref(&key));

    let mut had_error = false;
    for r in &results {
        if let Some(ref err) = r.error {
            had_error = true;
            eprintln!("✗ {}: {err}", r.display_name);
            continue;
        }
        if r.applied {
            let path = r.config_path.as_deref().unwrap_or("?");
            println!("✓ wrote {} ({path})", r.display_name);
            if let Some(ref backup) = r.backup {
                println!("  (previous config backed up to {backup})");
            }
        } else if r.skipped {
            println!("- skipped {}", r.display_name);
        }
    }
    if had_error {
        return Err(format!("failed to apply config for \"{client_name}\""));
    }
    Ok(())
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
        eprintln!("  (previous config backed up to {})", backup.display());
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

/// Read a password from the terminal without echoing it.
fn read_hidden(prompt: &str) -> std::io::Result<String> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, read};
    use std::io::Write;
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    crossterm::terminal::enable_raw_mode()?;
    let mut password = String::new();
    let outcome = loop {
        match read() {
            Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "cancelled",
                    ));
                }
                KeyCode::Backspace => {
                    password.pop();
                }
                KeyCode::Char(c) => password.push(c),
                _ => {}
            },
            Ok(Event::Paste(text)) => password.push_str(&text),
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();
    outcome.map(|()| password)
}

async fn run_profile_action(
    cli: &Cli,
    action: &ProfileAction,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::IsTerminal;
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
            // No password source given: ask for one at a terminal, hidden, so
            // it stays out of shell history and the screen.
            let url_has_password = url
                .as_deref()
                .and_then(|u| url::Url::parse(u).ok())
                .is_some_and(|u| u.password().is_some());
            let password = match password {
                Some(p) => Some(p.clone()),
                None if password_command.is_none()
                    && password_file.is_none()
                    && credential_provider.is_none()
                    && !url_has_password
                    && std::io::stdin().is_terminal() =>
                {
                    let typed = read_hidden(&format!(
                        "password for '{name}' (hidden; leave empty if none): "
                    ))?;
                    (!typed.is_empty()).then_some(typed)
                }
                None => None,
            };
            let profile = nexql_conn::ProfileConfig {
                url: url.clone(),
                host: host.clone(),
                port: *port,
                dbname: dbname.clone(),
                user: user.clone(),
                password,
                password_command: password_command.clone(),
                password_file: password_file.clone(),
                credential_provider: credential_provider.clone(),
                access_mode: access_mode.clone(),
                ..Default::default()
            };
            if !*no_test {
                let params = nexql_conn::resolve_profile(name, &profile)?;
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
            let password = if std::io::stdin().is_terminal() {
                read_hidden(&format!("password for '{name}' (hidden): "))?
            } else {
                // Piped in, e.g. from a password manager.
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                line.trim_end_matches(['\r', '\n']).to_string()
            };
            let stored = nexql_conn::store_profile_password(name, &password)?;
            profile.password = None;
            profile.credential_provider = Some(stored.provider.clone());
            profile.password_file = stored.password_file;
            config.upsert_profile(name.clone(), profile);
            config.save(&path)?;
            if stored.provider == nexql_conn::ENCRYPTED_FILE_PROVIDER {
                eprintln!("warning: {}", nexql_conn::encrypted_file_storage_warning());
                println!("password updated for '{name}' (stored in encrypted local file)");
            } else {
                println!("password updated for '{name}' (stored in OS keyring)");
            }
            Ok(())
        }
        ProfileAction::Test { name } => {
            let (_path, config) = load_profile_config(cli)?;
            let profile_name = name
                .clone()
                .or_else(|| config.default_profile.clone())
                .ok_or("no profile name given and no default_profile set")?;
            let params = if let Some(profile) = config.profiles.get(&profile_name) {
                nexql_conn::resolve_profile(&profile_name, profile)?
            } else {
                return Err(format!("profile not found: {profile_name}").into());
            };
            let report = nexql_conn::test_connection(&params).await?;
            if report.is_superuser {
                eprintln!("{}", nexql_policy::superuser_warning());
                eprintln!("{}", nexql_policy::readonly_role_sql());
            }
            let superuser = if report.is_superuser {
                "WARNING"
            } else {
                "false"
            };
            println!(
                "connected in {:.0}ms: {} (superuser={superuser})",
                report.latency.as_secs_f64() * 1000.0,
                report.server_version,
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
        ProfileAction::ReadonlyRole => {
            println!("{}", nexql_policy::readonly_role_sql());
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

/// Every user column (`schema`, `table`, `column`) visible to the role.
const USER_COLUMNS_SQL: &str = "SELECT n.nspname::text, c.relname::text, a.attname::text \
     FROM pg_attribute a \
     JOIN pg_class c ON c.oid = a.attrelid \
     JOIN pg_namespace n ON n.oid = c.relnamespace \
     WHERE c.relkind IN ('r', 'v', 'm', 'p', 'f') AND a.attnum > 0 AND NOT a.attisdropped \
       AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
       AND n.nspname NOT LIKE 'pg\\_%' \
     ORDER BY 1, 2, a.attnum";

/// `schema.table.column` for each column the profile masks by name or by
/// `pii_columns`, skipping tables the policy hides.
fn masked_columns(
    filter: &nexql_policy::PolicyFilter,
    columns: impl IntoIterator<Item = (String, String, String)>,
) -> Vec<String> {
    columns
        .into_iter()
        .filter(|(schema, table, column)| {
            let tables = [nexql_policy::ObjectRef::new(schema.clone(), table.clone())];
            filter.allows_table(schema, table)
                && nexql_policy::column_should_redact(filter, &tables, column)
        })
        .map(|(schema, table, column)| format!("{schema}.{table}.{column}"))
        .collect()
}

fn masked_columns_summary(filter: &nexql_policy::PolicyFilter, cols: &[String]) -> String {
    let personal = if filter.personal_data_heuristics {
        "on"
    } else {
        "off (personal_data_heuristics = false)"
    };
    if cols.is_empty() {
        return format!("masking: personal data {personal}; no columns masked");
    }
    const SHOWN: usize = 12;
    let mut out = format!(
        "masking: personal data {personal}; {} column(s) masked in results: {}",
        cols.len(),
        cols.iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    if cols.len() > SHOWN {
        out.push_str(&format!(", … and {} more", cols.len() - SHOWN));
    }
    out
}

async fn run_doctor(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if update_check_enabled(cli) {
        println!("{}", update_check::doctor_line().await);
    } else {
        println!("update: check disabled");
    }
    let mode = cli_access_mode(&cli.access)?;
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
    let notice = superuser_write_notice(mode, is_superuser, cli.access.i_know_what_im_doing);
    if let Some(notice) = &notice {
        eprintln!("warning: {notice} (--i-know-what-im-doing overrides this)");
    }
    let mode = if notice.is_some() {
        AccessMode::Read
    } else {
        mode
    };
    if is_superuser {
        eprintln!("{}", nexql_policy::superuser_warning());
        eprintln!("{}", nexql_policy::readonly_role_sql());
        println!("access_mode={mode:?} superuser=WARNING");
    } else {
        println!("access_mode={mode:?} superuser=false");
    }

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

    // Columns the name heuristics will mask (non-fatal).
    let mut policy = policy_from_profile(resolved.profile.as_ref(), mode, PolicyCaps::default());
    apply_cli_schemas(&cli.access, &mut policy);
    match client.query(USER_COLUMNS_SQL, &[]).await {
        Ok(rows) => {
            let cols = masked_columns(
                &policy.filter,
                rows.iter().map(|r| (r.get(0), r.get(1), r.get(2))),
            );
            println!("{}", masked_columns_summary(&policy.filter, &cols));
        }
        Err(e) => eprintln!("warning: could not list masked columns ({e})"),
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
    let cli_mode = cli_access_mode(&cli.access)?;
    let resolved_list = nexql_conn::resolve_all(&resolve_inputs(cli))?;
    let mut default_caps = PolicyCaps::default();
    if let Some(n) = cli.access.max_rows {
        default_caps = default_caps.with_max_rows(n);
    }

    let active_id = resolved_list.first().and_then(|r| r.profile_name.clone());
    let connections = connection_infos(&cli.access, &resolved_list, cli_mode, &default_caps);

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
    fn installer_booleans_parse() {
        for v in ["true", "TRUE", "1", "yes", " on "] {
            assert!(is_truthy(v), "{v}");
        }
        for v in ["false", "", "0", "no", "${user_config.allow_writes}"] {
            assert!(!is_truthy(v), "{v}");
        }
    }

    #[test]
    fn allow_writes_upgrades_read_but_not_managed_mode() {
        let mut access = AccessArgs {
            access_mode: "read".into(),
            allow_writes: true,
            ..Default::default()
        };
        assert_eq!(cli_access_mode(&access).unwrap(), AccessMode::Write);
        access.access_mode = "admin".into();
        assert_eq!(cli_access_mode(&access).unwrap(), AccessMode::Admin);
        access.managed_extension = true;
        assert_eq!(cli_access_mode(&access).unwrap(), AccessMode::Read);
    }

    fn resolved(profile: Option<&str>, dbname: &str) -> nexql_conn::ResolvedConnection {
        nexql_conn::ResolvedConnection {
            params: nexql_conn::ConnectionParams {
                host: Some("h".into()),
                dbname: Some(dbname.into()),
                ..Default::default()
            },
            source: if profile.is_some() {
                nexql_conn::ConnectionSource::Profile
            } else {
                nexql_conn::ConnectionSource::DatabaseUrl
            },
            profile_name: profile.map(str::to_owned),
            profile: profile.map(|_| nexql_conn::ProfileConfig::default()),
            project_config: None,
        }
    }

    #[test]
    fn launch_settings_stay_with_the_launch_connection() {
        let access = AccessArgs {
            schemas: vec!["sales".into()],
            ..Default::default()
        };
        let list = [resolved(None, "shop"), resolved(Some("billing"), "billing")];
        let conns = connection_infos(&access, &list, AccessMode::Write, &PolicyCaps::default());
        assert_eq!(conns[0].id, "default");
        assert_eq!(conns[0].policy.access_mode, AccessMode::Write);
        assert_eq!(conns[0].policy.filter.allow_schemas, ["sales"]);
        assert_eq!(conns[1].id, "billing");
        assert_eq!(conns[1].policy.access_mode, AccessMode::Read);
        assert!(conns[1].policy.filter.allow_schemas.is_empty());

        // Without a launch connection the settings apply to every profile.
        let only_profiles = [resolved(Some("billing"), "billing")];
        let conns = connection_infos(
            &access,
            &only_profiles,
            AccessMode::Write,
            &PolicyCaps::default(),
        );
        assert_eq!(conns[0].policy.access_mode, AccessMode::Write);
        assert_eq!(conns[0].policy.filter.allow_schemas, ["sales"]);
    }

    #[test]
    fn launch_connection_never_takes_a_saved_profiles_name() {
        let list = [resolved(None, "shop"), resolved(Some("default"), "other")];
        let conns = connection_infos(
            &AccessArgs::default(),
            &list,
            AccessMode::Read,
            &PolicyCaps::default(),
        );
        assert_eq!(conns[0].id, "database_url");
        assert_eq!(conns[1].id, "default");
    }

    #[test]
    fn cli_schemas_apply_only_without_profile_schemas() {
        let access = AccessArgs {
            schemas: vec![" sales ".into(), "".into(), "crm".into()],
            ..Default::default()
        };
        let mut policy = policy_from_profile(None, AccessMode::Read, PolicyCaps::default());
        apply_cli_schemas(&access, &mut policy);
        assert_eq!(policy.filter.allow_schemas, ["sales", "crm"]);

        let mut own = policy_from_profile(None, AccessMode::Read, PolicyCaps::default());
        own.filter.allow_schemas = vec!["public".into()];
        apply_cli_schemas(&access, &mut own);
        assert_eq!(own.filter.allow_schemas, ["public"]);

        // An unsubstituted installer placeholder is ignored.
        let placeholder = AccessArgs {
            schemas: vec!["${user_config.schemas}".into()],
            ..Default::default()
        };
        let mut policy = policy_from_profile(None, AccessMode::Read, PolicyCaps::default());
        apply_cli_schemas(&placeholder, &mut policy);
        assert!(policy.filter.allow_schemas.is_empty());
    }

    #[test]
    fn doctor_lists_masked_columns_and_respects_policy() {
        let col = |s: &str, t: &str, c: &str| (s.to_owned(), t.to_owned(), c.to_owned());
        let columns = vec![
            col("customers", "users", "email"),
            col("customers", "users", "first_name"),
            col("customers", "users", "id"),
            col("auth", "sessions", "token"),
            col("catalog", "products", "name"),
            col("billing", "invoices", "note"),
        ];
        let filter = nexql_policy::PolicyFilter {
            deny_tables: vec!["auth.*".into()],
            pii_columns: vec!["billing.invoices.note".into()],
            ..Default::default()
        };
        let masked = masked_columns(&filter, columns.clone());
        assert_eq!(
            masked,
            vec![
                "customers.users.email",
                "customers.users.first_name",
                "billing.invoices.note"
            ]
        );
        let summary = masked_columns_summary(&filter, &masked);
        assert!(
            summary.starts_with("masking: personal data on; 3 column(s)"),
            "{summary}"
        );

        let off = nexql_policy::PolicyFilter {
            personal_data_heuristics: false,
            ..Default::default()
        };
        let masked = masked_columns(&off, columns);
        assert_eq!(masked, vec!["auth.sessions.token"]);
        assert!(masked_columns_summary(&off, &masked).contains("off (personal_data_heuristics"));
    }

    /// Guards the `--tools` help text against drifting from the real active tool
    /// surface count — this class of drift has regressed once before (see
    /// `nexql-tools/src/registry.rs`'s `docs_tools_readme_matches_active_surface`).
    #[test]
    fn cli_help_tool_count_matches_active_surface() {
        use clap::CommandFactory;
        let command = Cli::command();
        let tools_arg = command
            .get_arguments()
            .find(|a| a.get_id() == "tools")
            .expect("--tools arg must exist");
        let help = tools_arg
            .get_long_help()
            .or_else(|| tools_arg.get_help())
            .expect("--tools must have help text")
            .to_string();
        let expected = format!("all {} tools", nexql_tools::ToolName::ACTIVE.len());
        assert!(
            help.contains(&expected),
            "--tools help text is stale: expected to contain {expected:?}, got {help:?}"
        );
    }

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
            Some(Commands::Init { client, tui, apply }) => {
                assert_eq!(client, None);
                assert!(!tui);
                assert!(!apply);
            }
            _ => panic!("Expected Commands::Init"),
        }

        let cli_with_client = Cli::try_parse_from(["nexql-mcp", "init", "claude"]).unwrap();
        match cli_with_client.command {
            Some(Commands::Init { client, tui, apply }) => {
                assert_eq!(client.as_deref(), Some("claude"));
                assert!(!tui);
                assert!(!apply);
            }
            _ => panic!("Expected Commands::Init"),
        }

        let cli_tui = Cli::try_parse_from(["nexql-mcp", "init", "--tui"]).unwrap();
        match cli_tui.command {
            Some(Commands::Init { client, tui, apply }) => {
                assert_eq!(client, None);
                assert!(tui);
                assert!(!apply);
            }
            _ => panic!("Expected Commands::Init"),
        }
    }

    #[test]
    fn cli_subcommand_init_apply_parse() {
        let cli = Cli::try_parse_from(["nexql-mcp", "init", "claude-code", "--apply"]).unwrap();
        match cli.command {
            Some(Commands::Init { client, tui, apply }) => {
                assert_eq!(client.as_deref(), Some("claude-code"));
                assert!(!tui);
                assert!(apply);
            }
            _ => panic!("Expected Commands::Init"),
        }
    }

    /// `init <client> --apply` reuses the exact merge machinery `nexql-mcp
    /// setup`'s HTTP API uses (`setup::wire::preview_wire`/`apply_wire`) —
    /// this proves the in-process path writes the same `.mcp.json` shape the
    /// browser flow would have produced, non-interactively.
    #[test]
    fn init_client_apply_writes_merged_config_for_claude_code() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let workspace_dir = tempfile::tempdir().unwrap();

        let mut cfg = ConfigFile::default();
        cfg.upsert_profile(
            "local",
            nexql_conn::ProfileConfig {
                url: Some("postgres://dev@localhost:5432/appdb".into()),
                ..Default::default()
            },
        );
        cfg.save(&cfg_path).unwrap();

        let cli = Cli::try_parse_from([
            "nexql-mcp",
            "--config",
            cfg_path.to_str().unwrap(),
            "--workspace-root",
            workspace_dir.path().to_str().unwrap(),
            "init",
            "claude-code",
            "--apply",
        ])
        .unwrap();

        run_init_apply(&cli, "claude-code").expect("apply should succeed");

        let written = workspace_dir.path().join(".mcp.json");
        assert!(
            written.exists(),
            "expected {} to be written",
            written.display()
        );
        let content = std::fs::read_to_string(&written).unwrap();
        assert!(content.contains("nexql-mcp"), "{content}");
        assert!(content.contains("--profile"), "{content}");
        assert!(content.contains("local"), "{content}");
    }

    fn cli_with_config(cfg_path: &std::path::Path) -> Cli {
        Cli::try_parse_from([
            "nexql-mcp",
            "--config",
            cfg_path.to_str().unwrap(),
            "doctor",
        ])
        .unwrap()
    }

    /// Re-running `init <url>` for the same target must keep the profile's
    /// policy instead of replacing it with a bare URL.
    #[test]
    fn persist_profile_from_url_keeps_policy_for_same_target() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        std::fs::write(
            &cfg_path,
            r#"
[profiles.appuser_appdb]
url = "postgres://appuser@localhost:5432/appdb"
access_mode = "read"
pii_columns = ["public.users.email"]
deny_tables = ["auth.*"]
"#,
        )
        .unwrap();
        let cli = cli_with_config(&cfg_path);

        let name =
            persist_profile_from_url(&cli, "postgres://appuser@localhost/appdb?sslmode=require")
                .unwrap();
        assert_eq!(name, "appuser_appdb");
        let config = ConfigFile::load_path(&cfg_path).unwrap();
        let profile = &config.profiles["appuser_appdb"];
        assert_eq!(profile.pii_columns, vec!["public.users.email"]);
        assert_eq!(profile.deny_tables, vec!["auth.*"]);
        assert_eq!(profile.access_mode.as_deref(), Some("read"));
        assert!(profile.url.as_deref().unwrap().contains("sslmode=require"));
    }

    /// A different server that derives the same profile name gets a suffix
    /// rather than overwriting the existing profile.
    #[test]
    fn persist_profile_from_url_does_not_clobber_a_different_target() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        std::fs::write(
            &cfg_path,
            r#"
[profiles.appuser_appdb]
url = "postgres://appuser@prod.example.com:5432/appdb"
pii_columns = ["public.users.email"]
"#,
        )
        .unwrap();
        let cli = cli_with_config(&cfg_path);

        let name = persist_profile_from_url(&cli, "postgres://appuser@localhost/appdb").unwrap();
        assert_eq!(name, "appuser_appdb_2");
        let config = ConfigFile::load_path(&cfg_path).unwrap();
        let original = &config.profiles["appuser_appdb"];
        assert!(
            original
                .url
                .as_deref()
                .unwrap()
                .contains("prod.example.com")
        );
        assert_eq!(original.pii_columns, vec!["public.users.email"]);
    }

    /// An unreadable config must be reported, not silently replaced.
    #[test]
    fn persist_profile_from_url_refuses_to_overwrite_unreadable_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let broken = "[profiles.keep]\nurl = \"postgres://a@h/db\"\nthis is not toml = =\n";
        std::fs::write(&cfg_path, broken).unwrap();
        let cli = cli_with_config(&cfg_path);

        let err = persist_profile_from_url(&cli, "postgres://appuser@localhost/appdb").unwrap_err();
        assert!(err.contains("could not read"), "{err}");
        assert_eq!(std::fs::read_to_string(&cfg_path).unwrap(), broken);
    }

    /// The first URL saved into an empty config becomes `default_profile`, and
    /// the password is not left in the TOML (userinfo, encoded, or query).
    #[test]
    fn persist_profile_from_url_sets_default_and_omits_password() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let cli = cli_with_config(&cfg_path);
        let url = "postgres://appuser:p%40ss%23w%3Fd@localhost:5432/appdb";
        let name = persist_profile_from_url(&cli, url).unwrap();
        assert_eq!(name, "appuser_appdb");
        let config = ConfigFile::load_path(&cfg_path).unwrap();
        assert_eq!(config.default_profile.as_deref(), Some("appuser_appdb"));
        let written = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(!written.contains("p@ss#w?d"), "{written}");
        assert!(!written.contains("p%40ss"), "{written}");

        let query = "postgres://appuser@localhost:5432/appdb?password=queried-secret";
        let err = persist_profile_from_url(&cli, query).unwrap_err();
        assert!(
            err.contains("password") || err.contains("could not be moved"),
            "{err}"
        );
        let written = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(!written.contains("queried-secret"), "{written}");
    }

    /// A password that also appears in the user, database, or URL scheme
    /// (`postgres:postgres`) must not be mistaken for a leaked password.
    #[test]
    fn persist_profile_from_url_accepts_password_equal_to_user() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let cli = cli_with_config(&cfg_path);
        for url in [
            "postgres://postgres:postgres@localhost:5432/appdb",
            "postgres://admin:appdb@localhost:5432/appdb",
            "postgres://u:5432@localhost:5432/db",
        ] {
            persist_profile_from_url(&cli, url).unwrap_or_else(|e| panic!("{url}: {e}"));
        }
        let config = ConfigFile::load_path(&cfg_path).unwrap();
        for profile in config.profiles.values() {
            assert!(profile.password.is_none());
            assert!(
                !init_clients::url_contains_password(profile.url.as_deref().unwrap()),
                "{profile:?}"
            );
        }
    }

    /// `init <client> --apply <url>` creates the keyring profile it then wires,
    /// and the password never lands in the client JSON or config.toml.
    #[test]
    fn init_client_apply_url_creates_profile_without_password() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let workspace_dir = tempfile::tempdir().unwrap();
        let url = "postgres://appuser:s3cret-batch1@localhost:5432/appdb";
        let cli = Cli::try_parse_from([
            "nexql-mcp",
            "--config",
            cfg_path.to_str().unwrap(),
            "--workspace-root",
            workspace_dir.path().to_str().unwrap(),
            url,
            "init",
            "claude-code",
            "--apply",
        ])
        .unwrap();

        run_init_apply(&cli, "claude-code").expect("apply should create a profile and wire it");

        let written = std::fs::read_to_string(workspace_dir.path().join(".mcp.json")).unwrap();
        let config = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(written.contains("--profile"), "{written}");
        assert!(!written.contains("s3cret-batch1"), "{written}");
        assert!(!config.contains("s3cret-batch1"), "{config}");
        assert!(
            config.contains("appuser_appdb") || config.contains("appuser"),
            "{config}"
        );
    }

    /// A copy-only client (no real mergeable config file) must refuse
    /// `--apply` with an actionable error, not silently do nothing or panic.
    #[test]
    fn init_client_apply_rejects_copy_only_client() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let cli = Cli::try_parse_from([
            "nexql-mcp",
            "--config",
            cfg_path.to_str().unwrap(),
            "init",
            "jetbrains",
            "--apply",
        ])
        .unwrap();

        let err = run_init_apply(&cli, "jetbrains").expect_err("jetbrains has no mergeable file");
        assert!(err.contains("copy/paste-only"), "{err}");
    }

    #[test]
    fn cli_subcommand_onboarding_parse() {
        let cli = Cli::try_parse_from(["nexql-mcp", "onboarding"]).unwrap();
        match cli.command {
            Some(Commands::Onboarding) => {}
            _ => panic!("Expected Commands::Onboarding"),
        }
    }

    #[test]
    fn cli_subcommand_setup_parse() {
        let cli = Cli::try_parse_from([
            "nexql-mcp",
            "setup",
            "--workspace-root",
            "/tmp/proj",
            "--no-open",
            "--name",
            "dev",
            "--dsn",
            "postgres://u@h/db",
            "--client",
            "cursor",
            "--apply",
            "--exit",
            "--skip-test",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Setup {
                workspace_root,
                open,
                name,
                dsn,
                client,
                apply,
                exit,
                skip_test,
                ..
            }) => {
                assert_eq!(
                    workspace_root.as_deref(),
                    Some(PathBuf::from("/tmp/proj").as_path())
                );
                assert!(!open);
                assert_eq!(name.as_deref(), Some("dev"));
                assert_eq!(dsn.as_deref(), Some("postgres://u@h/db"));
                assert_eq!(client, vec!["cursor".to_string()]);
                assert!(apply);
                assert!(exit);
                assert!(skip_test);
            }
            _ => panic!("Expected Commands::Setup"),
        }
    }

    #[test]
    fn setup_headless_missing_dsn_errors() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let err = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(setup::headless::run(setup::headless::HeadlessOptions {
                name: "x".into(),
                dsn: "".into(),
                clients: vec![],
                apply: false,
                skip_test: true,
                config_path: cfg_path,
                workspace_root: None,
            }))
            .expect_err("empty dsn should fail");
        assert!(err.contains("NEXQL_MCP_DSN"), "{err}");
    }

    #[test]
    fn setup_headless_creates_profile_and_applies_cursor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let workspace_dir = tempfile::tempdir().unwrap();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(setup::headless::run(setup::headless::HeadlessOptions {
                name: "local".into(),
                dsn: "postgres://dev@localhost:5432/appdb".into(),
                clients: vec!["cursor".to_string()],
                apply: true,
                skip_test: true,
                config_path: cfg_path.clone(),
                workspace_root: Some(workspace_dir.path().to_path_buf()),
            }))
            .expect("headless setup should succeed");

        let loaded = nexql_conn::ConfigFile::load_path_migrated(&cfg_path)
            .expect("config should load")
            .0;
        assert!(loaded.profiles.contains_key("local"));
        assert_eq!(loaded.default_profile.as_deref(), Some("local"));

        let written = workspace_dir.path().join(".cursor/mcp.json");
        assert!(written.exists(), "expected {} to exist", written.display());
        let content = std::fs::read_to_string(&written).unwrap();
        assert!(content.contains("nexql-mcp"), "{content}");
        assert!(content.contains("--profile"), "{content}");
        assert!(content.contains("local"), "{content}");
    }
}
