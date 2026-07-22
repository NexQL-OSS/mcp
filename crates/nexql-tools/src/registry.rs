/// Initial read-only tool surface (22 from ToolSpec.ts, minus select_connection_context).
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
}

impl ToolName {
    pub const READ_ONLY: &'static [ToolName] = &[
        Self::SearchSchema,
        Self::DescribeObject,
        Self::GetJoinPath,
        Self::SampleValues,
        Self::RunSelect,
        Self::ExplainQuery,
        Self::ListConnections,
        Self::ListDatabases,
        Self::ListSchemas,
        Self::ListObjects,
        Self::GetCurrentContext,
        Self::SwitchConnection,
        Self::GetDdl,
        Self::TableStats,
        Self::IndexUsage,
        Self::ListRunningQueries,
        Self::FindBlockingLocks,
        Self::SlowQueries,
        Self::DbHealthCheck,
        Self::ExplainAnalyze,
        Self::AnalyzeQueryPlan,
    ];

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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ToolName;

    #[test]
    fn read_only_catalog_has_twenty_one_tools() {
        assert_eq!(ToolName::READ_ONLY.len(), 21);
    }
}
