/// Where a Postgres connection string was resolved from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    CliArg,
    Profile,
    Flags,
    DatabaseUrl,
    PgEnv,
    DefaultProfile,
    PgPass,
    EnvFile,
}
