//! Tool name catalog for the MCP surface.

/// Tool surface preset profiles to control context window overhead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolProfile {
    /// Core schema search, object inspection, join path, query execution, and export tools.
    Query,
    /// DBA health checks, index suggestions, table stats, locks, slow queries, and maintenance.
    Dba,
    /// Minimal initial tool surface (5 core tools) + discover_tools meta-tool for lazy tool activation.
    Meta,
    /// All active MCP surface tools.
    #[default]
    Full,
}

impl ToolProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Dba => "dba",
            Self::Meta => "meta",
            Self::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "query" => Some(Self::Query),
            "dba" => Some(Self::Dba),
            "meta" => Some(Self::Meta),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// Read-only tool surface (catalog + index + Phase 4 monitoring/DDL + Phase 4b breadth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolName {
    ResolveTarget,
    SearchSchema,
    DescribeObject,
    GetJoinPath,
    SampleValues,
    RunSelect,
    ExplainQuery,
    ListConnections,
    ListDatabases,
    ListSchemas,
    ListObjects,
    GetCurrentContext,
    SwitchConnection,
    GetDdl,
    TableStats,
    IndexUsage,
    ListRunningQueries,
    FindBlockingLocks,
    SlowQueries,
    DbHealthCheck,
    ExplainAnalyze,
    AnalyzeQueryPlan,
    GetIndexStatus,
    ListExtensions,
    ServerSettings,
    SuggestIndexes,
    FindUnusedIndexes,
    BloatReport,
    FindMissingFks,
    ExportQuery,
    ListRoles,
    DbDashboard,
    DeepPlanAnalysis,
    SchemaDiff,
    GenerateMigration,
    ExecuteSql,
    EditRow,
    ImportData,
    ApplyDdl,
    CreateIndexConcurrently,
    RunMaintenance,
    TerminateQuery,
    DiscoverTools,
    AutoTuneQuery,
    CheckDdlSafety,
}

impl ToolName {
    pub const PHASE2: &'static [ToolName] = &[
        Self::ListConnections,
        Self::ListDatabases,
        Self::ListSchemas,
        Self::ListObjects,
        Self::GetCurrentContext,
        Self::SwitchConnection,
        Self::RunSelect,
        Self::ExplainQuery,
        Self::DiscoverTools,
    ];

    /// Index-backed tools (require `nexql-mcp index build`).
    pub const PHASE3: &'static [ToolName] = &[
        Self::ResolveTarget,
        Self::SearchSchema,
        Self::DescribeObject,
        Self::GetJoinPath,
        Self::SampleValues,
    ];

    /// Phase 4 monitoring / DDL / index-status (+ free advisory tools).
    pub const PHASE4: &'static [ToolName] = &[
        Self::GetDdl,
        Self::TableStats,
        Self::IndexUsage,
        Self::ListRunningQueries,
        Self::FindBlockingLocks,
        Self::SlowQueries,
        Self::DbHealthCheck,
        Self::ExplainAnalyze,
        Self::AnalyzeQueryPlan,
        Self::GetIndexStatus,
        Self::ListExtensions,
        Self::ServerSettings,
        Self::SuggestIndexes,
        Self::FindUnusedIndexes,
        Self::BloatReport,
        Self::FindMissingFks,
    ];

    /// Phase 4b read-only breadth (export / roles / dashboard; more tools land here).
    pub const PHASE4B: &'static [ToolName] = &[
        Self::ExportQuery,
        Self::ListRoles,
        Self::DbDashboard,
        Self::DeepPlanAnalysis,
        Self::SchemaDiff,
        Self::GenerateMigration,
        Self::AutoTuneQuery,
        Self::CheckDdlSafety,
    ];

    /// Phase 9 write/admin tools (listed always; gated at call time by access mode).
    pub const PHASE9: &'static [ToolName] = &[
        Self::ExecuteSql,
        Self::EditRow,
        Self::ImportData,
        Self::ApplyDdl,
        Self::CreateIndexConcurrently,
        Self::RunMaintenance,
        Self::TerminateQuery,
    ];

    /// Full tools/list surface for the current phase.
    pub const ACTIVE: &'static [ToolName] = &[
        Self::ListConnections,
        Self::ListDatabases,
        Self::ListSchemas,
        Self::ListObjects,
        Self::GetCurrentContext,
        Self::SwitchConnection,
        Self::RunSelect,
        Self::ExplainQuery,
        Self::DiscoverTools,
        Self::ResolveTarget,
        Self::SearchSchema,
        Self::DescribeObject,
        Self::GetJoinPath,
        Self::SampleValues,
        Self::GetDdl,
        Self::TableStats,
        Self::IndexUsage,
        Self::ListRunningQueries,
        Self::FindBlockingLocks,
        Self::SlowQueries,
        Self::DbHealthCheck,
        Self::ExplainAnalyze,
        Self::AnalyzeQueryPlan,
        Self::GetIndexStatus,
        Self::ListExtensions,
        Self::ServerSettings,
        Self::SuggestIndexes,
        Self::FindUnusedIndexes,
        Self::BloatReport,
        Self::FindMissingFks,
        Self::ExportQuery,
        Self::ListRoles,
        Self::DbDashboard,
        Self::DeepPlanAnalysis,
        Self::SchemaDiff,
        Self::GenerateMigration,
        Self::AutoTuneQuery,
        Self::CheckDdlSafety,
        Self::ExecuteSql,
        Self::EditRow,
        Self::ImportData,
        Self::ApplyDdl,
        Self::CreateIndexConcurrently,
        Self::RunMaintenance,
        Self::TerminateQuery,
    ];

    /// Read-only subset (Phase 2–4b). Write/admin tools are in ACTIVE but gated at dispatch.
    pub const READ_ONLY: &'static [ToolName] = &[
        Self::ListConnections,
        Self::ListDatabases,
        Self::ListSchemas,
        Self::ListObjects,
        Self::GetCurrentContext,
        Self::SwitchConnection,
        Self::RunSelect,
        Self::ExplainQuery,
        Self::ResolveTarget,
        Self::SearchSchema,
        Self::DescribeObject,
        Self::GetJoinPath,
        Self::SampleValues,
        Self::GetDdl,
        Self::TableStats,
        Self::IndexUsage,
        Self::ListRunningQueries,
        Self::FindBlockingLocks,
        Self::SlowQueries,
        Self::DbHealthCheck,
        Self::ExplainAnalyze,
        Self::AnalyzeQueryPlan,
        Self::GetIndexStatus,
        Self::ListExtensions,
        Self::ServerSettings,
        Self::SuggestIndexes,
        Self::FindUnusedIndexes,
        Self::BloatReport,
        Self::FindMissingFks,
        Self::ExportQuery,
        Self::ListRoles,
        Self::DbDashboard,
        Self::DeepPlanAnalysis,
        Self::SchemaDiff,
        Self::GenerateMigration,
    ];

    /// Subset of tools optimized for context-constrained query & schema exploration tasks.
    pub const QUERY_PROFILE: &'static [ToolName] = &[
        Self::ListConnections,
        Self::ListDatabases,
        Self::ListSchemas,
        Self::ListObjects,
        Self::GetCurrentContext,
        Self::SwitchConnection,
        Self::RunSelect,
        Self::ExplainQuery,
        Self::ResolveTarget,
        Self::SearchSchema,
        Self::DescribeObject,
        Self::GetJoinPath,
        Self::SampleValues,
        Self::GetDdl,
        Self::ExportQuery,
    ];

    /// Subset of tools optimized for database administration, performance tuning, and health checks.
    pub const DBA_PROFILE: &'static [ToolName] = &[
        Self::ListConnections,
        Self::GetCurrentContext,
        Self::TableStats,
        Self::IndexUsage,
        Self::ListRunningQueries,
        Self::FindBlockingLocks,
        Self::SlowQueries,
        Self::DbHealthCheck,
        Self::ExplainAnalyze,
        Self::AnalyzeQueryPlan,
        Self::GetIndexStatus,
        Self::ListExtensions,
        Self::ServerSettings,
        Self::SuggestIndexes,
        Self::FindUnusedIndexes,
        Self::BloatReport,
        Self::FindMissingFks,
        Self::DbDashboard,
        Self::DeepPlanAnalysis,
        Self::SchemaDiff,
        Self::GenerateMigration,
        Self::AutoTuneQuery,
        Self::CheckDdlSafety,
        Self::RunMaintenance,
        Self::TerminateQuery,
    ];

    /// Minimal initial tool surface with discover_tools for lazy tool activation.
    pub const META_PROFILE: &'static [ToolName] = &[
        Self::ListConnections,
        Self::GetCurrentContext,
        Self::SearchSchema,
        Self::DescribeObject,
        Self::RunSelect,
        Self::DiscoverTools,
    ];

    pub fn for_profile(profile: ToolProfile) -> &'static [ToolName] {
        match profile {
            ToolProfile::Query => Self::QUERY_PROFILE,
            ToolProfile::Dba => Self::DBA_PROFILE,
            ToolProfile::Meta => Self::META_PROFILE,
            ToolProfile::Full => Self::ACTIVE,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResolveTarget => "resolve_target",
            Self::SearchSchema => "search_schema",
            Self::DescribeObject => "describe_object",
            Self::GetJoinPath => "get_join_path",
            Self::SampleValues => "sample_values",
            Self::RunSelect => "run_select",
            Self::ExplainQuery => "explain_query",
            Self::ListConnections => "list_connections",
            Self::ListDatabases => "list_databases",
            Self::ListSchemas => "list_schemas",
            Self::ListObjects => "list_objects",
            Self::GetCurrentContext => "get_current_context",
            Self::SwitchConnection => "switch_connection",
            Self::GetDdl => "get_ddl",
            Self::TableStats => "table_stats",
            Self::IndexUsage => "index_usage",
            Self::ListRunningQueries => "list_running_queries",
            Self::FindBlockingLocks => "find_blocking_locks",
            Self::SlowQueries => "slow_queries",
            Self::DbHealthCheck => "db_health_check",
            Self::ExplainAnalyze => "explain_analyze",
            Self::AnalyzeQueryPlan => "analyze_query_plan",
            Self::GetIndexStatus => "get_index_status",
            Self::ListExtensions => "list_extensions",
            Self::ServerSettings => "server_settings",
            Self::SuggestIndexes => "suggest_indexes",
            Self::FindUnusedIndexes => "find_unused_indexes",
            Self::BloatReport => "bloat_report",
            Self::FindMissingFks => "find_missing_fks",
            Self::ExportQuery => "export_query",
            Self::ListRoles => "list_roles",
            Self::DbDashboard => "db_dashboard",
            Self::DeepPlanAnalysis => "deep_plan_analysis",
            Self::SchemaDiff => "schema_diff",
            Self::GenerateMigration => "generate_migration",
            Self::AutoTuneQuery => "auto_tune_query",
            Self::CheckDdlSafety => "check_ddl_safety",
            Self::ExecuteSql => "execute_sql",
            Self::EditRow => "edit_row",
            Self::ImportData => "import_data",
            Self::ApplyDdl => "apply_ddl",
            Self::CreateIndexConcurrently => "create_index_concurrently",
            Self::RunMaintenance => "run_maintenance",
            Self::TerminateQuery => "terminate_query",
            Self::DiscoverTools => "discover_tools",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ACTIVE.iter().copied().find(|t| t.as_str() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::ToolName;

    #[test]
    fn read_only_is_thirty_five_tools() {
        assert_eq!(ToolName::READ_ONLY.len(), 35);
    }

    #[test]
    fn phase9_has_seven_tools() {
        assert_eq!(ToolName::PHASE9.len(), 7);
    }

    #[test]
    fn active_surface_is_forty_five_tools() {
        assert_eq!(ToolName::ACTIVE.len(), 45);
    }

    #[test]
    fn read_only_subset_of_active() {
        for tool in ToolName::READ_ONLY {
            assert!(ToolName::ACTIVE.contains(tool));
        }
        assert_ne!(ToolName::READ_ONLY.len(), ToolName::ACTIVE.len());
    }
}
