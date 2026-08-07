// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! DDL migration safety inspection rules for locking risk assessment.

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Critical,
    High,
    Medium,
    Safe,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "CRITICAL",
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Safe => "SAFE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DdlSafetyIssue {
    pub risk_level: RiskLevel,
    pub lock_type: &'static str,
    pub issue: String,
    pub recommendation: String,
    pub safe_alternative_sql: Option<String>,
}

use pg_query::{NodeRef, parse};

pub fn analyze_ddl_safety(ddl: &str) -> Value {
    let parse_result = match parse(ddl) {
        Ok(res) => res,
        Err(e) => {
            return json!({
                "overall_risk": "CRITICAL",
                "statement_count": 0,
                "issue_count": 1,
                "issues": [{
                    "risk_level": "CRITICAL",
                    "lock_type": "Parse Error",
                    "issue": format!("Failed to parse SQL statement(s): {e}"),
                    "recommendation": "Check SQL syntax.",
                    "safe_alternative_sql": Value::Null,
                }],
                "is_safe": false,
            });
        }
    };

    let mut issues = Vec::new();
    let mut overall_risk = RiskLevel::Safe;
    let statement_count = parse_result.protobuf.stmts.len();

    for (node_ref, _depth, _context, _loc) in parse_result.protobuf.nodes() {
        match node_ref {
            NodeRef::IndexStmt(idx) => {
                if !idx.concurrent {
                    let safe_sql = ddl.replacen("CREATE INDEX", "CREATE INDEX CONCURRENTLY", 1);
                    let issue = DdlSafetyIssue {
                        risk_level: RiskLevel::Critical,
                        lock_type: "ShareLock / AccessExclusiveLock",
                        issue: "Building index without CONCURRENTLY blocks concurrent write operations on the table.".into(),
                        recommendation: "Use CREATE INDEX CONCURRENTLY to build indexes without blocking writes.".into(),
                        safe_alternative_sql: Some(safe_sql),
                    };
                    update_overall_risk(&mut overall_risk, RiskLevel::Critical);
                    issues.push(issue);
                }
            }
            NodeRef::DropStmt(_drop) => {
                let issue = DdlSafetyIssue {
                    risk_level: RiskLevel::Critical,
                    lock_type: "AccessExclusiveLock (Irreversible Data Loss)",
                    issue: "Dropping objects permanently removes data/schema and acquires AccessExclusiveLock.".into(),
                    recommendation: "Verify backup and confirm object is no longer in active use.".into(),
                    safe_alternative_sql: None,
                };
                update_overall_risk(&mut overall_risk, RiskLevel::Critical);
                issues.push(issue);
            }
            NodeRef::TruncateStmt(_) => {
                let issue = DdlSafetyIssue {
                    risk_level: RiskLevel::Critical,
                    lock_type: "AccessExclusiveLock (Irreversible Data Loss)",
                    issue: "Truncating tables permanently removes data and acquires AccessExclusiveLock.".into(),
                    recommendation: "Verify backup and confirm table is no longer in active use.".into(),
                    safe_alternative_sql: None,
                };
                update_overall_risk(&mut overall_risk, RiskLevel::Critical);
                issues.push(issue);
            }
            NodeRef::AlterTableCmd(cmd) => {
                use pg_query::protobuf::AlterTableType;
                if let Ok(subtype) = AlterTableType::try_from(cmd.subtype) {
                    match subtype {
                        AlterTableType::AtDropColumn => {
                            let issue = DdlSafetyIssue {
                                risk_level: RiskLevel::High,
                                lock_type: "AccessExclusiveLock",
                                issue: "Dropping a column acquires an AccessExclusiveLock, blocking all reads and writes.".into(),
                                recommendation: "Ensure application code has stopped referencing the column before dropping.".into(),
                                safe_alternative_sql: None,
                            };
                            update_overall_risk(&mut overall_risk, RiskLevel::High);
                            issues.push(issue);
                        }
                        AlterTableType::AtAlterColumnType => {
                            let issue = DdlSafetyIssue {
                                risk_level: RiskLevel::Critical,
                                lock_type: "AccessExclusiveLock (Full Table Rewrite)",
                                issue: "Altering a column type forces a full table rewrite while holding an AccessExclusiveLock.".into(),
                                recommendation: "Add a new column, backfill data asynchronously, dual-write in app logic, then drop the old column.".into(),
                                safe_alternative_sql: None,
                            };
                            update_overall_risk(&mut overall_risk, RiskLevel::Critical);
                            issues.push(issue);
                        }
                        AlterTableType::AtAddConstraint => {
                            if let Some(def_node) = &cmd.def {
                                if let Some(pg_query::protobuf::node::Node::Constraint(c)) =
                                    &def_node.node
                                {
                                    use pg_query::protobuf::ConstrType;
                                    if let Ok(ConstrType::ConstrForeign) =
                                        ConstrType::try_from(c.contype)
                                    {
                                        if !c.skip_validation {
                                            let safe_sql =
                                                format!("{} NOT VALID;", ddl.trim_end_matches(';'));
                                            let issue = DdlSafetyIssue {
                                                risk_level: RiskLevel::High,
                                                lock_type: "AccessExclusiveLock",
                                                issue: "Adding a foreign key constraint scans the entire table under AccessExclusiveLock.".into(),
                                                recommendation: "Add the constraint with NOT VALID first, then run ALTER TABLE ... VALIDATE CONSTRAINT separately.".into(),
                                                safe_alternative_sql: Some(safe_sql),
                                            };
                                            update_overall_risk(&mut overall_risk, RiskLevel::High);
                                            issues.push(issue);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    let issues_json: Vec<Value> = issues
        .into_iter()
        .map(|i| {
            json!({
                "risk_level": i.risk_level.as_str(),
                "lock_type": i.lock_type,
                "issue": i.issue,
                "recommendation": i.recommendation,
                "safe_alternative_sql": i.safe_alternative_sql,
            })
        })
        .collect();

    json!({
        "overall_risk": overall_risk.as_str(),
        "statement_count": statement_count,
        "issue_count": issues_json.len(),
        "issues": issues_json,
        "is_safe": overall_risk == RiskLevel::Safe,
    })
}

fn update_overall_risk(current: &mut RiskLevel, new_risk: RiskLevel) {
    match (*current, new_risk) {
        (RiskLevel::Critical, _) => {}
        (_, RiskLevel::Critical) => *current = RiskLevel::Critical,
        (RiskLevel::High, _) => {}
        (_, RiskLevel::High) => *current = RiskLevel::High,
        (RiskLevel::Medium, _) => {}
        (_, RiskLevel::Medium) => *current = RiskLevel::Medium,
        (RiskLevel::Safe, RiskLevel::Safe) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_concurrent_index_flagged() {
        let sql = "CREATE INDEX idx_users_email ON users(email);";
        let res = analyze_ddl_safety(sql);
        assert_eq!(res["overall_risk"], "CRITICAL");
        assert_eq!(res["issue_count"], 1);
        assert!(
            res["issues"][0]["safe_alternative_sql"]
                .as_str()
                .unwrap()
                .contains("CONCURRENTLY")
        );
    }

    #[test]
    fn test_concurrent_index_is_safe() {
        let sql = "CREATE INDEX CONCURRENTLY idx_users_email ON users(email);";
        let res = analyze_ddl_safety(sql);
        assert_eq!(res["overall_risk"], "SAFE");
        assert_eq!(res["issue_count"], 0);
    }

    #[test]
    fn test_keyword_in_string_literal_is_safe() {
        let sql = "SELECT 'CREATE INDEX idx_test ON test(col);' AS query;";
        let res = analyze_ddl_safety(sql);
        assert_eq!(res["overall_risk"], "SAFE");
        assert_eq!(res["issue_count"], 0);
    }

    #[test]
    fn test_drop_table_flagged() {
        let sql = "DROP TABLE users;";
        let res = analyze_ddl_safety(sql);
        assert_eq!(res["overall_risk"], "CRITICAL");
        assert_eq!(res["issue_count"], 1);
    }
}
