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

pub fn analyze_ddl_safety(ddl: &str) -> Value {
    let statements: Vec<&str> = ddl
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut issues = Vec::new();
    let mut overall_risk = RiskLevel::Safe;

    for stmt in &statements {
        let stmt_upper = stmt.to_uppercase();

        // 1. CREATE INDEX without CONCURRENTLY
        if stmt_upper.contains("CREATE INDEX") && !stmt_upper.contains("CONCURRENTLY") {
            let safe_sql = stmt.replacen("CREATE INDEX", "CREATE INDEX CONCURRENTLY", 1);
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

        // 2. DROP COLUMN
        if stmt_upper.contains("DROP COLUMN")
            || (stmt_upper.contains("DROP ") && stmt_upper.contains("COLUMN"))
        {
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

        // 3. ALTER COLUMN TYPE
        if stmt_upper.contains("ALTER COLUMN") && stmt_upper.contains("TYPE") {
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

        // 4. DROP TABLE or TRUNCATE
        if stmt_upper.contains("DROP TABLE") || stmt_upper.contains("TRUNCATE") {
            let issue = DdlSafetyIssue {
                risk_level: RiskLevel::Critical,
                lock_type: "AccessExclusiveLock (Irreversible Data Loss)",
                issue: "Dropping or truncating tables permanently removes data and acquires AccessExclusiveLock.".into(),
                recommendation: "Verify backup and confirm table is no longer in active use.".into(),
                safe_alternative_sql: None,
            };
            update_overall_risk(&mut overall_risk, RiskLevel::Critical);
            issues.push(issue);
        }

        // 5. ADD CONSTRAINT without NOT VALID
        if stmt_upper.contains("ADD CONSTRAINT")
            && stmt_upper.contains("FOREIGN KEY")
            && !stmt_upper.contains("NOT VALID")
        {
            let safe_sql = format!("{stmt} NOT VALID;");
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
        "statement_count": statements.len(),
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
}
