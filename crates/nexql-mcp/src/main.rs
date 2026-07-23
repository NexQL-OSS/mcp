//! CLI entrypoint for the standalone NexQL Postgres MCP server.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use async_trait::async_trait;
use clap::{Parser, Subcommand};
use nexql_conn::{
    ConnectionParams, PoolOptions, ResolveInputs, apply_session_guards, connect_once, resolve,
};
use nexql_index::{
    BuildDepth, BuildMode, BuildRequest, IndexScope, IndexStore, PgCatalogDb, build_index,
};
use nexql_policy::{AccessMode, PolicyCaps, check_superuser_guard};
use nexql_proto::{StdioServer, ToolBackend, ToolCallResult, ToolDescriptor};
use nexql_tools::{ToolRouter, ToolSession};
use serde_json::Value;
use tracing_subscriber::EnvFilter;

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
}

#[derive(clap::Args, Default)]
struct AccessArgs {
    #[arg(long, default_value = "read")]
    access_mode: String,

    #[arg(long = "i-know-what-im-doing")]
    i_know_what_im_doing: bool,

    #[arg(long = "max-rows")]
    max_rows: Option<u32>,
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
    Init {
        client: String,
    },
}

#[derive(Subcommand)]
enum IndexAction {
    Build,
    Status,
    Refresh,
    Clear,
}

#[derive(Subcommand)]
enum ProfileAction {
    List,
    Add,
    SetPassword { name: String },
    Test { name: Option<String> },
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
            .map(|s| ToolDescriptor {
                name: s.name.as_str().to_string(),
                description: s.description.to_string(),
                input_schema: s.input_schema.clone(),
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    // MCP stdio: never log to stdout.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

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
            Commands::Init { client } => {
                println!("{}", init_snippet(client, cli.connection_string.as_deref()));
            }
            Commands::Index { action } => {
                return match run_index_action(&cli, &action).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("{e}");
                        ExitCode::FAILURE
                    }
                };
            }
            Commands::Profile { action } => match action {
                ProfileAction::List => eprintln!("profile list: not yet implemented"),
                ProfileAction::Add => eprintln!("profile add: not yet implemented"),
                ProfileAction::SetPassword { name } => {
                    eprintln!("profile set-password {name}: not yet implemented");
                }
                ProfileAction::Test { name } => {
                    eprintln!("profile test {name:?}: not yet implemented");
                }
            },
        }
        return ExitCode::SUCCESS;
    }

    if cli.transport.http {
        eprintln!("HTTP transport lands in phase 8 — use default stdio for now");
        return ExitCode::FAILURE;
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
        env: None,
        ..Default::default()
    }
}

async fn run_stdio_server(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mode: AccessMode = cli.access.access_mode.parse()?;
    let resolved = resolve(&resolve_inputs(cli))?;
    let mut caps = PolicyCaps::default();
    if let Some(n) = cli.access.max_rows {
        caps = caps.with_max_rows(n);
    }
    let session = ToolSession::from_resolved(resolved, mode, caps).await?;
    let backend = Arc::new(RouterBackend {
        router: ToolRouter::new(session),
    });
    let server = StdioServer::new(backend);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    server.serve(stdin, stdout).await?;
    Ok(())
}

async fn run_index_action(
    cli: &Cli,
    action: &IndexAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        IndexAction::Build => run_index_build(cli).await,
        IndexAction::Status => {
            eprintln!("index status: not yet implemented");
            Ok(())
        }
        IndexAction::Refresh => {
            eprintln!("index refresh: not yet implemented");
            Ok(())
        }
        IndexAction::Clear => {
            eprintln!("index clear: not yet implemented");
            Ok(())
        }
    }
}

async fn run_index_build(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve(&resolve_inputs(cli))?;
    let database = resolved
        .params
        .dbname
        .clone()
        .unwrap_or_else(|| "postgres".into());
    let connection_id = resolved
        .profile_name
        .clone()
        .unwrap_or_else(|| {
            let host = resolved.params.host.as_deref().unwrap_or("localhost");
            format!("{host}/{database}")
        });

    let client = connect_once(&resolved.params).await?;
    let store = IndexStore::new(default_index_root());
    let db = PgCatalogDb::new(&client);
    let req = BuildRequest {
        connection_id: connection_id.clone(),
        database: database.clone(),
        scope: IndexScope {
            included_schemas: vec!["public".into()],
            excluded_objects: vec![],
            pii_excluded_columns: vec![],
        },
        depth: BuildDepth::Structure,
        build_mode: BuildMode::Guided,
        environment: "development".into(),
    };

    let mut on_progress = |ev: nexql_index::BuildProgress| {
        eprintln!("index build: {ev:?}");
    };

    let manifest = build_index(&store, &db, &req, Some(&mut on_progress), None).await?;
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

fn default_index_root() -> PathBuf {
    if let Ok(p) = std::env::var("NEXQL_MCP_INDEX_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/nexql-mcp")
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
    println!("doctor: ok");
    Ok(())
}

fn init_snippet(client: &str, url: Option<&str>) -> String {
    let cmd = match url {
        Some(u) => format!("nexql-mcp {u}"),
        None => "nexql-mcp".into(),
    };
    match client {
        "claude" | "claude-desktop" => format!(
            r#"{{
  "mcpServers": {{
    "nexql": {{
      "command": "nexql-mcp",
      "args": [{args}]
    }}
  }}
}}"#,
            args = url.map(|u| format!("\"{u}\"")).unwrap_or_default()
        ),
        "cursor" => format!(
            r#"{{
  "mcpServers": {{
    "nexql": {{
      "command": "nexql-mcp",
      "args": [{args}]
    }}
  }}
}}"#,
            args = url.map(|u| format!("\"{u}\"")).unwrap_or_default()
        ),
        other => format!("# paste-ready config for '{other}' — use command: {cmd}"),
    }
}
