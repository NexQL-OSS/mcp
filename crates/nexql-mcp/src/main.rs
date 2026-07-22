//! CLI entrypoint for the standalone NexQL Postgres MCP server.
//!
//! Subcommands and flags are defined here; implementation lands in phase 1+.

use clap::{Parser, Subcommand};

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
    /// Named profile from config (repeatable)
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
}

#[derive(clap::Args, Default)]
struct TransportArgs {
    /// stdio transport (default)
    #[arg(long)]
    stdio: bool,

    /// Streamable HTTP transport
    #[arg(long)]
    http: bool,

    /// HTTP listen port (only with --http)
    #[arg(long = "http-port", default_value_t = 8899)]
    http_port: u16,

    /// HTTP bind address (only with --http)
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
}

#[derive(clap::Args, Default)]
struct AccessArgs {
    /// read | write | admin
    #[arg(long, default_value = "read")]
    access_mode: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Build or manage the on-disk schema index
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
    /// Manage connection profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Connectivity, grants, and index freshness checks
    Doctor,
    /// Emit paste-ready MCP client config
    Init {
        /// claude | cursor | vscode | zed | windsurf | continue
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Doctor => tracing::info!("doctor: not yet implemented"),
            Commands::Init { client } => {
                tracing::info!(client, "init: not yet implemented");
            }
            Commands::Index { action } => match action {
                IndexAction::Build => tracing::info!("index build: not yet implemented"),
                IndexAction::Status => tracing::info!("index status: not yet implemented"),
                IndexAction::Refresh => tracing::info!("index refresh: not yet implemented"),
                IndexAction::Clear => tracing::info!("index clear: not yet implemented"),
            },
            Commands::Profile { action } => match action {
                ProfileAction::List => tracing::info!("profile list: not yet implemented"),
                ProfileAction::Add => tracing::info!("profile add: not yet implemented"),
                ProfileAction::SetPassword { name } => {
                    tracing::info!(%name, "profile set-password: not yet implemented");
                }
                ProfileAction::Test { name } => {
                    tracing::info!(?name, "profile test: not yet implemented");
                }
            },
        }
        return;
    }

    // Default: start MCP server (stdio unless --http)
    let transport = if cli.transport.http { "http" } else { "stdio" };
    tracing::info!(
        transport,
        http_port = cli.transport.http_port,
        access_mode = %cli.access.access_mode,
        "nexql-mcp server scaffold — implementation starts in phase 1"
    );
    let _ = cli.connection_string;
}
