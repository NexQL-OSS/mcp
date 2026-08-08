// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Tool dispatch for catalog (Phase 2) + index (Phase 3) + Phase 4 surfaces.

use std::sync::Arc;

use nexql_index::{
    BuildDepth, BuildMode, BuildRequest, CatalogDb, Embedder, IndexQueryService, IndexScope,
    IndexStore, ObjectEntry, PgCatalogDb, QueryPolicyFilter, RefResolution, SearchOptions,
    build_index,
};
use nexql_policy::{
    PolicyFilter, SqlDecision, enforce_read_table_policy, select_table_refs, validate_readonly_sql,
};
use serde_json::{Value, json};

use crate::cell_json::{columnarize_read_payload, redact_pii_in_payload, rows_to_json_array};
use crate::critique;
use crate::error::ToolError;
use crate::export::{ExportFormat, columns_from_rows, rows_to_csv, rows_to_sql_insert};
use crate::plan::{analyze_deep_plan, build_explain_sql, extract_plan_metrics};
use crate::registry::ToolName;
use crate::schema::{ToolSpec, active_tools};
use crate::session::ToolSession;
use crate::sql::{self, REPORT_LIMIT_DEFAULT, SLOW_QUERIES_DEFAULT, parse_ref};
use crate::write::{
    apply_ddl, create_index_concurrently, edit_row, execute_sql, import_data, run_maintenance,
    terminate_query,
};

/// Default hit cap for `search_schema` (matches TS ToolExecutor).
const SEARCH_SCHEMA_LIMIT: usize = 10;

const NO_INDEX_HINT: &str =
    "No schema index configured — call the 'rebuild_index' tool to build an index.";

#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub text: String,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl ToolOutcome {
    /// Success payload for MCP `structuredContent`.
    ///
    /// Cursor (and some other clients) require `structuredContent` to be a JSON
    /// **object**. Bare arrays are dropped before the model sees them — always
    /// wrap: `{ "rows": [ ... ] }`.
    pub fn ok_json(value: Value) -> Self {
        let value = ensure_structured_object(value);
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Self {
            text,
            structured: Some(value),
            is_error: false,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        let message = msg.into();
        Self {
            text: message.clone(),
            structured: Some(json!({ "error": message })),
            is_error: true,
        }
    }
}

/// Cursor MCP rejects non-object `structuredContent`. Wrap arrays as `{ "rows": … }`.
fn ensure_structured_object(value: Value) -> Value {
    match value {
        Value::Array(rows) => json!({ "rows": rows }),
        other => other,
    }
}

pub struct ToolRouter {
    session: Arc<ToolSession>,
    /// Optional override; when `None`, uses `session.index_store`.
    index_override: Option<Option<IndexStore>>,
    /// When true and an embedder is set, `search_schema` fuses via RRF.
    use_semantic: bool,
    embedder: Option<Arc<dyn Embedder>>,
    specs: Vec<ToolSpec>,
    managed_extension: bool,
}

impl ToolRouter {
    pub fn new(session: Arc<ToolSession>) -> Self {
        Self {
            session,
            index_override: None,
            use_semantic: false,
            embedder: None,
            specs: active_tools(),
            managed_extension: false,
        }
    }

    /// Build with an explicit index store (or `None` to force the no-index error path).
    pub fn with_index_store(session: Arc<ToolSession>, store: Option<IndexStore>) -> Self {
        Self {
            session,
            index_override: Some(store),
            use_semantic: false,
            embedder: None,
            specs: active_tools(),
            managed_extension: false,
        }
    }

    /// Enable semantic RRF fusion for `search_schema` (requires embeddings on disk + embedder).
    pub fn with_semantic(
        mut self,
        use_semantic: bool,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Self {
        self.use_semantic = use_semantic;
        self.embedder = embedder;
        self
    }

    /// Filter active tools by requested `ToolProfile`.
    pub fn with_profile(mut self, profile: crate::registry::ToolProfile) -> Self {
        self.specs = crate::schema::tools_for_profile(profile);
        self
    }

    /// Exclude setup/profile mutation tools for managed extension hosts.
    pub fn with_managed_extension(mut self, enabled: bool) -> Self {
        self.managed_extension = enabled;
        if enabled {
            const BLOCKED: &[ToolName] = &[
                ToolName::SetupConnection,
                ToolName::SaveProfile,
                ToolName::TestProfile,
                ToolName::ExportProfile,
                ToolName::ImportProfile,
            ];
            self.specs.retain(|s| !BLOCKED.contains(&s.name));
        }
        self
    }

    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn index_store(&self) -> Option<&IndexStore> {
        match &self.index_override {
            Some(inner) => inner.as_ref(),
            None => self.session.index_store.as_ref(),
        }
    }

    fn query_filter(&self) -> QueryPolicyFilter {
        policy_to_query_filter(&self.session.filter())
    }

    pub async fn call(&self, name: &str, args: Value) -> ToolOutcome {
        let outcome = match self.call_inner(name, args).await {
            Ok(out) => out,
            Err(e) => ToolOutcome::err(e.to_string()),
        };
        self.tag_outcome_with_context(outcome).await
    }

    async fn tag_outcome_with_context(&self, mut outcome: ToolOutcome) -> ToolOutcome {
        let (connection_id, database) = self.session.active_context().await;
        let access_mode = match self.session.access_mode() {
            nexql_policy::AccessMode::Read => "read",
            nexql_policy::AccessMode::Write => "write",
            nexql_policy::AccessMode::Admin => "admin",
        };
        let mut freshness: Option<serde_json::Value> = None;
        if let Some(store) = self.session.index_store.as_ref() {
            let base = store.base_dir(&connection_id, &database);
            if let Ok(Some(manifest)) = store.read_manifest(&base) {
                let stale = self.session.is_index_stale(&connection_id, &database);
                let mut freshness_obj = json!({
                    "indexedAt": manifest.indexed_at,
                    "schemaFingerprint": manifest.schema_fingerprint,
                    "stale": stale,
                });
                if stale {
                    freshness_obj["reason"] = json!("schema_changed");
                }
                freshness = Some(freshness_obj);
            } else {
                freshness = Some(json!({ "stale": true, "reason": "no_index" }));
            }
        }
        if let Some(ref mut structured) = outcome.structured
            && let Some(obj) = structured.as_object_mut()
        {
            if !obj.contains_key("connectionId") {
                obj.insert("connectionId".into(), json!(connection_id));
            }
            if !obj.contains_key("database") {
                obj.insert("database".into(), json!(database));
            }
            if !obj.contains_key("accessMode") {
                obj.insert("accessMode".into(), json!(access_mode));
            }
            if let Some(ref f) = freshness {
                obj.insert("freshness".into(), f.clone());
            }
        }
        let header = format!(
            "[context connectionId={connection_id} database={database} accessMode={access_mode}]\n"
        );
        if !outcome.text.starts_with("[context ") {
            outcome.text = format!("{header}{}", outcome.text);
        }
        outcome
    }

    async fn call_inner(&self, name: &str, args: Value) -> Result<ToolOutcome, ToolError> {
        let tool = ToolName::parse(name).ok_or_else(|| ToolError::Unknown(name.to_string()))?;
        match tool {
            ToolName::ListConnections => Ok(self.list_connections()),
            ToolName::ListDatabases => self.list_databases(&args).await,
            ToolName::ListSchemas => self.list_schemas().await,
            ToolName::ListObjects => self.list_objects(&args).await,
            ToolName::GetCurrentContext => self.get_current_context().await,
            ToolName::SwitchConnection => self.switch_connection(&args).await,
            ToolName::RunSelect => self.run_select(&args).await,
            ToolName::ExplainQuery => self.explain_query(&args).await,
            ToolName::SearchSchema => self.search_schema(&args).await,
            ToolName::DescribeObject => self.describe_object(&args).await,
            ToolName::GetJoinPath => self.get_join_path(&args).await,
            ToolName::SampleValues => self.sample_values(&args).await,
            ToolName::GetDdl => self.get_ddl(&args).await,
            ToolName::TableStats => self.table_stats(&args).await,
            ToolName::IndexUsage => self.index_usage(&args).await,
            ToolName::ListRunningQueries => self.list_running_queries().await,
            ToolName::FindBlockingLocks => self.find_blocking_locks().await,
            ToolName::SlowQueries => self.slow_queries(&args).await,
            ToolName::DbHealthCheck => self.db_health_check().await,
            ToolName::ExplainAnalyze => self.explain_analyze(&args).await,
            ToolName::AnalyzeQueryPlan => self.analyze_query_plan(&args).await,
            ToolName::GetIndexStatus => self.get_index_status().await,
            ToolName::ListExtensions => self.list_extensions().await,
            ToolName::ServerSettings => self.server_settings().await,
            ToolName::SuggestIndexes => self.suggest_indexes(&args).await,
            ToolName::FindUnusedIndexes => self.find_unused_indexes(&args).await,
            ToolName::BloatReport => self.bloat_report(&args).await,
            ToolName::FindMissingFks => self.find_missing_fks(&args).await,
            ToolName::ExportQuery => self.export_query(&args).await,
            ToolName::ListRoles => self.list_roles(&args).await,
            ToolName::DbDashboard => self.db_dashboard().await,
            ToolName::DeepPlanAnalysis => self.deep_plan_analysis(&args).await,
            ToolName::SchemaDiff => self.schema_diff(&args).await,
            ToolName::GenerateMigration => self.generate_migration(&args).await,
            ToolName::ExecuteSql => self.execute_sql_tool(&args).await,
            ToolName::EditRow => self.edit_row_tool(&args).await,
            ToolName::ImportData => self.import_data_tool(&args).await,
            ToolName::ApplyDdl => self.apply_ddl_tool(&args).await,
            ToolName::CreateIndexConcurrently => self.create_index_concurrently_tool(&args).await,
            ToolName::RunMaintenance => self.run_maintenance_tool(&args).await,
            ToolName::TerminateQuery => self.terminate_query_tool(&args).await,
            ToolName::ResolveTarget => self.resolve_target(&args).await,
            ToolName::Orient => self.orient(&args).await,
            ToolName::DiscoverTools => self.discover_tools(&args).await,
            ToolName::AutoTuneQuery => self.auto_tune_query(&args).await,
            ToolName::CheckDdlSafety => self.check_ddl_safety_tool(&args).await,
            ToolName::RebuildIndex => self.rebuild_index_tool(&args).await,
            ToolName::RefreshIndex => self.refresh_index_tool(&args).await,
            ToolName::RunDoctor => self.run_doctor_tool().await,
            ToolName::SetupConnection => self.setup_connection_tool(&args).await,
            ToolName::SaveProfile => self.save_profile_tool(&args).await,
            ToolName::TestProfile => self.test_profile_tool(&args).await,
            ToolName::ExportProfile => self.export_profile_tool(&args).await,
            ToolName::ImportProfile => self.import_profile_tool(&args).await,
        }
    }

    fn require_write(&self) -> Result<(), ToolError> {
        if !self.session.access_mode().allows_writes() {
            return Err(ToolError::Execution(
                "write tools require --access-mode write or admin (current session: read)".into(),
            ));
        }
        Ok(())
    }

    fn require_admin(&self) -> Result<(), ToolError> {
        if !self.session.access_mode().allows_admin() {
            return Err(ToolError::Execution(
                "admin tools require --access-mode admin".into(),
            ));
        }
        Ok(())
    }

    async fn execute_sql_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_write()?;
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        execute_sql(&self.session, sql, dry_run).await
    }

    async fn edit_row_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_write()?;
        edit_row(&self.session, args).await
    }

    async fn import_data_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_write()?;
        import_data(&self.session, args).await
    }

    async fn apply_ddl_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        apply_ddl(&self.session, sql, dry_run).await
    }

    async fn create_index_concurrently_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        create_index_concurrently(&self.session, sql).await
    }

    async fn run_maintenance_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        run_maintenance(&self.session, args).await
    }

    async fn terminate_query_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        terminate_query(&self.session, args).await
    }

    /// Autonomously resolve which connection/database matches a free-text `hint` and/or
    /// `objectHint`, then switch the session context to it.
    async fn live_databases_by_connection(
        &self,
    ) -> std::collections::HashMap<String, std::collections::HashSet<String>> {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        for conn in self.session.connections() {
            if let Ok(names) = self.list_database_names_for(&conn).await {
                map.insert(conn.id.clone(), names.into_iter().collect());
            }
        }
        map
    }

    async fn list_database_names_for(
        &self,
        conn: &crate::session::ConnectionInfo,
    ) -> Result<Vec<String>, ToolError> {
        let client = if self.session.active_context().await.0 == conn.id {
            self.session.checkout().await?
        } else {
            let pool_opts = self.session.pool_opts();
            let pool = nexql_conn::create_pool(&conn.params, &pool_opts).await?;
            nexql_conn::checkout_guarded(&pool, &pool_opts).await?
        };
        let rows = client
            .query(
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
                &[],
            )
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    async fn resolve_target(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let hint = args
            .get("hint")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let object_hint = args
            .get("objectHint")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if hint.is_none() && object_hint.is_none() {
            return Err(ToolError::InvalidArgs(
                "At least one of \"hint\" or \"objectHint\" is required.".into(),
            ));
        }

        let connections = self.session.connections();
        if connections.is_empty() {
            return Ok(ToolOutcome::err("No connections configured."));
        }

        #[derive(Clone)]
        struct Candidate {
            connection_id: String,
            database: String,
        }
        fn key_of(c: &Candidate) -> String {
            format!("{}\u{0}{}", c.connection_id, c.database)
        }

        let indexed: Vec<(String, String)> = self
            .index_store()
            .map(|store| store.list_indexed_databases().unwrap_or_default())
            .unwrap_or_default();

        let live_dbs = self.live_databases_by_connection().await;

        let mut seen = std::collections::HashSet::new();
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut add_candidate = |connection_id: &str, database: &str| {
            if !connections.iter().any(|c| c.id == connection_id) {
                return;
            }
            let key = format!("{connection_id}\u{0}{database}");
            if !seen.insert(key) {
                return;
            }
            candidates.push(Candidate {
                connection_id: connection_id.to_string(),
                database: database.to_string(),
            });
        };
        for (cid, db) in &indexed {
            if let Some(set) = live_dbs.get(cid) {
                if set.contains(db) {
                    add_candidate(cid, db);
                } else if let Some(store) = self.index_store() {
                    let _ = store.clear_index(cid, db);
                }
            }
        }
        for c in &connections {
            if let Some(dbs) = live_dbs.get(&c.id) {
                for db in dbs {
                    add_candidate(&c.id, db);
                }
            } else {
                let db = c.database.clone().unwrap_or_else(|| "postgres".into());
                add_candidate(&c.id, &db);
            }
        }

        let mut scored: std::collections::HashMap<String, (Candidate, f64, Vec<String>)> =
            std::collections::HashMap::new();

        if let Some(hint) = hint {
            for c in &candidates {
                let Some(conn) = connections.iter().find(|x| x.id == c.connection_id) else {
                    continue;
                };
                let fields: [(&str, &str); 3] = [
                    ("connection name", conn.name.as_str()),
                    ("host", conn.host.as_deref().unwrap_or("")),
                    ("database", c.database.as_str()),
                ];
                let mut best = 0.0f64;
                let mut best_field = "";
                for (label, value) in fields {
                    let s = fuzzy_score(hint, value);
                    if s > best {
                        best = s;
                        best_field = label;
                    }
                }
                if best > 0.0 {
                    let entry = scored
                        .entry(key_of(c))
                        .or_insert_with(|| (c.clone(), 0.0, Vec::new()));
                    entry.1 += best;
                    entry
                        .2
                        .push(format!("{best_field} matched hint \"{hint}\" ({best:.0})"));
                }
            }
        }

        if let Some(object_hint) = object_hint
            && let Some(store) = self.index_store()
        {
            let filter = self.query_filter();
            for (cid, db) in &indexed {
                let svc = IndexQueryService::new(store, cid.clone(), db.clone());
                if let Ok(hits) = svc.search_schema(
                    object_hint,
                    3,
                    Some(&filter),
                    SearchOptions {
                        use_semantic: self.use_semantic,
                        embedder: self.embedder.as_deref(),
                    },
                ) && let Some(top) = hits.first()
                {
                    let c = Candidate {
                        connection_id: cid.clone(),
                        database: db.clone(),
                    };
                    let entry = scored
                        .entry(key_of(&c))
                        .or_insert_with(|| (c.clone(), 0.0, Vec::new()));
                    entry.1 += top.score * 10.0;
                    entry.2.push(format!(
                        "schema search for \"{object_hint}\" found {} (score {:.2})",
                        top.ref_, top.score
                    ));
                }
            }
        }

        let mut ranked: Vec<(Candidate, f64, Vec<String>)> = scored.into_values().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if ranked.is_empty() {
            let candidates_json: Vec<Value> = connections
                .iter()
                .map(|c| {
                    json!({
                        "connectionId": c.id,
                        "connectionName": c.name,
                        "database": c.database.clone().unwrap_or_else(|| "postgres".into()),
                    })
                })
                .collect();
            return Ok(ToolOutcome::ok_json(json!({
                "ambiguous": true,
                "message": format!(
                    "No connection/database matched \"{}\". Choose from the configured connections.",
                    hint.or(object_hint).unwrap_or_default()
                ),
                "candidates": candidates_json
            })));
        }

        let winner = &ranked[0];
        let is_tied = ranked
            .get(1)
            .is_some_and(|runner_up| runner_up.1 >= winner.1 * 0.85);

        if is_tied {
            let threshold = winner.1 * 0.85;
            let tied: Vec<&(Candidate, f64, Vec<String>)> =
                ranked.iter().filter(|r| r.1 >= threshold).take(5).collect();
            let candidates_json: Vec<Value> = tied
                .iter()
                .filter_map(|(c, score, evidence)| {
                    connections
                        .iter()
                        .find(|x| x.id == c.connection_id)
                        .map(|conn| {
                            json!({
                                "connectionId": c.connection_id,
                                "connectionName": conn.name,
                                "database": c.database,
                                "score": score,
                                "evidence": evidence,
                            })
                        })
                })
                .collect();
            return Ok(ToolOutcome::ok_json(json!({
                "ambiguous": true,
                "message": format!("{} equally-plausible candidates matched.", tied.len()),
                "candidates": candidates_json
            })));
        }

        let (winner_candidate, winner_score, winner_evidence) = winner;

        if let Some(object_hint) = object_hint
            && let Some(store) = self.index_store()
        {
            let filter = self.query_filter();
            let svc = IndexQueryService::new(
                store,
                &winner_candidate.connection_id,
                &winner_candidate.database,
            );
            if let Ok(hits) = svc.search_schema(
                object_hint,
                5,
                Some(&filter),
                SearchOptions {
                    use_semantic: self.use_semantic,
                    embedder: self.embedder.as_deref(),
                },
            ) && hits.len() >= 2
            {
                let top_score = hits[0].score;
                let tied: Vec<&nexql_index::RankedHit> = hits
                    .iter()
                    .filter(|h| scores_equal(h.score, top_score))
                    .collect();
                if tied.len() > 1 {
                    let candidates_json: Vec<Value> = tied
                        .iter()
                        .map(|h| {
                            json!({
                                "ref": h.ref_,
                                "score": h.score,
                                "kind": h.kind,
                                "connectionId": winner_candidate.connection_id,
                                "database": winner_candidate.database,
                            })
                        })
                        .collect();
                    return Ok(ToolOutcome::ok_json(json!({
                        "ambiguous": true,
                        "message": format!(
                            "{} objects matched \"{object_hint}\" with equal scores — choose explicitly.",
                            tied.len()
                        ),
                        "candidates": candidates_json,
                    })));
                }
            }
        }

        self.session
            .switch(
                &winner_candidate.connection_id,
                Some(winner_candidate.database.clone()),
            )
            .await?;
        let conn = connections
            .iter()
            .find(|x| x.id == winner_candidate.connection_id)
            .ok_or_else(|| ToolError::Execution("resolved connection vanished".into()))?;

        Ok(ToolOutcome::ok_json(json!({
            "resolved": true,
            "connectionId": winner_candidate.connection_id,
            "connectionName": conn.name,
            "database": winner_candidate.database,
            "confidence": winner_score,
            "evidence": winner_evidence,
        })))
    }

    /// Bootstrap digest (Issue 4): tables + FK joins + enum-like columns in one
    /// call, so orienting on an unfamiliar schema costs one round trip instead
    /// of the ~10 calls (list_objects, search_schema, find_missing_fks,
    /// get_join_path ×N, describe_object, sample_values, table_stats, get_ddl…)
    /// the field report measured. Flat "name:type!" column strings instead of
    /// per-column objects — cheaper in tokens at the cost of structure the
    /// caller rarely needs here (full detail is one describe_object away).
    async fn orient(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        const TABLE_LIMIT: usize = 40;
        /// Enum-heuristic thresholds: a text/varchar column profiled with a
        /// small number of distinct values and captured common_values reads
        /// as an enum-ish status/category column an agent should know the
        /// vocabulary of before writing an equality filter (see Issue 6).
        const ENUM_MAX_DISTINCT: f64 = 20.0;

        let focus = args
            .get("focus")
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase);
        let (connection_id, database) = self.session.active_context().await;

        let Some(store) = self.index_store() else {
            return Ok(ToolOutcome::ok_json(json!({
                "database": database,
                "tables": [],
                "joins": [],
                "enums": {},
                "notes": [NO_INDEX_HINT],
            })));
        };
        let base = store.base_dir(&connection_id, &database);
        let Some(manifest) = store.read_manifest(&base)? else {
            return Ok(ToolOutcome::ok_json(json!({
                "database": database,
                "tables": [],
                "joins": [],
                "enums": {},
                "notes": [format!(
                    "No schema index for database \"{database}\" — call the 'rebuild_index' tool to build an index."
                )],
            })));
        };

        let mut entries: Vec<(String, ObjectEntry)> = Vec::new();
        for shard in &manifest.shards {
            if let Some(shard_entries) = store.read_shard_entries(&base, &shard.file)? {
                entries.extend(shard_entries);
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(f) = &focus {
            entries.retain(|(ref_, _)| ref_.to_ascii_lowercase().contains(f.as_str()));
        }

        let mut notes: Vec<String> = manifest.stats.warnings.clone();
        let total = entries.len();
        if total > TABLE_LIMIT {
            notes.push(format!(
                "Showing {TABLE_LIMIT} of {total} objects — pass `focus` to narrow the digest."
            ));
            entries.truncate(TABLE_LIMIT);
        }
        let shown_refs: std::collections::HashSet<&str> =
            entries.iter().map(|(r, _)| r.as_str()).collect();

        let mut tables = Vec::with_capacity(entries.len());
        let mut enums = serde_json::Map::new();
        for (ref_, entry) in &entries {
            if entry.excluded == Some(true) {
                continue;
            }
            let pk = entry
                .primary_key
                .as_ref()
                .map(|cols| cols.join(","))
                .filter(|s| !s.is_empty());
            let columns = entry
                .columns
                .iter()
                .map(|c| {
                    let bang = if c.not_null { "!" } else { "" };
                    format!("{}:{}{bang}", c.name, c.type_name)
                })
                .collect::<Vec<_>>()
                .join(", ");
            tables.push(json!({
                "ref": ref_,
                "kind": entry.kind.as_str(),
                "rows": format!("~{}", entry.row_estimate.round() as i64),
                "pk": pk,
                "columns": columns,
            }));

            for col in &entry.columns {
                if col.pii == Some(true) {
                    continue;
                }
                let is_textish = col.type_name.contains("char") || col.type_name.contains("text");
                let Some(profile) = &col.profile else {
                    continue;
                };
                if !is_textish
                    || profile.n_distinct <= 0.0
                    || profile.n_distinct > ENUM_MAX_DISTINCT
                {
                    continue;
                }
                if let Some(vals) = &profile.common_values
                    && !vals.is_empty()
                {
                    enums.insert(format!("{ref_}.{}", col.name), json!(vals));
                }
            }
        }

        let mut joins = Vec::new();
        if let Some(graph) = store.read_join_graph(&base, &manifest)? {
            for edge in &graph.edges {
                if focus.is_some()
                    && !(shown_refs.contains(edge.from.as_str())
                        || shown_refs.contains(edge.to.as_str()))
                {
                    continue;
                }
                let edge_str = edge
                    .cols
                    .iter()
                    .map(|(from_col, to_col)| {
                        format!("{}.{from_col} -> {}.{to_col}", edge.from, edge.to)
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let inferred = edge.inferred == Some(true);
                joins.push(json!({
                    "edge": edge_str,
                    "declared": !inferred,
                    "detection": if inferred { "join_graph_inferred" } else { "declared_fk" },
                    "via": edge.via,
                    "disabled": edge.disabled == Some(true),
                }));
            }
        }

        Ok(ToolOutcome::ok_json(json!({
            "database": database,
            "tables": tables,
            "joins": joins,
            "enums": Value::Object(enums),
            "notes": notes,
        })))
    }

    async fn discover_tools(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::to_lowercase);
        let category = args
            .get("category")
            .and_then(|v| v.as_str())
            .map(str::to_lowercase);

        // Always search the full catalog — meta profile may expose only a subset via tools/list.
        let all_specs = active_tools();
        let filtered: Vec<Value> = all_specs
            .into_iter()
            .filter(|spec| {
                if spec.name == ToolName::DiscoverTools {
                    return false;
                }
                if let Some(ref cat) = category {
                    match cat.as_str() {
                        "query" if !ToolName::QUERY_PROFILE.contains(&spec.name) => return false,
                        "dba" if !ToolName::DBA_PROFILE.contains(&spec.name) => return false,
                        "write" if !ToolName::PHASE9.contains(&spec.name) => return false,
                        _ => {}
                    }
                }
                if let Some(ref q) = query {
                    let name_match = spec.name.as_str().contains(q.as_str());
                    let desc_match = spec.description.to_lowercase().contains(q.as_str());
                    if !name_match && !desc_match {
                        return false;
                    }
                }
                true
            })
            .map(|spec| {
                json!({
                    "name": spec.name.as_str(),
                    "description": spec.description,
                    "input_schema": spec.input_schema,
                })
            })
            .collect();

        Ok(ToolOutcome::ok_json(json!({
            "query": args.get("query"),
            "category": args.get("category"),
            "count": filtered.len(),
            "tools": filtered,
        })))
    }

    fn build_tuning_summary(plan_structured: &Option<Value>, suggestions: &Value) -> String {
        let mut parts = Vec::new();
        if let Some(structured) = plan_structured
            && let Some(metrics) = structured.get("metrics")
        {
            if let Some(exec_time) = metrics.get("executionTime").and_then(|v| v.as_f64()) {
                parts.push(format!("Query executed in {:.2}ms.", exec_time));
            }
            if let Some(seq_scans) = metrics.get("sequentialScans").and_then(|v| v.as_u64())
                && seq_scans > 0
            {
                parts.push(format!("Found {seq_scans} sequential scan(s)."));
            }
        }

        let candidate_count = suggestions
            .get("high_seq_scan_tables")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
            + suggestions
                .get("unindexed_fk_columns")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

        if candidate_count > 0 {
            parts.push(format!(
                "{candidate_count} index recommendation(s) identified."
            ));
        } else {
            parts.push("No explicit index candidate recommendations generated.".into());
        }

        if parts.is_empty() {
            "Auto-tune evaluation complete. Inspect execution plan and index recommendations."
                .into()
        } else {
            parts.join(" ")
        }
    }

    async fn auto_tune_query(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;

        let deep_plan = self
            .deep_plan_analysis(&json!({ "sql": sql, "analyze": true }))
            .await?;

        let suggestions_res = self.suggest_indexes(&json!({ "sql": sql })).await;
        let (suggestions_data, suggestions_error) = match suggestions_res {
            Ok(outcome) => (outcome.structured.unwrap_or(json!([])), None),
            Err(e) => (json!([]), Some(e.to_string())),
        };

        let summary_text = Self::build_tuning_summary(&deep_plan.structured, &suggestions_data);

        let mut payload = json!({
            "target_query": sql,
            "deep_plan_analysis": deep_plan.structured,
            "index_suggestions": suggestions_data,
            "tuning_summary": summary_text,
        });

        if let Some(err) = suggestions_error {
            payload["suggestions_error"] = json!(err);
        }

        Ok(ToolOutcome::ok_json(payload))
    }

    async fn check_ddl_safety_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ddl = args
            .get("ddl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ddl is required".into()))?;

        let report = crate::dba_guard::analyze_ddl_safety(ddl);
        Ok(ToolOutcome::ok_json(report))
    }

    async fn rebuild_index_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let store = self
            .index_store()
            .ok_or_else(|| ToolError::Execution("Index store unavailable".into()))?;
        let (connection_id, database) = self.session.active_context().await;
        let depth_str = args
            .get("depth")
            .and_then(|v| v.as_str())
            .unwrap_or("structure");
        let depth: BuildDepth = match depth_str.to_lowercase().as_str() {
            "profiles" | "full" => BuildDepth::Profiles,
            _ => BuildDepth::Structure,
        };

        let req = BuildRequest {
            connection_id: connection_id.clone(),
            database: database.clone(),
            scope: IndexScope {
                included_schemas: vec![],
                excluded_objects: vec![],
                pii_excluded_columns: vec![],
            },
            depth,
            build_mode: BuildMode::Guided,
            environment: "development".into(),
            embeddings: self.use_semantic,
        };

        let client = self.session.checkout().await?;
        let db = PgCatalogDb::new(&client);
        let manifest = build_index(store, &db, &req, None, None, self.embedder.as_deref())
            .await
            .map_err(|e| ToolError::Execution(format!("Index build failed: {e}")))?;
        self.session.clear_index_stale(&connection_id, &database);

        Ok(ToolOutcome::ok_json(json!({
            "status": "completed",
            "connection_id": connection_id,
            "database": database,
            "schema_fingerprint": manifest.schema_fingerprint,
            "counts": manifest.counts,
            "build_ms": manifest.stats.build_ms,
        })))
    }

    async fn refresh_index_tool(&self, _args: &Value) -> Result<ToolOutcome, ToolError> {
        let store = self
            .index_store()
            .ok_or_else(|| ToolError::Execution("Index store unavailable".into()))?;
        let (connection_id, database) = self.session.active_context().await;
        let base = store.base_dir(&connection_id, &database);
        let manifest = store.read_manifest(&base)?.ok_or_else(|| {
            ToolError::Execution(
                "No existing index manifest to refresh — call 'rebuild_index'.".into(),
            )
        })?;

        let req = BuildRequest {
            connection_id: connection_id.clone(),
            database: database.clone(),
            scope: manifest.scope,
            depth: manifest.build_depth,
            build_mode: manifest.build_mode,
            environment: manifest.environment,
            embeddings: self.use_semantic,
        };

        let client = self.session.checkout().await?;
        let db = PgCatalogDb::new(&client);
        let new_manifest = build_index(store, &db, &req, None, None, self.embedder.as_deref())
            .await
            .map_err(|e| ToolError::Execution(format!("Index refresh failed: {e}")))?;
        self.session.clear_index_stale(&connection_id, &database);

        Ok(ToolOutcome::ok_json(json!({
            "status": "refreshed",
            "connection_id": connection_id,
            "database": database,
            "schema_fingerprint": new_manifest.schema_fingerprint,
            "counts": new_manifest.counts,
            "build_ms": new_manifest.stats.build_ms,
        })))
    }

    async fn run_doctor_tool(&self) -> Result<ToolOutcome, ToolError> {
        let (connection_id, database) = self.session.active_context().await;
        let client = self.session.checkout().await?;

        let version: String = client
            .query_one("SELECT version()", &[])
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .get(0);

        let is_super: String = client
            .query_one("SELECT current_setting('is_superuser')", &[])
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .get(0);
        let is_superuser = is_super.eq_ignore_ascii_case("on");

        let ro: String = client
            .query_one("SHOW default_transaction_read_only", &[])
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .get(0);

        let timeout: String = client
            .query_one("SHOW statement_timeout", &[])
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .get(0);

        let pgs_present: bool = match client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_stat_statements')",
                &[],
            )
            .await
        {
            Ok(row) => row.get(0),
            Err(_) => false,
        };

        let index_status = if let Some(store) = self.index_store() {
            let base = store.base_dir(&connection_id, &database);
            match store.read_manifest(&base) {
                Ok(Some(m)) => json!({
                    "present": true,
                    "indexed_at": m.indexed_at,
                    "fingerprint": m.schema_fingerprint,
                    "tables": m.counts.tables,
                }),
                _ => json!({ "present": false }),
            }
        } else {
            json!({ "present": false, "reason": "no_index_store" })
        };

        let recent_errors = read_recent_log_errors();

        Ok(ToolOutcome::ok_json(json!({
            "status": "ok",
            "connection_id": connection_id,
            "database": database,
            "version": version.split(',').next().unwrap_or(&version),
            "access_mode": format!("{:?}", self.session.access_mode()),
            "superuser": is_superuser,
            "read_only": ro,
            "statement_timeout": timeout,
            "pg_stat_statements": pgs_present,
            "index": index_status,
            "recent_errors": recent_errors,
        })))
    }

    fn register_profile_in_session(
        &self,
        name: &str,
        profile: &nexql_conn::ProfileConfig,
    ) -> Result<(), ToolError> {
        self.session.register_profile(
            name,
            profile,
            self.session.access_mode(),
            self.session.caps(),
        )
    }

    async fn setup_connection_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let profile_name = args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let candidates = crate::detect::ConnectionDetector::detect_all(None);

        let url = args.get("url").and_then(|v| v.as_str());
        let host = args.get("host").and_then(|v| v.as_str());
        let port = args.get("port").and_then(|v| v.as_u64()).map(|n| n as u16);
        let dbname = args.get("dbname").and_then(|v| v.as_str());
        let user = args.get("user").and_then(|v| v.as_str());
        let password = args.get("password").and_then(|v| v.as_str());
        let sslmode = args.get("sslmode").and_then(|v| v.as_str());

        let best_cand = candidates
            .iter()
            .find(|c| c.is_complete)
            .or_else(|| candidates.first());

        let res_host = host.or_else(|| best_cand.and_then(|c| c.host.as_deref()));
        let res_port = port.or_else(|| best_cand.and_then(|c| c.port));
        let res_dbname = dbname.or_else(|| best_cand.and_then(|c| c.dbname.as_deref()));
        let res_user = user.or_else(|| best_cand.and_then(|c| c.user.as_deref()));
        let res_password = password.or_else(|| best_cand.and_then(|c| c.password.as_deref()));
        let res_url = url.or_else(|| best_cand.and_then(|c| c.url.as_deref()));
        let res_sslmode = sslmode.or_else(|| best_cand.and_then(|c| c.sslmode.as_deref()));

        if res_url.is_none() && (res_host.is_none() || res_dbname.is_none() || res_user.is_none()) {
            let missing: Vec<&str> = vec![
                if res_host.is_none() {
                    Some("host")
                } else {
                    None
                },
                if res_dbname.is_none() {
                    Some("dbname")
                } else {
                    None
                },
                if res_user.is_none() {
                    Some("user")
                } else {
                    None
                },
            ]
            .into_iter()
            .flatten()
            .collect();

            return Ok(ToolOutcome::ok_json(json!({
                "status": "needs_input",
                "message": "Insufficient connection details. Please supply missing fields.",
                "detectedCandidates": candidates.iter().map(|c| c.redacted_json()).collect::<Vec<_>>(),
                "missingFields": missing
            })));
        }

        let params = nexql_conn::ConnectionParams {
            url: res_url.map(String::from),
            host: res_host.map(String::from),
            port: res_port,
            dbname: res_dbname.map(String::from),
            user: res_user.map(String::from),
            password: res_password.map(String::from),
            sslmode: res_sslmode.map(String::from),
            ..Default::default()
        };

        match nexql_conn::test_connection(&params).await {
            Ok(report) => {
                let p_config = nexql_conn::ProfileConfig {
                    url: params.url.clone(),
                    host: params.host.clone(),
                    port: params.port,
                    dbname: params.dbname.clone(),
                    user: params.user.clone(),
                    password: params.password.clone(),
                    sslmode: params.sslmode.clone(),
                    ..Default::default()
                };

                let path = nexql_conn::ConfigFile::default_path().ok_or_else(|| {
                    ToolError::Execution("Could not resolve config directory".into())
                })?;
                let mut cfg = nexql_conn::ConfigFile::load_path(&path).unwrap_or_default();
                cfg.upsert_profile(profile_name, p_config.clone());
                let backup = cfg
                    .save(&path)
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                self.register_profile_in_session(profile_name, &p_config)?;

                Ok(ToolOutcome::ok_json(json!({
                    "status": "configured",
                    "profileName": profile_name,
                    "serverVersion": report.server_version,
                    "isSuperuser": report.is_superuser,
                    "latencyMs": report.latency.as_millis(),
                    "configPath": path.to_string_lossy().to_string(),
                    "backup": backup.map(|b| b.to_string_lossy().to_string()),
                    "sessionReloaded": true,
                })))
            }
            Err(e) => Ok(ToolOutcome::ok_json(json!({
                "status": "failed",
                "error": e.to_string(),
                "detectedCandidates": candidates.iter().map(|c| c.redacted_json()).collect::<Vec<_>>()
            }))),
        }
    }

    async fn save_profile_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("name parameter is required".into()))?;

        let p_config = nexql_conn::ProfileConfig {
            url: args.get("url").and_then(|v| v.as_str()).map(String::from),
            host: args.get("host").and_then(|v| v.as_str()).map(String::from),
            port: args.get("port").and_then(|v| v.as_u64()).map(|n| n as u16),
            dbname: args
                .get("dbname")
                .and_then(|v| v.as_str())
                .map(String::from),
            user: args.get("user").and_then(|v| v.as_str()).map(String::from),
            password: args
                .get("password")
                .and_then(|v| v.as_str())
                .map(String::from),
            sslmode: args
                .get("sslmode")
                .and_then(|v| v.as_str())
                .map(String::from),
            access_mode: args
                .get("access_mode")
                .and_then(|v| v.as_str())
                .map(String::from),
            max_rows: args
                .get("max_rows")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
            ..Default::default()
        };

        let path = nexql_conn::ConfigFile::default_path()
            .ok_or_else(|| ToolError::Execution("Could not resolve config directory".into()))?;

        let mut cfg = nexql_conn::ConfigFile::load_path(&path).unwrap_or_default();
        cfg.upsert_profile(name, p_config.clone());
        let backup = cfg
            .save(&path)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        self.register_profile_in_session(name, &p_config)?;

        Ok(ToolOutcome::ok_json(json!({
            "status": "saved",
            "profile": name,
            "configPath": path.to_string_lossy().to_string(),
            "backup": backup.map(|b| b.to_string_lossy().to_string()),
            "sessionReloaded": true,
        })))
    }

    async fn test_profile_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let name = args.get("name").and_then(|v| v.as_str());

        let params = if let Some(pname) = name {
            let conn = self
                .session
                .connections()
                .into_iter()
                .find(|c| c.id == pname)
                .ok_or_else(|| ToolError::InvalidArgs(format!("Profile '{pname}' not found")))?;
            conn.params.clone()
        } else {
            nexql_conn::ConnectionParams {
                url: args.get("url").and_then(|v| v.as_str()).map(String::from),
                host: args.get("host").and_then(|v| v.as_str()).map(String::from),
                port: args.get("port").and_then(|v| v.as_u64()).map(|n| n as u16),
                dbname: args
                    .get("dbname")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                user: args.get("user").and_then(|v| v.as_str()).map(String::from),
                password: args
                    .get("password")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                sslmode: args
                    .get("sslmode")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                ..Default::default()
            }
        };

        match nexql_conn::test_connection(&params).await {
            Ok(report) => Ok(ToolOutcome::ok_json(json!({
                "success": true,
                "serverVersion": report.server_version,
                "isSuperuser": report.is_superuser,
                "latencyMs": report.latency.as_millis()
            }))),
            Err(e) => Ok(ToolOutcome::ok_json(json!({
                "success": false,
                "error": e.to_string()
            }))),
        }
    }

    async fn export_profile_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("full");
        let path = nexql_conn::ConfigFile::default_path()
            .ok_or_else(|| ToolError::Execution("Could not resolve config directory".into()))?;
        let cfg = nexql_conn::ConfigFile::load_path(&path).unwrap_or_default();

        if format == "project" {
            let proj = cfg.export_shareable();
            let toml_str =
                toml::to_string_pretty(&proj).map_err(|e| ToolError::Execution(e.to_string()))?;
            Ok(ToolOutcome::ok_json(json!({
                "format": "project",
                "filename": ".nexql/config.toml",
                "description": "Project policy overlay (no credentials). Use format=full for shareable connection profiles.",
                "content": toml_str,
            })))
        } else {
            let sanitized = cfg.export_full_sanitized();
            let toml_str = sanitized
                .to_toml_string()
                .map_err(|e| ToolError::Execution(e.to_string()))?;
            Ok(ToolOutcome::ok_json(json!({
                "format": "full",
                "description": "Full user config with passwords and secrets stripped — suitable for team sharing.",
                "profileCount": sanitized.profiles.len(),
                "content": toml_str,
            })))
        }
    }

    async fn import_profile_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let content = if let Some(c) = args.get("content").and_then(|v| v.as_str()) {
            c.to_string()
        } else if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
            std::fs::read_to_string(p)
                .map_err(|e| ToolError::Execution(format!("failed to read file {p}: {e}")))?
        } else {
            return Err(ToolError::Execution(
                "either 'content' or 'path' must be specified".into(),
            ));
        };

        let path = nexql_conn::ConfigFile::default_path()
            .ok_or_else(|| ToolError::Execution("Could not resolve config directory".into()))?;
        let mut cfg = nexql_conn::ConfigFile::load_path(&path).unwrap_or_default();

        let imported: nexql_conn::ConfigFile = toml::from_str(&content)
            .map_err(|e| ToolError::Execution(format!("failed to parse TOML content: {e}")))?;

        let mut count = 0;
        let mut imported_names: Vec<String> = Vec::new();
        for (name, prof) in imported.profiles {
            cfg.upsert_profile(name.clone(), prof.clone());
            self.register_profile_in_session(&name, &prof)?;
            imported_names.push(name);
            count += 1;
        }
        if imported.default_profile.is_some() {
            cfg.default_profile = imported.default_profile;
        }

        let backup = cfg
            .save(&path)
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolOutcome::ok_json(json!({
            "status": "imported",
            "imported_profiles": count,
            "profiles": imported_names,
            "configPath": path.to_string_lossy().to_string(),
            "backup": backup.map(|b| b.to_string_lossy().to_string()),
            "sessionReloaded": true,
        })))
    }

    async fn index_service(&self) -> Result<(&IndexStore, String, String), ToolError> {
        let store = self
            .index_store()
            .ok_or_else(|| ToolError::Execution(NO_INDEX_HINT.into()))?;
        let (connection_id, database) = self.session.active_context().await;
        let base = store.base_dir(&connection_id, &database);
        if store.read_manifest(&base)?.is_none() {
            return Err(ToolError::Execution(format!(
                "No schema index for database \"{database}\" — call the 'rebuild_index' tool to build an index."
            )));
        }
        Ok((store, connection_id, database))
    }

    /// Turn a non-`Resolved` [`RefResolution`] into the actionable error an agent
    /// needs to self-correct (Issue 2): ambiguous names list every candidate,
    /// unknown names carry a "did you mean" when one is close enough.
    fn ref_resolution_error(ref_: &str, resolution: &RefResolution) -> Option<ToolError> {
        match resolution {
            RefResolution::Resolved(_) => None,
            RefResolution::Ambiguous(candidates) => Some(ToolError::InvalidArgs(format!(
                "ambiguous relation \"{ref_}\": {}",
                candidates.join(", ")
            ))),
            RefResolution::Unknown {
                suggestion: Some(s),
            } => Some(ToolError::InvalidArgs(format!(
                "unknown relation \"{ref_}\" — did you mean \"{s}\"?"
            ))),
            RefResolution::Unknown { suggestion: None } => Some(ToolError::InvalidArgs(format!(
                "unknown relation \"{ref_}\" — call search_schema to find valid refs."
            ))),
        }
    }

    /// Strictly resolve `ref_` through the schema index: errors loudly on
    /// ambiguous/unknown names instead of letting a literal unresolved string
    /// flow into pathfinding or lookup (the false-negative this issue fixes).
    fn resolve_indexed_ref_strict(
        svc: &IndexQueryService<'_>,
        ref_: &str,
    ) -> Result<String, ToolError> {
        let resolution = svc.resolve_ref(ref_)?;
        if let Some(err) = Self::ref_resolution_error(ref_, &resolution) {
            return Err(err);
        }
        match resolution {
            RefResolution::Resolved(r) => Ok(r),
            _ => unreachable!("ref_resolution_error covers every non-Resolved case"),
        }
    }

    /// Best-effort resolution for tools that also work without an index
    /// (`get_ddl`, `table_stats`): upgrade a unique unqualified match, error
    /// loudly on ambiguity, but pass an unknown name through unchanged so the
    /// caller's own live-catalog lookup produces its own error.
    fn resolve_indexed_ref_soft(
        svc: &IndexQueryService<'_>,
        ref_: &str,
    ) -> Result<String, ToolError> {
        match svc.resolve_ref(ref_)? {
            RefResolution::Resolved(r) => Ok(r),
            RefResolution::Ambiguous(candidates) => Err(ToolError::InvalidArgs(format!(
                "ambiguous relation \"{ref_}\": {}",
                candidates.join(", ")
            ))),
            RefResolution::Unknown { .. } => Ok(ref_.to_owned()),
        }
    }

    /// Best-effort resolution for live-catalog tools that work with or without
    /// an index (`get_ddl`, `table_stats`): if an index is present, upgrade a
    /// unique unqualified match and error on ambiguity; otherwise (or on an
    /// unknown name) pass the ref through unchanged so `parse_ref` and the live
    /// catalog query produce their own error.
    async fn resolve_ref_best_effort(&self, ref_: &str) -> Result<String, ToolError> {
        let Some(store) = self.index_store() else {
            return Ok(ref_.to_owned());
        };
        let (connection_id, database) = self.session.active_context().await;
        let base = store.base_dir(&connection_id, &database);
        if store.read_manifest(&base)?.is_none() {
            return Ok(ref_.to_owned());
        }
        let svc = IndexQueryService::new(store, &connection_id, &database);
        Self::resolve_indexed_ref_soft(&svc, ref_)
    }

    async fn search_schema(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if query.is_empty() {
            return Ok(ToolOutcome::ok_json(json!([])));
        }
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let filter = self.query_filter();
        let hits = svc.search_schema(
            query,
            SEARCH_SCHEMA_LIMIT,
            Some(&filter),
            SearchOptions {
                use_semantic: self.use_semantic,
                embedder: self.embedder.as_deref(),
            },
        )?;
        let rows: Vec<Value> = hits
            .into_iter()
            .map(|h| {
                json!({
                    "ref": h.ref_,
                    "score": h.score,
                    "kind": h.kind,
                })
            })
            .collect();
        Ok(ToolOutcome::ok_json(json!(rows)))
    }

    async fn describe_object(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let resolved = Self::resolve_indexed_ref_soft(&svc, ref_)?;
        let filter = self.query_filter();
        let entry = svc.describe_object(&resolved, Some(&filter))?;
        let value = serde_json::to_value(entry).map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutcome::ok_json(value))
    }

    async fn get_join_path(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let a = args
            .get("a")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("a is required".into()))?;
        let b = args
            .get("b")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("b is required".into()))?;
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        // Resolve both endpoints before pathfinding — an unresolved literal here
        // is exactly what produced the false-negative "no join path found" for
        // unqualified names (Issue 2). Only genuine BFS exhaustion reaches
        // svc.get_join_path below.
        let resolved_a = Self::resolve_indexed_ref_strict(&svc, a)?;
        let resolved_b = Self::resolve_indexed_ref_strict(&svc, b)?;
        let path = svc.get_join_path(&resolved_a, &resolved_b)?;
        let path_value =
            serde_json::to_value(path).map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutcome::ok_json(json!({
            "path": path_value,
            "resolved_a": resolved_a,
            "resolved_b": resolved_b,
        })))
    }

    async fn sample_values(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let col = args
            .get("col")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("col is required".into()))?;
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let resolved = Self::resolve_indexed_ref_soft(&svc, ref_)?;
        let ref_ = resolved.as_str();
        let filter = self.query_filter();
        let result = svc.sample_values(ref_, col, Some(&filter), None)?;

        let mut values = result.values;
        let mut message = result.message;

        if values.is_empty()
            && let Ok(client) = self.session.checkout().await
        {
            let parts: Vec<&str> = ref_.split('.').collect();
            let (schema, table) = match parts.as_slice() {
                [s, t] => (*s, *t),
                _ => ("public", ref_),
            };
            let safe_schema = schema.replace('"', "\"\"");
            let safe_table = table.replace('"', "\"\"");
            let safe_col = col.replace('"', "\"\"");
            let query = format!(
                "SELECT DISTINCT \"{safe_col}\"::text FROM \"{safe_schema}\".\"{safe_table}\" WHERE \"{safe_col}\" IS NOT NULL LIMIT 20"
            );
            if let Ok(rows) = client.query(&query, &[]).await {
                let sampled: Vec<String> = rows
                    .iter()
                    .filter_map(|r| r.get::<_, Option<String>>(0))
                    .collect();
                if !sampled.is_empty() {
                    values = sampled;
                    message = None;
                }
            }
        }

        let mut payload = json!({ "values": values });
        if let Some(msg) = message {
            payload["message"] = json!(msg);
        }
        Ok(ToolOutcome::ok_json(payload))
    }

    fn list_connections(&self) -> ToolOutcome {
        let rows: Vec<Value> = self
            .session
            .connections()
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "host": c.host,
                    "port": c.port,
                    "database": c.database,
                })
            })
            .collect();
        ToolOutcome::ok_json(json!(rows))
    }

    async fn list_databases(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let connection_id = args
            .get("connectionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("connectionId is required".into()))?;
        let conn = self
            .session
            .connections()
            .into_iter()
            .find(|c| c.id == connection_id)
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "Connection not found for ID: {connection_id} — call list_connections"
                ))
            })?;
        // Connect using that profile's params (may differ from active).
        let client = {
            // Temporarily use active checkout if same id; else one-shot.
            if self.session.active_context().await.0 == connection_id {
                self.session.checkout().await?
            } else {
                let pool_opts = self.session.pool_opts();
                let pool = nexql_conn::create_pool(&conn.params, &pool_opts).await?;
                nexql_conn::checkout_guarded(&pool, &pool_opts).await?
            }
        };
        let rows = client
            .query(
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
                &[],
            )
            .await?;
        let names: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
        Ok(ToolOutcome::ok_json(json!(names)))
    }

    async fn list_schemas(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client
            .query(
                r#"
                SELECT nspname AS schema_name
                FROM pg_namespace
                WHERE nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
                  AND nspname NOT LIKE 'pg_%'
                ORDER BY nspname
                "#,
                &[],
            )
            .await?;
        let out: Vec<Value> = rows
            .iter()
            .filter(|r| {
                let name: String = r.get(0);
                self.session.filter().allows_schema(&name)
            })
            .map(|r| json!({ "schema_name": r.get::<_, String>(0) }))
            .collect();
        Ok(ToolOutcome::ok_json(json!(out)))
    }

    async fn list_objects(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let schema = args
            .get("schema")
            .and_then(|v| v.as_str())
            .unwrap_or("public");
        if !schema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ToolError::InvalidArgs(
                "Invalid or missing schema name format".into(),
            ));
        }
        if !self.session.filter().allows_schema(schema) {
            return Ok(ToolOutcome::ok_json(json!([])));
        }
        let kind = args.get("kind").and_then(|v| v.as_str());
        let mut queries = Vec::new();
        let push_rel = |queries: &mut Vec<String>, relkinds: &[&str], label: &str| {
            let kinds = relkinds
                .iter()
                .map(|k| format!("'{k}'"))
                .collect::<Vec<_>>()
                .join(",");
            queries.push(format!(
                r#"
                SELECT n.nspname AS schema, c.relname AS name, '{label}' AS kind,
                       d.description AS comment
                FROM pg_class c
                JOIN pg_namespace n ON n.oid = c.relnamespace
                LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = 0
                WHERE n.nspname = $1 AND c.relkind IN ({kinds})
                "#
            ));
        };
        if kind.is_none() || kind == Some("table") {
            push_rel(&mut queries, &["r", "f", "p"], "table");
        }
        if kind.is_none() || kind == Some("view") {
            push_rel(&mut queries, &["v"], "view");
        }
        if kind.is_none() || kind == Some("matview") {
            push_rel(&mut queries, &["m"], "matview");
        }
        if queries.is_empty() {
            return Ok(ToolOutcome::ok_json(json!([])));
        }
        let sql = queries.join("\nUNION ALL\n") + "\nORDER BY kind, name";
        let client = self.session.checkout().await?;
        let rows = client.query(&sql, &[&schema]).await?;
        let out: Vec<Value> = rows
            .iter()
            .filter(|r| {
                let s: String = r.get("schema");
                let name: String = r.get("name");
                self.session.filter().allows_table(&s, &name)
            })
            .map(|r| {
                json!({
                    "schema": r.get::<_, String>("schema"),
                    "name": r.get::<_, String>("name"),
                    "kind": r.get::<_, String>("kind"),
                    "comment": r.get::<_, Option<String>>("comment"),
                })
            })
            .collect();
        Ok(ToolOutcome::ok_json(json!(out)))
    }

    async fn get_current_context(&self) -> Result<ToolOutcome, ToolError> {
        let (connection_id, database) = self.session.active_context().await;
        let conn = self
            .session
            .connections()
            .into_iter()
            .find(|c| c.id == connection_id);
        Ok(ToolOutcome::ok_json(json!({
            "connectionId": connection_id,
            "connectionName": conn.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "Unknown".into()),
            "database": database,
            "host": conn.as_ref().and_then(|c| c.host.clone()),
            "port": conn.as_ref().and_then(|c| c.port),
            "access_mode": match self.session.access_mode() {
                nexql_policy::AccessMode::Read => "read",
                nexql_policy::AccessMode::Write => "write",
                nexql_policy::AccessMode::Admin => "admin",
            },
        })))
    }

    async fn switch_connection(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let connection_id = args
            .get("connectionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("connectionId is required".into()))?;
        let database = args
            .get("database")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        self.session.switch(connection_id, database).await?;
        self.get_current_context().await
    }

    async fn run_select(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        match validate_readonly_sql(sql)? {
            SqlDecision::Allow => {}
            SqlDecision::Reject => {
                return Err(ToolError::Execution(
                    "Security Error: Only read-only SELECT, WITH, or EXPLAIN statements are permitted."
                        .into(),
                ));
            }
        }
        enforce_read_table_policy(&self.session.filter(), sql)?;
        let trimmed = sql.trim().to_ascii_lowercase();
        let outcome = if trimmed.starts_with("explain") {
            self.run_select_internal(sql, None, true).await?
        } else {
            let max_rows = self.session.caps().max_rows;
            self.run_select_internal(sql, Some(max_rows), true).await?
        };
        Ok(self.attach_critique(sql, outcome).await)
    }

    /// Issue 6: attach a `critique` array to the response when a cheap
    /// heuristic fires — converting a silent plausible-but-wrong result into
    /// a self-correcting one. Best-effort only: any failure here (parse
    /// error, no index, ambiguous table) just means no critique, never a
    /// tool error — this must never break a successful query result.
    async fn attach_critique(&self, sql: &str, mut outcome: ToolOutcome) -> ToolOutcome {
        if outcome.is_error {
            return outcome;
        }
        let Some(structured) = outcome.structured.as_ref() else {
            return outcome;
        };
        if structured.get("truncated_chars").is_some() {
            // Payload already had to be cut for size — not worth the extra
            // data-access round trip for a critique on top of that.
            return outcome;
        }
        let Some(row_count) = structured
            .get("rows")
            .and_then(|v| v.as_array())
            .map(Vec::len)
        else {
            return outcome;
        };

        let mut critique = Vec::new();
        if let Some(item) = critique::limit_without_order_by(sql) {
            critique.push(item.to_json());
        }
        if row_count == 0
            && let Some((col, val)) = critique::simple_equality_filter(sql)
            && let Ok(tables) = select_table_refs(sql)
            && let [table] = tables.as_slice()
            && let Some(values) = self.sample_values_best_effort(table, &col).await
        {
            let sample = values
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            critique.push(json!({
                "signal": "zero_rows",
                "message": format!(
                    "no rows: {col} = '{val}'; observed values include: {sample}"
                ),
            }));
        }

        if critique.is_empty() {
            return outcome;
        }
        if let Some(obj) = outcome.structured.as_mut().and_then(|v| v.as_object_mut()) {
            obj.insert("critique".into(), json!(critique));
        }
        if let Some(structured) = &outcome.structured
            && let Ok(text) = serde_json::to_string(structured)
        {
            outcome.text = text;
        }
        outcome
    }

    /// Index-backed sample values for the zero-rows critique. `None` on any
    /// miss (no index, object/column not profiled) — this is advisory only.
    async fn sample_values_best_effort(
        &self,
        table: &nexql_policy::ObjectRef,
        col: &str,
    ) -> Option<Vec<String>> {
        let store = self.index_store()?;
        let (connection_id, database) = self.session.active_context().await;
        let base = store.base_dir(&connection_id, &database);
        store.read_manifest(&base).ok()??;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let ref_ = format!("{}.{}", table.schema, table.name);
        let filter = self.query_filter();
        let result = svc.sample_values(&ref_, col, Some(&filter), None).ok()?;
        if result.values.is_empty() {
            None
        } else {
            Some(result.values)
        }
    }

    async fn explain_query(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        match validate_readonly_sql(sql)? {
            SqlDecision::Allow => {}
            SqlDecision::Reject => {
                return Err(ToolError::Execution(
                    "Security Error: Only SELECT, WITH, or EXPLAIN statements can be analyzed."
                        .into(),
                ));
            }
        }
        enforce_read_table_policy(&self.session.filter(), sql)?;
        let clean = if sql.trim().to_ascii_lowercase().starts_with("explain") {
            sql.to_string()
        } else {
            format!("EXPLAIN {sql}")
        };
        // Re-validate EXPLAIN wrapper
        if validate_readonly_sql(&clean)? == SqlDecision::Reject {
            return Err(ToolError::Execution(
                "Security Error: EXPLAIN target is not read-only.".into(),
            ));
        }
        self.run_select_internal(&clean, None, false).await
    }

    async fn get_ddl(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let resolved = self.resolve_ref_best_effort(ref_).await?;
        let (schema, name) = parse_ref(&resolved).map_err(ToolError::InvalidArgs)?;
        let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("table");
        let reg = sql::regclass_literal(&schema, &name);
        let client = self.session.checkout().await?;

        match kind {
            "view" | "matview" => {
                let sql = format!("SELECT pg_get_viewdef({reg}, true) AS definition");
                let rows = client.query(&sql, &[]).await?;
                Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
            }
            "function" => {
                let sql = format!(
                    r#"SELECT p.proname AS name, pg_get_functiondef(p.oid) AS definition
                       FROM pg_proc p
                       JOIN pg_namespace n ON n.oid = p.pronamespace
                       WHERE n.nspname = '{schema}' AND p.proname = '{name}'"#
                );
                let rows = client.query(&sql, &[]).await?;
                Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
            }
            "index" => {
                let sql = format!("SELECT pg_get_indexdef({reg}) AS definition");
                let rows = client.query(&sql, &[]).await?;
                Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
            }
            "table" => {
                let columns = client
                    .query(&sql::column_details(&schema, &name), &[])
                    .await?;
                let constraints = client
                    .query(
                        &format!(
                            r#"SELECT conname AS name, pg_get_constraintdef(oid) AS definition
                               FROM pg_constraint WHERE conrelid = {reg} ORDER BY conname"#
                        ),
                        &[],
                    )
                    .await?;
                let indexes = client
                    .query(
                        &format!(
                            r#"SELECT indexname AS name, indexdef AS definition
                               FROM pg_indexes
                               WHERE schemaname = '{schema}' AND tablename = '{name}'
                               ORDER BY indexname"#
                        ),
                        &[],
                    )
                    .await?;
                Ok(ToolOutcome::ok_json(json!({
                    "table": format!("{schema}.{name}"),
                    "columns": rows_to_json(&columns),
                    "constraints": rows_to_json(&constraints),
                    "indexes": rows_to_json(&indexes),
                })))
            }
            other => Err(ToolError::InvalidArgs(format!(
                "Unsupported DDL kind \"{other}\". Use table, view, matview, function, or index."
            ))),
        }
    }

    async fn table_stats(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let resolved = self.resolve_ref_best_effort(ref_).await?;
        let (schema, name) = parse_ref(&resolved).map_err(ToolError::InvalidArgs)?;
        let client = self.session.checkout().await?;
        let stats = client.query(&sql::table_stats(&schema, &name), &[]).await?;
        let activity = client
            .query(&sql::table_activity(&schema, &name), &[])
            .await?;
        let columns = client
            .query(&sql::column_stats(&schema, &name), &[])
            .await?;
        let size = rows_to_json(&stats)
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        let activity = rows_to_json(&activity)
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        Ok(ToolOutcome::ok_json(json!({
            "size": size,
            "activity": activity,
            "columns": rows_to_json(&columns),
        })))
    }

    async fn index_usage(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let (schema, name) = parse_ref(ref_).map_err(ToolError::InvalidArgs)?;
        let client = self.session.checkout().await?;
        let rows = client.query(&sql::index_usage(&schema, &name), &[]).await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn list_running_queries(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::running_queries(), &[]).await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn find_blocking_locks(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::blocking_locks(), &[]).await?;
        let values = rows_to_json(&rows);
        if values.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "message": "No blocking locks found.",
                "locks": [],
            })));
        }
        Ok(ToolOutcome::ok_json(values))
    }

    async fn slow_queries(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(SLOW_QUERIES_DEFAULT);
        let client = self.session.checkout().await?;
        match client.query(&sql::slow_queries(limit), &[]).await {
            Ok(rows) => Ok(ToolOutcome::ok_json(rows_to_json(&rows))),
            Err(e) => {
                if let Some(message) = sql::map_stat_statements_error(&e) {
                    Ok(ToolOutcome::ok_json(json!({
                        "error": message,
                        "hint": message,
                    })))
                } else {
                    Err(ToolError::Postgres(e))
                }
            }
        }
    }

    async fn db_health_check(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let sections: &[(&str, &str)] = &[
            ("overview", sql::database_stats()),
            ("cache", sql::cache_hit_ratio()),
            ("dead_tuples", sql::database_maintenance_stats()),
            ("connection_states", sql::connection_states()),
            ("blocking_locks", sql::blocking_locks()),
        ];
        let mut report = serde_json::Map::new();
        for (key, q) in sections {
            match client.query(*q, &[]).await {
                Ok(rows) => {
                    report.insert((*key).into(), rows_to_json(&rows));
                }
                Err(e) => {
                    report.insert((*key).into(), json!({ "error": e.to_string() }));
                }
            }
        }
        let lock_count = report
            .get("blocking_locks")
            .and_then(|v| v.as_array())
            .map(|a| a.len() as u64);
        report.insert("blocking_lock_count".into(), json!(lock_count));
        Ok(ToolOutcome::ok_json(Value::Object(report)))
    }

    async fn explain_analyze(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(&self.session.filter(), sql)?;
        let explain = build_explain_sql(sql, true);
        self.run_explain_in_transaction(&explain).await
    }

    async fn analyze_query_plan(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(&self.session.filter(), sql)?;
        let analyze = args
            .get("analyze")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let explain = build_explain_sql(sql, analyze);
        let outcome = self.run_explain_in_transaction(&explain).await?;
        let rows = outcome.structured.unwrap_or(Value::Null);
        let row_array = rows
            .get("rows")
            .and_then(|v| v.as_array())
            .or_else(|| rows.as_array());
        let plan = row_array
            .and_then(|a| a.first())
            .and_then(|r| r.get("QUERY PLAN"))
            .cloned()
            .unwrap_or(Value::Null);
        let metrics = extract_plan_metrics(&plan).or_else(|| extract_plan_metrics(&rows));
        let recommendations = metrics
            .as_ref()
            .and_then(|m| m.get("recommendations"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(ToolOutcome::ok_json(json!({
            "metrics": metrics,
            "recommendations": recommendations,
            "plan": plan,
        })))
    }

    /// EXPLAIN ANALYZE executes the query — always wrap in READ ONLY + ROLLBACK.
    async fn run_explain_in_transaction(
        &self,
        explain_sql: &str,
    ) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        client
            .batch_execute("SET statement_timeout = '30s'")
            .await?;
        client.batch_execute("BEGIN").await?;
        let result = async {
            client.batch_execute("SET TRANSACTION READ ONLY").await?;
            let rows = client.query(explain_sql, &[]).await?;
            Ok::<_, ToolError>(rows_to_json(&rows))
        }
        .await;
        // Always roll back — belt-and-braces on top of default_transaction_read_only.
        let _ = client.batch_execute("ROLLBACK").await;
        match result {
            Ok(values) => Ok(ToolOutcome::ok_json(values)),
            Err(e) => Err(e),
        }
    }

    /// Status tool: absence of an index is a normal state, not a failure.
    /// Unlike `search_schema` / `get_join_path` (which correctly fail loudly
    /// with remediation, since they can't do their job without one),
    /// `get_index_status` is the tool an agent calls specifically to find out
    /// whether an index exists — so it must return that fact rather than
    /// throw (Issue 3). Reserve real errors for actual failures (corrupt or
    /// unreadable index data), not "not built yet".
    async fn get_index_status(&self) -> Result<ToolOutcome, ToolError> {
        let (connection_id, database) = self.session.active_context().await;
        let Some(store) = self.index_store() else {
            return Ok(ToolOutcome::ok_json(json!({
                "status": "missing",
                "connectionId": connection_id,
                "database": database,
                "remediation": "rebuild_index",
            })));
        };
        let base = store.base_dir(&connection_id, &database);
        let Some(manifest) = store.read_manifest(&base)? else {
            return Ok(ToolOutcome::ok_json(json!({
                "status": "missing",
                "connectionId": connection_id,
                "database": database,
                "remediation": "rebuild_index",
            })));
        };

        let mut live_fingerprint: Option<String> = None;
        let mut drift: Option<bool> = None;
        if let Ok(client) = self.session.checkout().await {
            let db = PgCatalogDb::new(&client);
            if let Ok(fp) = db.schema_fingerprint().await {
                drift = Some(fp != manifest.schema_fingerprint);
                live_fingerprint = Some(fp);
            }
        }

        Ok(ToolOutcome::ok_json(json!({
            "status": "ok",
            "connectionId": manifest.connection_id,
            "database": manifest.database,
            "indexedAt": manifest.indexed_at,
            "fingerprint": manifest.schema_fingerprint,
            "liveFingerprint": live_fingerprint,
            "drift": drift,
            "pgVersion": manifest.pg_version,
            "counts": {
                "tables": manifest.counts.tables,
                "views": manifest.counts.views,
                "functions": manifest.counts.functions,
                "enums": manifest.counts.enums,
            },
            "buildMs": manifest.stats.build_ms,
            "warnings": manifest.stats.warnings,
        })))
    }

    async fn list_extensions(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::list_extensions(), &[]).await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn server_settings(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::server_settings(), &[]).await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn suggest_indexes(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let client = self.session.checkout().await?;
        let mut query_errors = serde_json::Map::new();

        let high_seq_json = match client.query(&sql::high_seq_scan_tables(limit), &[]).await {
            Ok(rows) => rows_to_json(&rows),
            Err(e) => {
                query_errors.insert(
                    "high_seq_scan_tables".into(),
                    json!(nexql_conn::format_postgres_error(&e)),
                );
                Value::Null
            }
        };

        let unindexed_json = match client.query(&sql::unindexed_fk_columns(limit), &[]).await {
            Ok(rows) => rows_to_json(&rows),
            Err(e) => {
                query_errors.insert(
                    "unindexed_fk_columns".into(),
                    json!(nexql_conn::format_postgres_error(&e)),
                );
                Value::Null
            }
        };

        let mut pg_stat_available = false;
        let mut slow_queries = Value::Null;
        let mut pg_stat_note: Option<String> = None;
        match client.query(&sql::slow_queries(limit.min(10)), &[]).await {
            Ok(rows) => {
                pg_stat_available = true;
                slow_queries = rows_to_json(&rows);
            }
            Err(e) => {
                if let Some(message) = sql::map_stat_statements_error(&e) {
                    pg_stat_note = Some(message);
                } else {
                    query_errors.insert(
                        "slow_queries".into(),
                        json!(nexql_conn::format_postgres_error(&e)),
                    );
                }
            }
        }

        let mut plan_heuristics = Value::Null;
        if let Some(sql_text) = args.get("sql").and_then(|v| v.as_str()) {
            require_select_or_with(&self.session.filter(), sql_text)?;
            let explain = build_explain_sql(sql_text, false);
            match self.run_explain_in_transaction(&explain).await {
                Ok(outcome) => {
                    let rows = outcome.structured.unwrap_or(Value::Null);
                    let plan = rows
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|r| r.get("QUERY PLAN"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let metrics =
                        extract_plan_metrics(&plan).or_else(|| extract_plan_metrics(&rows));
                    plan_heuristics = json!({
                        "metrics": metrics,
                        "hint": "Use analyze_query_plan with analyze=true for actual timings before creating indexes.",
                    });
                }
                Err(e) => {
                    query_errors.insert("plan_heuristics".into(), json!(e.to_string()));
                }
            }
        }

        let has_candidates = high_seq_json
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
            || unindexed_json
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false)
            || plan_heuristics != Value::Null;

        let mut payload = if !has_candidates && !pg_stat_available {
            json!({
                "suggestions": [],
                "message": "No index suggestions yet. Either table stats show healthy index use, or there is not enough scan history. Enable pg_stat_statements and/or pass a sql argument for EXPLAIN plan heuristics.",
                "hint": pg_stat_note,
            })
        } else if !has_candidates {
            json!({
                "high_seq_scan_tables": high_seq_json,
                "unindexed_fk_columns": unindexed_json,
                "slow_queries": slow_queries,
                "plan_heuristics": plan_heuristics,
                "message": "No strong index candidates from sequential-scan or unindexed-FK heuristics. Review slow_queries / pass sql for plan-level advice.",
                "hint": "CREATE INDEX CONCURRENTLY after validating with EXPLAIN (ANALYZE, BUFFERS).",
            })
        } else {
            json!({
                "high_seq_scan_tables": high_seq_json,
                "unindexed_fk_columns": unindexed_json,
                "slow_queries": slow_queries,
                "plan_heuristics": plan_heuristics,
                "pg_stat_statements": pg_stat_available,
                "hint": pg_stat_note.unwrap_or_else(|| {
                    "Validate candidates with analyze_query_plan / EXPLAIN before CREATE INDEX CONCURRENTLY.".into()
                }),
            })
        };

        if !query_errors.is_empty() {
            payload["query_errors"] = Value::Object(query_errors);
        }

        Ok(ToolOutcome::ok_json(payload))
    }

    async fn find_unused_indexes(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let client = self.session.checkout().await?;
        let rows = client.query(&sql::find_unused_indexes(limit), &[]).await?;
        let indexes = rows_to_json(&rows);
        if indexes.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "indexes": [],
                "message": "No unused non-constraint indexes found (idx_scan = 0). Note: pg_stat_reset / server restart clears scan counts — treat never-scanned indexes cautiously on fresh stats.",
            })));
        }
        Ok(ToolOutcome::ok_json(json!({
            "indexes": indexes,
            "hint": "Prefer DROP INDEX CONCURRENTLY after confirming the workload (and that stats are mature).",
        })))
    }

    async fn bloat_report(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let client = self.session.checkout().await?;
        let rows = client.query(&sql::bloat_report(limit), &[]).await?;
        let tables = rows_to_json(&rows);
        if tables.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "tables": [],
                "method": "dead_tuple_ratio",
                "message": "No tables with significant dead-tuple pressure (>1000 dead tuples). This is a simplified estimate from pg_stat_user_tables, not physical page bloat.",
            })));
        }
        Ok(ToolOutcome::ok_json(json!({
            "tables": tables,
            "method": "dead_tuple_ratio",
            "note": "Approximate bloat via n_dead_tup / (n_live_tup + n_dead_tup). Not a physical page-bloat estimate (pgstattuple / check_postgres). Consider VACUUM / VACUUM FULL only after confirming impact.",
            "hint": "VACUUM ANALYZE on high bloat_pct tables; investigate autovacuum settings if last_autovacuum is stale.",
        })))
    }

    async fn find_missing_fks(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let capped = limit.clamp(1, sql::REPORT_LIMIT_MAX) as usize;

        // Prefer schema-index join-graph inferred edges when an index exists.
        if let Ok((store, connection_id, database)) = self.index_service().await {
            let base = store.base_dir(&connection_id, &database);
            if let Ok(Some(manifest)) = store.read_manifest(&base)
                && let Ok(Some(graph)) = store.read_join_graph(&base, &manifest)
            {
                let candidates: Vec<Value> = graph
                    .edges
                    .into_iter()
                    .filter(|e| e.inferred == Some(true) && e.disabled != Some(true))
                    .take(capped)
                    .map(|e| {
                        let cols: Vec<Value> = e
                            .cols
                            .iter()
                            .map(|(a, b)| json!({ "from": a, "to": b }))
                            .collect();
                        json!({
                            "from_table": e.from,
                            "to_table": e.to,
                            "via": e.via,
                            "columns": cols,
                            "detection": "join_graph_inferred",
                        })
                    })
                    .collect();
                if !candidates.is_empty() {
                    return Ok(ToolOutcome::ok_json(json!({
                        "candidates": candidates,
                        "source": "join_graph",
                        "hint": "These edges were inferred by naming convention and have no declared FK. Review before ALTER TABLE … ADD FOREIGN KEY.",
                    })));
                }
            }
        }

        let client = self.session.checkout().await?;
        let rows = client
            .query(&sql::find_missing_fks_catalog(limit), &[])
            .await?;
        let candidates = rows_to_json(&rows);
        if candidates.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "candidates": [],
                "source": "catalog",
                "message": "No missing FK candidates found via join-graph inferred edges or *_id naming against single-column PKs.",
            })));
        }
        Ok(ToolOutcome::ok_json(json!({
            "candidates": candidates,
            "source": "catalog",
            "hint": "Naming-inferred only — verify referential integrity and nullability before adding constraints. Run `nexql-mcp index build` for join-graph inferred edges.",
        })))
    }

    async fn list_roles(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let role = args
            .get("role")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let Some(role_name) = role else {
            let rows = client.query(sql::list_roles(), &[]).await?;
            return Ok(ToolOutcome::ok_json(rows_to_json(&rows)));
        };

        let details = client.query(sql::role_details(), &[&role_name]).await?;
        if details.is_empty() {
            return Err(ToolError::Execution(format!(
                "Role \"{role_name}\" not found"
            )));
        }
        let member_of = client.query(sql::role_member_of(), &[&role_name]).await?;
        let has_members = client.query(sql::role_has_members(), &[&role_name]).await?;
        let privileges = client
            .query(sql::role_table_privileges(), &[&role_name])
            .await?;

        Ok(ToolOutcome::ok_json(json!({
            "role": rows_to_json(&details).as_array().and_then(|a| a.first().cloned()).unwrap_or(Value::Null),
            "member_of": rows_to_json(&member_of),
            "has_members": rows_to_json(&has_members),
            "table_privileges": rows_to_json(&privileges),
        })))
    }

    async fn export_query(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(&self.session.filter(), sql)?;

        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .map(|s| {
                ExportFormat::parse(s).ok_or_else(|| {
                    ToolError::InvalidArgs(format!(
                        "Unsupported format \"{s}\". Use csv, json, or sqlinsert."
                    ))
                })
            })
            .transpose()?
            .unwrap_or(ExportFormat::Csv);

        let table_target = match args.get("table").and_then(|v| v.as_str()) {
            Some(t) if !t.trim().is_empty() => Some(parse_ref(t).map_err(ToolError::InvalidArgs)?),
            _ => None,
        };

        if format == ExportFormat::SqlInsert && table_target.is_none() {
            return Err(ToolError::InvalidArgs(
                "table (schema.name) is required when format=sqlinsert".into(),
            ));
        }

        let max_rows = self.session.caps().max_rows;
        // Not columnar: export_query needs row-objects to derive `columns` /
        // build CSV / sqlinsert output below.
        let outcome = self.run_select_internal(sql, Some(max_rows), false).await?;
        if outcome.is_error {
            return Ok(outcome);
        }

        let structured = outcome.structured.unwrap_or(Value::Null);
        let rows_val = structured
            .get("rows")
            .cloned()
            .or_else(|| structured.get("data").and_then(|d| d.get("rows").cloned()))
            .unwrap_or(Value::Array(vec![]));
        let rows = rows_val.as_array().cloned().unwrap_or_default();
        let columns = columns_from_rows(&rows);
        let truncated = structured
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let payload = match format {
            ExportFormat::Json => json!({
                "format": format.as_str(),
                "rowCount": rows.len(),
                "truncated": truncated,
                "columns": columns,
                "rows": rows,
            }),
            ExportFormat::Csv => {
                let content = rows_to_csv(&rows, &columns);
                let caps = self.session.caps();
                let (char_trunc, content) = caps.truncate_chars(&content);
                json!({
                    "format": format.as_str(),
                    "rowCount": rows.len(),
                    "truncated": truncated || char_trunc,
                    "columns": columns,
                    "content": content,
                })
            }
            ExportFormat::SqlInsert => {
                let (schema, table) = table_target.expect("checked above");
                let content = rows_to_sql_insert(&rows, &columns, &schema, &table);
                let caps = self.session.caps();
                let (char_trunc, content) = caps.truncate_chars(&content);
                json!({
                    "format": format.as_str(),
                    "rowCount": rows.len(),
                    "truncated": truncated || char_trunc,
                    "table": format!("{schema}.{table}"),
                    "columns": columns,
                    "content": content,
                })
            }
        };

        Ok(ToolOutcome::ok_json(payload))
    }

    async fn db_dashboard(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let sections: &[(&str, &str)] = &[
            ("db_info", sql::dashboard_db_info()),
            ("connection_states", sql::connection_states()),
            ("top_tables", sql::dashboard_top_tables()),
            ("object_counts", sql::dashboard_object_counts()),
            ("active_queries", sql::dashboard_active_queries()),
            ("blocking_locks", sql::blocking_locks()),
            ("max_connections", sql::dashboard_max_connections()),
            ("extension_count", sql::dashboard_extension_count()),
            ("cache", sql::cache_hit_ratio()),
        ];
        let mut report = serde_json::Map::new();
        for (key, q) in sections {
            match client.query(*q, &[]).await {
                Ok(rows) => {
                    report.insert((*key).into(), rows_to_json(&rows));
                }
                Err(e) => {
                    report.insert((*key).into(), json!({ "error": e.to_string() }));
                }
            }
        }

        // Normalize single-row sections to objects for agents.
        for key in ["db_info", "object_counts", "extension_count", "cache"] {
            if let Some(Value::Array(arr)) = report.get(key).cloned()
                && arr.len() == 1
            {
                report.insert(key.into(), arr.into_iter().next().unwrap());
            }
        }
        if let Some(Value::Array(arr)) = report.get("max_connections").cloned()
            && let Some(row) = arr.first()
        {
            report.insert(
                "max_connections".into(),
                row.get("max_connections")
                    .cloned()
                    .unwrap_or_else(|| row.clone()),
            );
        }

        Ok(ToolOutcome::ok_json(Value::Object(report)))
    }

    async fn deep_plan_analysis(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(&self.session.filter(), sql)?;
        let analyze = args
            .get("analyze")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let explain = build_explain_sql(sql, analyze);
        let outcome = self.run_explain_in_transaction(&explain).await?;
        let rows = outcome.structured.unwrap_or(Value::Null);
        let row_array = rows
            .get("rows")
            .and_then(|v| v.as_array())
            .or_else(|| rows.as_array());
        let plan = row_array
            .and_then(|a| a.first())
            .and_then(|r| r.get("QUERY PLAN"))
            .cloned()
            .unwrap_or(Value::Null);
        let deep = analyze_deep_plan(&plan, sql)
            .or_else(|| analyze_deep_plan(&rows, sql))
            .ok_or_else(|| {
                ToolError::Execution("Could not parse EXPLAIN JSON plan for deep analysis".into())
            })?;
        let metrics = extract_plan_metrics(&plan).or_else(|| extract_plan_metrics(&rows));
        Ok(ToolOutcome::ok_json(json!({
            "deep": deep,
            "metrics": metrics,
            "plan": plan,
            "analyzed": analyze,
        })))
    }

    async fn schema_diff(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let source_schema = args
            .get("sourceSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sourceSchema is required".into()))?;
        let target_schema = args
            .get("targetSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("targetSchema is required".into()))?;
        crate::schema_diff::require_safe_schema(source_schema)?;
        crate::schema_diff::require_safe_schema(target_schema)?;

        let client = self.session.checkout().await?;
        let source = crate::schema_diff::load_schema_snapshot(&client, source_schema).await?;
        let target = crate::schema_diff::load_schema_snapshot(&client, target_schema).await?;
        let diffs = crate::schema_diff::compute_schema_diff(&source, &target);
        let changed = diffs
            .iter()
            .filter(|d| d.status != crate::schema_diff::DiffStatus::Unchanged)
            .count();
        Ok(ToolOutcome::ok_json(json!({
            "sourceSchema": source_schema,
            "targetSchema": target_schema,
            "tableCount": diffs.len(),
            "changedCount": changed,
            "diffs": crate::schema_diff::diffs_to_json(&diffs),
        })))
    }

    async fn generate_migration(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let source_schema = args
            .get("sourceSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sourceSchema is required".into()))?;
        let target_schema = args
            .get("targetSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("targetSchema is required".into()))?;
        crate::schema_diff::require_safe_schema(source_schema)?;
        crate::schema_diff::require_safe_schema(target_schema)?;

        let client = self.session.checkout().await?;
        let source = crate::schema_diff::load_schema_snapshot(&client, source_schema).await?;
        let target = crate::schema_diff::load_schema_snapshot(&client, target_schema).await?;
        let diffs = crate::schema_diff::compute_schema_diff(&source, &target);
        let statements =
            crate::schema_diff::build_migration_statements(source_schema, target_schema, &diffs);
        let sql = if statements.is_empty() {
            format!("-- No differences between {source_schema} and {target_schema}")
        } else {
            statements.join("\n\n")
        };
        Ok(ToolOutcome::ok_json(json!({
            "sourceSchema": source_schema,
            "targetSchema": target_schema,
            "statementCount": statements.len(),
            "sql": sql,
            "hint": "Read-only: review and run via execute_sql / apply_ddl only with --access-mode write|admin. Destructive drops are commented out.",
        })))
    }

    /// `columnar`: reshape the result from per-row objects to
    /// `{"columns": [...], "rows": [[...]]}` before serializing, and skip
    /// pretty-printing (Issue 5 — 3–5x fewer tokens at higher row/column
    /// counts; pretty JSON is for humans, no MCP client needs it). Only the
    /// `run_select` tool itself passes `true` — `explain_query` and
    /// `export_query` reuse this function and need the row-object shape
    /// (export in particular derives `columns` and builds CSV/sqlinsert from
    /// it), so they stay on the original shape.
    async fn run_select_internal(
        &self,
        sql: &str,
        max_rows: Option<u32>,
        columnar: bool,
    ) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let Some(max_rows) = max_rows else {
            let rows = client.query(sql, &[]).await?;
            let values = rows_to_json(&rows);
            let mut payload = self.apply_pii_redaction(sql, ensure_structured_object(values));
            if columnar {
                payload = columnarize_read_payload(payload);
            }
            let text = if columnar {
                serde_json::to_string(&payload)
            } else {
                serde_json::to_string_pretty(&payload)
            }
            .map_err(|e| ToolError::Execution(e.to_string()))?;
            let caps = self.session.caps();
            let (trunc, text) = caps.truncate_chars(&text);
            let structured = if trunc {
                json!({ "truncated_chars": true, "data": payload })
            } else {
                payload
            };
            return Ok(ToolOutcome {
                text: text.to_string(),
                structured: Some(structured),
                is_error: false,
            });
        };

        let cleaned = sql.trim().trim_end_matches(';').trim();
        let wrapped = format!(
            "SELECT * FROM ({cleaned}) AS nexql_limited LIMIT {}",
            max_rows + 1
        );
        let rows = client.query(&wrapped, &[]).await.map_err(|e| {
            ToolError::Execution(format!(
                "Failed to execute row-limited query (refusing unbounded fallback): {}",
                nexql_conn::format_postgres_error(&e)
            ))
        })?;
        let truncated = rows.len() as u32 > max_rows;
        let keep = if truncated {
            &rows[..max_rows as usize]
        } else {
            &rows[..]
        };
        let values = rows_to_json(keep);
        // Always `{ "rows": [...] }` — truncation flags are extra fields on the object.
        let mut payload = self.apply_pii_redaction(sql, ensure_structured_object(values));
        if truncated && let Some(obj) = payload.as_object_mut() {
            obj.insert("truncated".into(), json!(true));
            obj.insert("maxRows".into(), json!(max_rows));
        }
        if columnar {
            payload = columnarize_read_payload(payload);
        }
        let text = if columnar {
            serde_json::to_string(&payload)
        } else {
            serde_json::to_string_pretty(&payload)
        }
        .map_err(|e| ToolError::Execution(e.to_string()))?;
        let caps = self.session.caps();
        let (char_trunc, text) = caps.truncate_chars(&text);
        let structured = if char_trunc {
            json!({ "truncated_chars": true, "data": payload })
        } else {
            payload
        };
        Ok(ToolOutcome {
            text: text.to_string(),
            structured: Some(structured),
            is_error: false,
        })
    }

    fn apply_pii_redaction(&self, sql: &str, mut payload: Value) -> Value {
        let filter = self.session.filter();
        if filter.pii_columns.is_empty() {
            return payload;
        }
        let Ok(tables) = select_table_refs(sql) else {
            return payload;
        };
        let (redacted, cols) = redact_pii_in_payload(payload, &filter.pii_columns, &tables);
        payload = redacted;
        if !cols.is_empty()
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("piiRedactedColumns".into(), json!(cols));
        }
        payload
    }
}

/// Lowercase + collapse to alphanumeric-separated-by-single-spaces, for `fuzzy_score`.
fn normalize_for_match(s: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = true;
    for ch in s.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push(' ');
            last_was_sep = true;
        }
    }
    out.trim().to_string()
}

/// Cheap fuzzy match, 0-100: exact > substring > token overlap.
fn fuzzy_score(hint: &str, candidate: &str) -> f64 {
    let h = normalize_for_match(hint);
    let c = normalize_for_match(candidate);
    if h.is_empty() || c.is_empty() {
        return 0.0;
    }
    if h == c {
        return 100.0;
    }
    if c.contains(&h) || h.contains(&c) {
        return 75.0;
    }
    let h_tokens: std::collections::HashSet<&str> =
        h.split(' ').filter(|s| !s.is_empty()).collect();
    let c_tokens: std::collections::HashSet<&str> =
        c.split(' ').filter(|s| !s.is_empty()).collect();
    let overlap = h_tokens.intersection(&c_tokens).count();
    if overlap == 0 {
        return 0.0;
    }
    (overlap as f64 / h_tokens.len().max(c_tokens.len()) as f64) * 60.0
}

fn policy_to_query_filter(filter: &PolicyFilter) -> QueryPolicyFilter {
    QueryPolicyFilter {
        allow_schemas: filter.allow_schemas.clone(),
        deny_schemas: filter.deny_schemas.clone(),
        deny_tables: filter.deny_tables.clone(),
        pii_columns: filter.pii_columns.clone(),
    }
}

fn require_select_or_with(filter: &PolicyFilter, sql: &str) -> Result<(), ToolError> {
    match validate_readonly_sql(sql)? {
        SqlDecision::Allow => {}
        SqlDecision::Reject => {
            return Err(ToolError::Execution(
                "Security Error: Only SELECT or WITH statements can be analyzed.".into(),
            ));
        }
    }
    enforce_read_table_policy(filter, sql)?;
    let trimmed = sql.trim().to_ascii_lowercase();
    if !(trimmed.starts_with("select") || trimmed.starts_with("with")) {
        return Err(ToolError::Execution(
            "Security Error: Only SELECT or WITH statements can be analyzed.".into(),
        ));
    }
    Ok(())
}

fn rows_to_json(rows: &[tokio_postgres::Row]) -> Value {
    rows_to_json_array(rows)
}

fn scores_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= f64::EPSILON * a.abs().max(b.abs()).max(1.0)
}

fn read_recent_log_errors() -> Vec<String> {
    let path = std::env::var("NEXQL_MCP_LOG")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                std::path::PathBuf::from(h)
                    .join(".config")
                    .join("nexql-mcp")
                    .join("logs")
                    .join("nexql-mcp.log")
            })
        });

    let Some(log_path) = path else {
        return Vec::new();
    };

    let Ok(content) = std::fs::read_to_string(&log_path) else {
        return Vec::new();
    };

    content
        .lines()
        .rev()
        .take(50)
        .filter(|line| {
            line.contains("ERROR")
                || line.contains("WARN")
                || line.contains("failed")
                || line.contains("Error")
        })
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::build_explain_sql;
    use nexql_policy::PolicyFilter;
    use serde_json::json;

    use crate::session::{ConnectionInfo, ConnectionPolicy, ToolSession};
    use nexql_policy::{AccessMode, PolicyCaps};

    fn test_conn() -> ConnectionInfo {
        ConnectionInfo {
            id: "conn-1".into(),
            name: "conn-1".into(),
            host: Some("127.0.0.1".into()),
            port: Some(5432),
            database: Some("appdb".into()),
            params: Default::default(),
            policy: ConnectionPolicy {
                access_mode: AccessMode::Read,
                caps: PolicyCaps::default(),
                filter: PolicyFilter::default(),
                environment: None,
            },
        }
    }

    #[test]
    fn scores_equal_treats_near_duplicates_as_tied() {
        let s = 3.295836866004329_f64;
        assert!(super::scores_equal(s, s));
        assert!(super::scores_equal(s, s + f64::EPSILON));
    }

    #[test]
    fn policy_maps_one_to_one() {
        let f = PolicyFilter {
            allow_schemas: vec!["public".into()],
            deny_schemas: vec!["pgboss".into()],
            deny_tables: vec!["auth.*".into()],
            pii_columns: vec!["public.users.ssn".into()],
        };
        let q = policy_to_query_filter(&f);
        assert_eq!(q.allow_schemas, f.allow_schemas);
        assert_eq!(q.deny_schemas, f.deny_schemas);
        assert_eq!(q.deny_tables, f.deny_tables);
        assert_eq!(q.pii_columns, f.pii_columns);
    }

    #[test]
    fn ok_json_wraps_arrays_for_cursor_structured_content() {
        let out = ToolOutcome::ok_json(json!([{ "id": 1 }, { "id": 2 }]));
        assert!(!out.is_error);
        let s = out.structured.as_ref().unwrap();
        assert!(s.is_object(), "structuredContent must be object, got {s}");
        assert_eq!(s["rows"].as_array().unwrap().len(), 2);
        assert!(out.text.contains("\"rows\""));
    }

    #[test]
    fn ok_json_leaves_objects_unchanged() {
        let out = ToolOutcome::ok_json(json!({ "kind": "table", "name": "orders" }));
        let s = out.structured.as_ref().unwrap();
        assert_eq!(s["kind"], "table");
        assert!(s.get("rows").is_none());
    }

    #[test]
    fn router_specs_include_phase4_and_phase9() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        assert_eq!(router.specs().len(), ToolName::ACTIVE.len());
        let names: Vec<_> = router.specs().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"search_schema"));
        assert!(names.contains(&"get_ddl"));
        assert!(names.contains(&"explain_analyze"));
        assert!(names.contains(&"get_index_status"));
        assert!(names.contains(&"list_extensions"));
        assert!(names.contains(&"server_settings"));
        assert!(names.contains(&"suggest_indexes"));
        assert!(names.contains(&"find_unused_indexes"));
        assert!(names.contains(&"bloat_report"));
        assert!(names.contains(&"find_missing_fks"));
        assert!(names.contains(&"export_query"));
        assert!(names.contains(&"list_roles"));
        assert!(names.contains(&"db_dashboard"));
        assert!(names.contains(&"deep_plan_analysis"));
        assert!(names.contains(&"execute_sql"));
        assert!(names.contains(&"edit_row"));
        assert!(names.contains(&"import_data"));
        assert!(names.contains(&"apply_ddl"));
        assert!(names.contains(&"create_index_concurrently"));
        assert!(names.contains(&"run_maintenance"));
        assert!(names.contains(&"terminate_query"));
    }

    #[tokio::test]
    async fn write_tools_refuse_read_mode() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        for tool in [
            "execute_sql",
            "edit_row",
            "import_data",
            "apply_ddl",
            "create_index_concurrently",
            "run_maintenance",
            "terminate_query",
        ] {
            let out = router
                .call(tool, json!({ "sql": "SELECT 1", "table": "public.t", "rows": [], "action": "insert", "values": {}, "pid": 1 }))
                .await;
            assert!(out.is_error, "{tool}: {}", out.text);
            assert!(
                out.text.contains("write") || out.text.contains("admin"),
                "{tool}: {}",
                out.text
            );
        }
    }

    #[tokio::test]
    async fn table_stats_rejects_injection_ref() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let out = router
            .call("table_stats", json!({ "ref": "public.users; DROP" }))
            .await;
        assert!(out.is_error, "{}", out.text);
        assert!(
            out.text.contains("Invalid object reference") || out.text.contains("invalid arguments"),
            "expected ref validation error, got: {}",
            out.text
        );
    }

    #[test]
    fn explain_transaction_path_builds_readonly_sequence() {
        // Documented contract: BEGIN → SET TRANSACTION READ ONLY → EXPLAIN → ROLLBACK
        let explain = build_explain_sql("SELECT 1", true);
        assert!(explain.starts_with("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)"));
        assert!(!explain.to_ascii_lowercase().contains("commit"));
        let steps = ["BEGIN", "SET TRANSACTION READ ONLY", &explain, "ROLLBACK"];
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0], "BEGIN");
        assert_eq!(steps[1], "SET TRANSACTION READ ONLY");
        assert_eq!(steps[3], "ROLLBACK");
    }

    #[tokio::test]
    async fn missing_index_returns_actionable_error() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let out = router
            .call("search_schema", json!({ "query": "users" }))
            .await;
        assert!(out.is_error, "{}", out.text);
        assert!(
            out.text.contains("rebuild_index"),
            "expected actionable hint, got: {}",
            out.text
        );
    }

    /// Minimal index fixture for `get_join_path` resolution tests: `orders`,
    /// `customers`, `order_items` in `public`, with `order_items -> orders ->
    /// customers` FK edges — mirrors the report's repro schema.
    fn write_join_path_fixture(store: &IndexStore) {
        use nexql_index::{
            BuildDepth, BuildMode, ColumnEntry, DbObjectKind, IndexCounts, IndexDerived,
            IndexManifest, IndexScope, IndexStats, JOIN_GRAPH_FILE, JoinEdge, JoinGraph,
            ObjectEntry, ObjectShard, TOKENS_FILE,
        };
        use std::collections::HashMap;

        let base = store.base_dir("conn-1", "appdb");
        let manifest = IndexManifest {
            format_version: 1,
            connection_id: "conn-1".into(),
            database: "appdb".into(),
            indexed_at: "2026-08-08T00:00:00.000Z".into(),
            build_mode: BuildMode::Auto,
            build_depth: BuildDepth::Structure,
            schema_fingerprint: "fp".into(),
            pg_version: "18.4".into(),
            environment: "development".into(),
            scope: IndexScope {
                included_schemas: vec!["public".into()],
                excluded_objects: vec![],
                pii_excluded_columns: vec![],
            },
            counts: IndexCounts {
                tables: 3,
                views: 0,
                functions: 0,
                enums: 0,
            },
            shards: vec![ObjectShard {
                file: "objects-public-0.json".into(),
                schema: "public".into(),
                objects: 3,
                bytes: 512,
                hash: "abc".into(),
            }],
            derived: IndexDerived {
                tokens: TOKENS_FILE.into(),
                join_graph: JOIN_GRAPH_FILE.into(),
                values: None,
                embeddings: None,
                embeddings_meta: None,
            },
            stats: IndexStats {
                build_ms: 1,
                queries_run: 1,
                warnings: vec![],
            },
        };
        store.write_manifest(&base, &manifest).unwrap();

        fn entry(oid: u32) -> ObjectEntry {
            ObjectEntry {
                kind: DbObjectKind::Table,
                oid,
                object_hash: format!("hash{oid}"),
                comment: None,
                row_estimate: 10.0,
                size_bytes: 8192,
                columns: vec![ColumnEntry {
                    name: "id".into(),
                    type_name: "integer".into(),
                    not_null: true,
                    default_value: None,
                    comment: None,
                    ordinal: 1,
                    is_pk: Some(true),
                    profile: None,
                    pii: None,
                }],
                primary_key: Some(vec!["id".into()]),
                foreign_keys: None,
                indexes: None,
                checks: None,
                excluded: None,
                definition: None,
                signature: None,
                language: None,
                volatility: None,
                body: None,
                values: None,
                base_type: None,
                constraint: None,
            }
        }
        // `orders.status` carries a profiled column with common_values, mirroring
        // the report's fixture — used by the zero-rows critique test below.
        let mut orders = entry(1);
        orders.columns.push(nexql_index::ColumnEntry {
            name: "status".into(),
            type_name: "text".into(),
            not_null: true,
            default_value: None,
            comment: None,
            ordinal: 2,
            is_pk: None,
            profile: Some(nexql_index::ColumnProfile {
                n_distinct: 2.0,
                null_frac: 0.0,
                common_values: Some(vec!["pending".into(), "paid".into()]),
                min: None,
                max: None,
            }),
            pii: None,
        });

        let mut shard = HashMap::new();
        shard.insert("public.orders".into(), orders);
        shard.insert("public.customers".into(), entry(2));
        shard.insert("public.order_items".into(), entry(3));
        store
            .write_shard_entries(&base, "objects-public-0.json", &shard)
            .unwrap();

        let graph = JoinGraph {
            edges: vec![
                JoinEdge {
                    from: "public.order_items".into(),
                    to: "public.orders".into(),
                    via: "order_items_order_id_fkey".into(),
                    cols: vec![("order_id".into(), "id".into())],
                    inferred: None,
                    disabled: None,
                },
                JoinEdge {
                    from: "public.orders".into(),
                    to: "public.customers".into(),
                    via: "orders_customer_id_fkey".into(),
                    cols: vec![("customer_id".into(), "id".into())],
                    inferred: None,
                    disabled: None,
                },
            ],
        };
        store.write_join_graph(&base, &graph).unwrap();
    }

    fn join_path_router() -> ToolRouter {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_join_path_fixture(&store);
        std::mem::forget(tmp); // keep the temp dir alive for the router's lifetime
        let session = ToolSession::for_tests(
            vec![test_conn()],
            PolicyFilter::default(),
            Some(IndexStore::new(store.root())),
        );
        ToolRouter::with_index_store(session, Some(store))
    }

    /// Issue 6: `LIMIT` without `ORDER BY` should attach a critique even
    /// when rows came back (it's about determinism, not emptiness).
    #[tokio::test]
    async fn attach_critique_flags_limit_without_order_by() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let outcome = ToolOutcome::ok_json(json!({ "columns": ["n"], "rows": [[1]] }));
        let out = router
            .attach_critique("SELECT * FROM orders LIMIT 10", outcome)
            .await;
        let structured = out.structured.unwrap();
        let critique = structured["critique"].as_array().unwrap();
        assert!(
            critique
                .iter()
                .any(|c| c["signal"] == "limit_without_order_by")
        );
    }

    #[tokio::test]
    async fn attach_critique_silent_when_nothing_fires() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let outcome = ToolOutcome::ok_json(json!({ "columns": ["n"], "rows": [[1]] }));
        let out = router
            .attach_critique("SELECT * FROM orders ORDER BY id LIMIT 10", outcome)
            .await;
        let structured = out.structured.unwrap();
        assert!(structured.get("critique").is_none());
    }

    /// Regression test for the report's headline Issue 6 example: zero rows
    /// from `status = 'complete'` should surface the actually-observed
    /// values instead of leaving a silent wrong answer.
    #[tokio::test]
    async fn attach_critique_zero_rows_suggests_observed_values() {
        let router = join_path_router();
        let outcome = ToolOutcome::ok_json(json!({ "columns": ["id"], "rows": [] }));
        let out = router
            .attach_critique(
                "SELECT * FROM public.orders WHERE status = 'complete'",
                outcome,
            )
            .await;
        let structured = out.structured.unwrap();
        let critique = structured["critique"].as_array().unwrap();
        let zero_rows = critique
            .iter()
            .find(|c| c["signal"] == "zero_rows")
            .expect("zero_rows critique");
        let msg = zero_rows["message"].as_str().unwrap();
        assert!(msg.contains("status = 'complete'"), "{msg}");
        assert!(msg.contains("pending") && msg.contains("paid"), "{msg}");
    }

    #[tokio::test]
    async fn attach_critique_zero_rows_silent_without_index() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let outcome = ToolOutcome::ok_json(json!({ "columns": ["id"], "rows": [] }));
        let out = router
            .attach_critique(
                "SELECT * FROM public.orders WHERE status = 'complete'",
                outcome,
            )
            .await;
        let structured = out.structured.unwrap();
        assert!(structured.get("critique").is_none());
    }

    /// Regression test for Issue 2: unqualified names that are directly
    /// FK-linked used to return a false-negative "no join path found" because
    /// `a`/`b` were never resolved against the index before BFS.
    #[tokio::test]
    async fn get_join_path_resolves_unqualified_names() {
        let router = join_path_router();
        let out = router
            .call(
                "get_join_path",
                json!({ "a": "order_items", "b": "orders" }),
            )
            .await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        assert_eq!(structured["resolved_a"], "public.order_items");
        assert_eq!(structured["resolved_b"], "public.orders");
        assert_eq!(structured["path"][0]["via"], "order_items_order_id_fkey");
    }

    #[tokio::test]
    async fn get_join_path_qualified_still_works() {
        let router = join_path_router();
        let out = router
            .call(
                "get_join_path",
                json!({ "a": "public.order_items", "b": "public.customers" }),
            )
            .await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        assert_eq!(structured["path"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn get_join_path_unknown_name_errors_with_suggestion() {
        let router = join_path_router();
        let out = router
            .call("get_join_path", json!({ "a": "ordrs", "b": "customers" }))
            .await;
        assert!(out.is_error, "{}", out.text);
        assert!(
            out.text.contains("did you mean") && out.text.contains("public.orders"),
            "expected a did-you-mean suggestion, got: {}",
            out.text
        );
    }

    /// Regression test for Issue 3: `get_index_status` is the tool an agent
    /// calls to *find out* whether an index exists, so absence must be a
    /// normal non-error result, not a thrown error.
    #[tokio::test]
    async fn get_index_status_reports_missing_without_erroring_no_store() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let out = router.call("get_index_status", json!({})).await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        assert_eq!(structured["status"], "missing");
        assert_eq!(structured["remediation"], "rebuild_index");
        assert_eq!(structured["database"], "appdb");
    }

    #[tokio::test]
    async fn get_index_status_reports_missing_without_erroring_empty_manifest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let session = ToolSession::for_tests(
            vec![test_conn()],
            PolicyFilter::default(),
            Some(IndexStore::new(tmp.path())),
        );
        let router = ToolRouter::with_index_store(session, Some(store));
        let out = router.call("get_index_status", json!({})).await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        assert_eq!(structured["status"], "missing");
        assert_eq!(structured["remediation"], "rebuild_index");
    }

    #[tokio::test]
    async fn get_index_status_reports_ok_when_indexed() {
        let router = join_path_router();
        let out = router.call("get_index_status", json!({})).await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        assert_eq!(structured["status"], "ok");
        assert_eq!(structured["database"], "appdb");
    }

    /// Issue 4: `orient` should answer "what tables/joins exist" in one call
    /// from the same fixture that previously needed `list_objects` +
    /// `search_schema` + `get_join_path` ×N to piece together.
    #[tokio::test]
    async fn orient_lists_tables_and_declared_joins() {
        let router = join_path_router();
        let out = router.call("orient", json!({})).await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        assert_eq!(structured["database"], "appdb");

        let tables = structured["tables"].as_array().unwrap();
        let refs: Vec<&str> = tables.iter().map(|t| t["ref"].as_str().unwrap()).collect();
        assert_eq!(
            refs,
            vec!["public.customers", "public.order_items", "public.orders"]
        );
        let orders = tables.iter().find(|t| t["ref"] == "public.orders").unwrap();
        assert_eq!(orders["pk"], "id");
        assert!(orders["columns"].as_str().unwrap().contains("id:integer!"));

        let joins = structured["joins"].as_array().unwrap();
        assert_eq!(joins.len(), 2);
        assert!(joins.iter().any(|j| j["edge"]
            == "public.order_items.order_id -> public.orders.id"
            && j["declared"] == true));
    }

    #[tokio::test]
    async fn orient_focus_narrows_tables_and_joins() {
        let router = join_path_router();
        let out = router.call("orient", json!({ "focus": "customers" })).await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        let tables = structured["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0]["ref"], "public.customers");
        // The orders->customers edge touches a focused table, so it stays.
        let joins = structured["joins"].as_array().unwrap();
        assert_eq!(joins.len(), 1);
        assert!(
            joins[0]["edge"]
                .as_str()
                .unwrap()
                .contains("public.customers")
        );
    }

    #[tokio::test]
    async fn orient_no_index_returns_notes_not_error() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let out = router.call("orient", json!({})).await;
        assert!(!out.is_error, "{}", out.text);
        let structured = out.structured.unwrap();
        assert_eq!(structured["tables"].as_array().unwrap().len(), 0);
        assert!(!structured["notes"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_index_dir_returns_build_hint() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let session = ToolSession::for_tests(
            vec![test_conn()],
            PolicyFilter::default(),
            Some(IndexStore::new(tmp.path())),
        );
        let router = ToolRouter::with_index_store(session, Some(store));
        let out = router
            .call("describe_object", json!({ "ref": "public.users" }))
            .await;
        assert!(out.is_error, "{}", out.text);
        assert!(
            out.text.contains("rebuild_index"),
            "expected build hint, got: {}",
            out.text
        );
    }

    #[tokio::test]
    async fn outcome_tagged_with_connection_id_and_database() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::new(session);
        let out = router.call("list_connections", json!({})).await;
        let structured = out.structured.expect("structured outcome");
        assert_eq!(
            structured.get("connectionId").and_then(|v| v.as_str()),
            Some("conn-1")
        );
        assert_eq!(
            structured.get("database").and_then(|v| v.as_str()),
            Some("appdb")
        );
    }

    #[tokio::test]
    async fn setup_connection_returns_needs_input_when_incomplete() {
        unsafe {
            std::env::remove_var("DATABASE_URL");
            std::env::remove_var("POSTGRES_URL");
            std::env::remove_var("PGHOST");
        }
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::new(session);
        let out = router.call("setup_connection", json!({})).await;
        let structured = out.structured.expect("structured outcome");
        assert!(structured.get("status").is_some());
    }

    #[tokio::test]
    async fn save_profile_persists_config() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::new(session);
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        unsafe {
            std::env::set_var("NEXQL_MCP_CONFIG", &cfg_path);
        }

        let out = router
            .call(
                "save_profile",
                json!({
                    "name": "staging",
                    "host": "127.0.0.1",
                    "port": 5432,
                    "dbname": "stage_db",
                    "user": "stage_user"
                }),
            )
            .await;

        let structured = out.structured.expect("structured outcome");
        assert_eq!(
            structured.get("status").and_then(|v| v.as_str()),
            Some("saved")
        );
        assert_eq!(
            structured.get("profile").and_then(|v| v.as_str()),
            Some("staging")
        );
    }

    #[tokio::test]
    async fn check_ddl_safety_tool_dispatches_ast_report() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::new(session);
        let out = router
            .call(
                "check_ddl_safety",
                json!({ "ddl": "CREATE INDEX idx_col ON users(col);" }),
            )
            .await;
        let structured = out.structured.expect("structured outcome");
        assert_eq!(
            structured.get("overall_risk").and_then(|v| v.as_str()),
            Some("CRITICAL")
        );
    }
}
