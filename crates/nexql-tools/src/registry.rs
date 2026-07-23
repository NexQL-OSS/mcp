//! Tool name catalog for the MCP surface.

/// Read-only tool surface (catalog + index + Phase 4 monitoring/DDL + Phase 4b breadth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolName {
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
    ];

    /// Index-backed tools (require `nexql-mcp index build`).
    pub const PHASE3: &'static [ToolName] = &[
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
    ];

    pub const READ_ONLY: &'static [ToolName] = Self::ACTIVE;

    pub fn as_str(self) -> &'static str {
        match self {
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
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::READ_ONLY.iter().copied().find(|t| t.as_str() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::ToolName;

    #[test]
    fn read_only_matches_active() {
        assert_eq!(ToolName::READ_ONLY.len(), ToolName::ACTIVE.len());
        assert_eq!(ToolName::READ_ONLY, ToolName::ACTIVE);
    }

    #[test]
    fn phase2_has_eight_tools() {
        assert_eq!(ToolName::PHASE2.len(), 8);
    }

    #[test]
    fn phase3_has_four_tools() {
        assert_eq!(ToolName::PHASE3.len(), 4);
    }

    #[test]
    fn phase4_has_sixteen_tools() {
        assert_eq!(ToolName::PHASE4.len(), 16);
    }

    #[test]
    fn phase4b_has_four_tools() {
        assert_eq!(ToolName::PHASE4B.len(), 4);
    }

    #[test]
    fn active_surface_is_thirty_two_tools() {
        assert_eq!(ToolName::ACTIVE.len(), 32);
        assert_eq!(
            ToolName::ACTIVE.len(),
            ToolName::PHASE2.len()
                + ToolName::PHASE3.len()
                + ToolName::PHASE4.len()
                + ToolName::PHASE4B.len()
        );
    }
}
