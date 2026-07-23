//! EXPLAIN JSON plan metrics — port of `QueryPerformanceAnalyzer.extractPlanMetrics`.

use serde_json::{Value, json};

/// Extract plan metrics + recommendations from EXPLAIN (FORMAT JSON) output.
pub fn extract_plan_metrics(explain_plan: &Value) -> Option<Value> {
    let plan_root = resolve_plan_root(explain_plan)?;
    let plan_node = plan_root.get("Plan")?;

    let mut sequential_scans = 0u64;
    let mut index_scans = 0u64;
    let mut lossy_bitmap_scans = 0u64;
    let mut spilled_to_disk = 0u64;
    let mut estimate_mismatches_over_10x = 0u64;
    let mut function_scans = 0u64;
    let mut cte_scans = 0u64;
    let mut subquery_scans = 0u64;
    let mut bottlenecks: Vec<String> = Vec::new();

    analyze_plan_node(
        plan_node,
        &mut sequential_scans,
        &mut index_scans,
        &mut lossy_bitmap_scans,
        &mut spilled_to_disk,
        &mut estimate_mismatches_over_10x,
        &mut function_scans,
        &mut cte_scans,
        &mut subquery_scans,
        &mut bottlenecks,
    );

    let total_cost = plan_node
        .get("Total Cost")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let planning_time = plan_root
        .get("Planning Time")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let execution_time = plan_root
        .get("Execution Time")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let buffer_stats = plan_root.get("Buffers").map(|buffers| {
        let hits = buffers
            .get("Shared Hit Blocks")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let reads = buffers
            .get("Shared Read Blocks")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let total = hits + reads;
        let hit_ratio = if total > 0 {
            Some(((total - reads) as f64 / total as f64) * 100.0)
        } else {
            None
        };
        json!({
            "bufferHits": hits,
            "bufferReads": reads,
            "hitRatio": hit_ratio,
        })
    });

    let mut recommendations = Vec::new();
    if sequential_scans > 0 && index_scans == 0 {
        recommendations.push(
            "Consider adding indexes on frequently filtered columns".to_owned(),
        );
    }
    if total_cost > 10_000.0 {
        recommendations.push(
            "Query planning cost is high; consider simplifying the query or analyzing table statistics"
                .to_owned(),
        );
    }
    if let Some(ref bs) = buffer_stats {
        if let Some(ratio) = bs.get("hitRatio").and_then(|v| v.as_f64()) {
            if ratio < 80.0 {
                recommendations.push(
                    "Low buffer hit ratio; consider increasing work_mem or improving indexes"
                        .to_owned(),
                );
            }
        }
    }
    if let Some(first) = bottlenecks.first() {
        recommendations.push(format!("Review bottlenecks: {first}"));
    }
    if estimate_mismatches_over_10x > 0 {
        recommendations.push(
            "Severe row estimate mismatch (>10x) detected. Run ANALYZE and review join/filter selectivity."
                .to_owned(),
        );
    }
    if lossy_bitmap_scans > 0 {
        recommendations.push(
            "Lossy bitmap heap scan detected. Consider more selective indexes or reducing bitmap recheck cost."
                .to_owned(),
        );
    }
    if spilled_to_disk > 0 {
        recommendations.push(
            "Plan node spilled to disk. Consider increasing work_mem for sorts/hashes."
                .to_owned(),
        );
    }

    Some(json!({
        "totalCost": total_cost,
        "planningTime": planning_time,
        "executionTime": execution_time,
        "sequentialScans": sequential_scans,
        "indexScans": index_scans,
        "bufferStats": buffer_stats,
        "bottlenecks": bottlenecks,
        "recommendations": recommendations,
        "lossyBitmapScans": lossy_bitmap_scans,
        "spilledToDisk": spilled_to_disk,
        "estimateMismatchesOver10x": estimate_mismatches_over_10x,
        "functionScans": function_scans,
        "cteScans": cte_scans,
        "subqueryScans": subquery_scans,
    }))
}

fn resolve_plan_root(explain_plan: &Value) -> Option<&Value> {
    if explain_plan.get("Plan").is_some() {
        return Some(explain_plan);
    }
    if let Some(arr) = explain_plan.as_array() {
        return arr.first().and_then(|v| {
            if v.get("Plan").is_some() {
                Some(v)
            } else {
                None
            }
        });
    }
    // EXPLAIN rows: [{ "QUERY PLAN": [ { Plan: ... } ] }] or [{ "QUERY PLAN": { Plan } }]
    if let Some(qp) = explain_plan
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r.get("QUERY PLAN"))
    {
        if qp.get("Plan").is_some() {
            return Some(qp);
        }
        if let Some(inner) = qp.as_array().and_then(|a| a.first()) {
            if inner.get("Plan").is_some() {
                return Some(inner);
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn analyze_plan_node(
    node: &Value,
    sequential_scans: &mut u64,
    index_scans: &mut u64,
    lossy_bitmap_scans: &mut u64,
    spilled_to_disk: &mut u64,
    estimate_mismatches_over_10x: &mut u64,
    function_scans: &mut u64,
    cte_scans: &mut u64,
    subquery_scans: &mut u64,
    bottlenecks: &mut Vec<String>,
) {
    let node_type = node
        .get("Node Type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let actual_rows = node.get("Actual Rows").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let plan_rows = node.get("Plan Rows").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let actual_time = node
        .get("Actual Total Time")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    if node_type.contains("Seq Scan") {
        *sequential_scans += 1;
    } else if node_type.contains("Index Scan") {
        *index_scans += 1;
    }
    if node_type.contains("Function Scan") {
        *function_scans += 1;
        let fname = node
            .get("Function Name")
            .and_then(|v| v.as_str())
            .map(|s| format!(" {s}"))
            .unwrap_or_default();
        bottlenecks.push(format!("Function scan{fname} observed in plan"));
    }
    if node_type.contains("CTE Scan") {
        *cte_scans += 1;
        let cte = node
            .get("CTE Name")
            .and_then(|v| v.as_str())
            .map(|s| format!(" {s}"))
            .unwrap_or_default();
        bottlenecks.push(format!("CTE scan{cte} observed in plan"));
    }
    if node_type.contains("Subquery Scan")
        || node_type.contains("SubPlan")
        || node_type.contains("InitPlan")
    {
        *subquery_scans += 1;
        bottlenecks.push(format!("{node_type} observed in plan"));
    }

    if plan_rows > 0.0 && actual_rows > 0.0 {
        let variance = (actual_rows - plan_rows).abs() / plan_rows;
        if variance > 0.5 {
            bottlenecks.push(format!(
                "Row estimation mismatch in {node_type}: planned {plan_rows}, actual {actual_rows}"
            ));
        }
        let ratio = (actual_rows / plan_rows.max(1.0)).max(plan_rows / actual_rows.max(1.0));
        if ratio > 10.0 {
            *estimate_mismatches_over_10x += 1;
        }
    }

    if node_type.contains("Bitmap Heap Scan") {
        if let Some(lossy) = node.get("Lossy Heap Blocks").and_then(|v| v.as_f64()) {
            if lossy > 0.0 {
                *lossy_bitmap_scans += 1;
                bottlenecks.push(format!(
                    "Lossy bitmap heap scan detected ({lossy} lossy blocks)"
                ));
            }
        }
    }
    let temp_written = node
        .get("Temp Written Blocks")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if temp_written > 0.0 {
        *spilled_to_disk += 1;
        bottlenecks.push(format!(
            "{node_type} spilled to disk ({temp_written} temp blocks written)"
        ));
    }
    if actual_time > 1000.0 {
        bottlenecks.push(format!("{node_type} took {actual_time:.2}ms"));
    }

    if let Some(plans) = node.get("Plans").and_then(|v| v.as_array()) {
        for child in plans {
            analyze_plan_node(
                child,
                sequential_scans,
                index_scans,
                lossy_bitmap_scans,
                spilled_to_disk,
                estimate_mismatches_over_10x,
                function_scans,
                cte_scans,
                subquery_scans,
                bottlenecks,
            );
        }
    }
}

/// Build EXPLAIN SQL for analyze tools (unit-tested).
pub fn build_explain_sql(sql: &str, analyze: bool) -> String {
    let options = if analyze {
        "ANALYZE, BUFFERS, FORMAT JSON"
    } else {
        "FORMAT JSON"
    };
    format!("EXPLAIN ({options}) {sql}")
}

const CRITICAL_PERCENT: f64 = 40.0;
const HIGH_PERCENT: f64 = 25.0;
const MEDIUM_PERCENT: f64 = 15.0;
const SKEW_SEVERE_RATIO: f64 = 10.0;
const SKEW_HIGH_RATIO: f64 = 4.0;
const SKEW_MEDIUM_RATIO: f64 = 2.0;
const EXPENSIVE_NODE_TIME_MS: f64 = 1000.0;

/// Severity-graded deep plan analysis (ported from pro `deepPlanAnalysis.ts`).
pub fn analyze_deep_plan(explain_plan: &Value, query: &str) -> Option<Value> {
    let plan_root = resolve_plan_root(explain_plan)?;
    let plan_node = plan_root.get("Plan")?;

    let total_cost = plan_node
        .get("Total Cost")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .max(1.0);
    let total_execution_time = plan_node
        .get("Actual Total Time")
        .and_then(|v| v.as_f64())
        .or_else(|| plan_root.get("Execution Time").and_then(|v| v.as_f64()))
        .unwrap_or(0.0)
        .max(1.0);

    let mut functions = Vec::new();
    let mut ctes: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    let mut subqueries = Vec::new();
    let mut estimate_skew = Vec::new();

    walk_deep_plan(
        plan_node,
        "root",
        total_cost,
        total_execution_time,
        &mut functions,
        &mut ctes,
        &mut subqueries,
        &mut estimate_skew,
    );

    let mut cte_list: Vec<Value> = ctes.into_values().collect();
    cte_list.sort_by(|a, b| {
        f64_desc(
            a.get("cumulativeCost").and_then(|v| v.as_f64()).unwrap_or(0.0),
            b.get("cumulativeCost").and_then(|v| v.as_f64()).unwrap_or(0.0),
        )
    });
    functions.sort_by(|a, b| {
        f64_desc(
            a.get("cumulativeCost").and_then(|v| v.as_f64()).unwrap_or(0.0),
            b.get("cumulativeCost").and_then(|v| v.as_f64()).unwrap_or(0.0),
        )
    });
    subqueries.sort_by(|a, b| {
        f64_desc(
            a.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0),
            b.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0),
        )
    });
    estimate_skew.sort_by(|a, b| {
        f64_desc(
            a.get("skewRatio").and_then(|v| v.as_f64()).unwrap_or(0.0),
            b.get("skewRatio").and_then(|v| v.as_f64()).unwrap_or(0.0),
        )
    });

    let sql_shape = extract_sql_shape(query);
    let recommendations =
        build_deep_recommendations(&functions, &cte_list, &subqueries, &estimate_skew);

    Some(json!({
        "sqlShape": sql_shape,
        "functions": functions,
        "ctes": cte_list,
        "subqueries": subqueries,
        "estimateSkew": estimate_skew,
        "recommendations": recommendations,
    }))
}

fn f64_desc(a: f64, b: f64) -> std::cmp::Ordering {
    b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
}

fn severity_from_percent(percent: f64) -> &'static str {
    if percent >= CRITICAL_PERCENT {
        "critical"
    } else if percent >= HIGH_PERCENT {
        "high"
    } else if percent >= MEDIUM_PERCENT {
        "medium"
    } else {
        "low"
    }
}

fn severity_from_skew(skew_ratio: f64) -> &'static str {
    if skew_ratio >= SKEW_SEVERE_RATIO {
        "critical"
    } else if skew_ratio >= SKEW_HIGH_RATIO {
        "high"
    } else if skew_ratio >= SKEW_MEDIUM_RATIO {
        "medium"
    } else {
        "low"
    }
}

fn to_percent(part: f64, total: f64) -> f64 {
    if total <= 0.0 {
        0.0
    } else {
        (part / total) * 100.0
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_cont(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// Best-effort CTE / set-returning-function name scrape (no regex dep).
fn extract_sql_shape(query: &str) -> Value {
    let lower = query.to_ascii_lowercase();
    let mut cte_names = Vec::new();
    if let Some(with_pos) = lower.find("with") {
        if let Some(select_rel) = lower[with_pos..].find("select") {
            let body = &query[with_pos + 4..with_pos + select_rel];
            let body_lower = body.to_ascii_lowercase();
            let mut search_from = 0;
            while let Some(as_rel) = body_lower[search_from..].find(" as ") {
                let as_abs = search_from + as_rel;
                let before = body[..as_abs].trim_end();
                if let Some(name) = before
                    .rsplit(|c: char| !(is_ident_cont(c)))
                    .next()
                    .filter(|s| !s.is_empty() && is_ident_start(s.chars().next().unwrap()))
                {
                    cte_names.push(name.to_string());
                }
                search_from = as_abs + 4;
            }
        }
    }
    cte_names.sort();
    cte_names.dedup();

    let mut from_function_names = Vec::new();
    for keyword in ["from ", "join "] {
        let mut search_from = 0;
        while let Some(rel) = lower[search_from..].find(keyword) {
            let start = search_from + rel + keyword.len();
            let rest = &query[start..];
            let rest_trim = rest.trim_start();
            let skipped = rest.len() - rest_trim.len();
            let mut end = 0;
            let chars: Vec<char> = rest_trim.chars().collect();
            if chars.first().copied().is_some_and(is_ident_start) {
                end = 1;
                while end < chars.len()
                    && (is_ident_cont(chars[end])
                        || (chars[end] == '.'
                            && end + 1 < chars.len()
                            && is_ident_start(chars[end + 1])))
                {
                    end += 1;
                }
                let after = chars.get(end..).and_then(|c| {
                    let s: String = c.iter().collect();
                    Some(s)
                });
                if after
                    .as_deref()
                    .map(|s| s.trim_start().starts_with('('))
                    .unwrap_or(false)
                {
                    let name: String = chars[..end].iter().collect();
                    from_function_names.push(name);
                }
            }
            search_from = start + skipped + end.max(1);
        }
    }
    from_function_names.sort();
    from_function_names.dedup();

    json!({
        "cteNames": cte_names,
        "fromFunctionNames": from_function_names,
    })
}

#[allow(clippy::too_many_arguments)]
fn walk_deep_plan(
    node: &Value,
    path: &str,
    total_cost: f64,
    total_execution_time: f64,
    functions: &mut Vec<Value>,
    ctes: &mut std::collections::HashMap<String, Value>,
    subqueries: &mut Vec<Value>,
    estimate_skew: &mut Vec<Value>,
) {
    let node_type = node
        .get("Node Type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let node_path = format!("{path}/{node_type}");
    let total_node_cost = node.get("Total Cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let actual_total_time = node
        .get("Actual Total Time")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let plan_rows = node.get("Plan Rows").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let actual_rows = node.get("Actual Rows").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let actual_loops = node
        .get("Actual Loops")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let function_name = node
        .get("Function Name")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let cte_name = node
        .get("CTE Name")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let subplan_name = node
        .get("Subplan Name")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let cost_percent = to_percent(total_node_cost, total_cost);
    let time_percent = to_percent(actual_total_time, total_execution_time);
    let dominant_percent = cost_percent.max(time_percent);

    if node_type.contains("Function Scan") || function_name.is_some() {
        let fname = function_name
            .clone()
            .unwrap_or_else(|| "unknown_function".into());
        let severity = severity_from_percent(dominant_percent);
        functions.push(json!({
            "functionName": fname,
            "nodeType": node_type,
            "path": node_path,
            "cumulativeTimeMs": actual_total_time,
            "cumulativeCost": total_node_cost,
            "loops": actual_loops,
            "estimatedRows": plan_rows,
            "actualRows": actual_rows,
            "severity": severity,
            "reason": format!(
                "{fname} contributes {:.1}% of dominant plan weight",
                dominant_percent
            ),
        }));
    }

    if node_type.contains("CTE Scan") || cte_name.is_some() {
        let name = cte_name.unwrap_or_else(|| "unnamed_cte".into());
        let existing = ctes.entry(name.clone()).or_insert_with(|| {
            json!({
                "cteName": name.clone(),
                "scans": 0u64,
                "cumulativeTimeMs": 0.0,
                "cumulativeCost": 0.0,
                "rowsRead": 0.0,
                "severity": "low",
                "reason": "",
            })
        });
        let scans = existing.get("scans").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        let cum_time = existing
            .get("cumulativeTimeMs")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            + actual_total_time;
        let cum_cost = existing
            .get("cumulativeCost")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            + total_node_cost;
        let rows_read = existing
            .get("rowsRead")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            + actual_rows;
        let cte_percent =
            to_percent(cum_cost, total_cost).max(to_percent(cum_time, total_execution_time));
        let severity = severity_from_percent(cte_percent);
        *existing = json!({
            "cteName": name,
            "scans": scans,
            "cumulativeTimeMs": cum_time,
            "cumulativeCost": cum_cost,
            "rowsRead": rows_read,
            "severity": severity,
            "reason": format!(
                "{name} scanned {scans} time(s), {cte_percent:.1}% dominant contribution"
            ),
        });
    }

    if node_type.contains("Subquery Scan")
        || node_type.contains("InitPlan")
        || node_type.contains("SubPlan")
        || subplan_name.is_some()
    {
        let severity = severity_from_percent(dominant_percent);
        subqueries.push(json!({
            "nodeType": node_type,
            "path": node_path,
            "subplanName": subplan_name,
            "timeMs": actual_total_time,
            "cost": total_node_cost,
            "severity": severity,
            "reason": format!(
                "{node_type} contributes {:.1}% of dominant plan weight",
                dominant_percent
            ),
        }));
    }

    if plan_rows > 0.0 && actual_rows > 0.0 {
        let skew_ratio = (actual_rows / plan_rows).max(plan_rows / actual_rows);
        if skew_ratio >= SKEW_MEDIUM_RATIO {
            let severity = severity_from_skew(skew_ratio);
            estimate_skew.push(json!({
                "nodeType": node_type,
                "path": node_path,
                "planRows": plan_rows,
                "actualRows": actual_rows,
                "skewRatio": skew_ratio,
                "severity": severity,
                "reason": format!(
                    "Planner skew {skew_ratio:.1}x between estimated and actual rows"
                ),
            }));
        }
    }

    if let Some(plans) = node.get("Plans").and_then(|v| v.as_array()) {
        for child in plans {
            walk_deep_plan(
                child,
                &node_path,
                total_cost,
                total_execution_time,
                functions,
                ctes,
                subqueries,
                estimate_skew,
            );
        }
    }
}

fn build_deep_recommendations(
    functions: &[Value],
    ctes: &[Value],
    subqueries: &[Value],
    estimate_skew: &[Value],
) -> Vec<String> {
    let mut recommendations = Vec::new();
    if let Some(f) = functions.iter().find(|f| {
        matches!(
            f.get("severity").and_then(|v| v.as_str()),
            Some("critical" | "high")
        )
    }) {
        let name = f
            .get("functionName")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        recommendations.push(format!(
            "Function scan hotspot on {name}. Inspect function logic and ensure predicates push down before invocation."
        ));
    }
    if let Some(c) = ctes.iter().find(|c| {
        c.get("scans").and_then(|v| v.as_u64()).unwrap_or(0) > 1
            || c.get("severity").and_then(|v| v.as_str()) == Some("critical")
    }) {
        let name = c.get("cteName").and_then(|v| v.as_str()).unwrap_or("cte");
        let scans = c.get("scans").and_then(|v| v.as_u64()).unwrap_or(0);
        recommendations.push(format!(
            "CTE {name} is reused {scans} times. Consider inline rewrite or reducing CTE output width/rows."
        ));
    }
    if let Some(s) = estimate_skew
        .iter()
        .find(|s| s.get("severity").and_then(|v| v.as_str()) == Some("critical"))
    {
        let skew = s.get("skewRatio").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let node_type = s.get("nodeType").and_then(|v| v.as_str()).unwrap_or("node");
        recommendations.push(format!(
            "Severe estimate skew ({skew:.1}x) in {node_type}. Run ANALYZE and review predicate selectivity/index coverage."
        ));
    }
    if let Some(s) = subqueries.iter().find(|s| {
        s.get("timeMs")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            >= EXPENSIVE_NODE_TIME_MS
    }) {
        let node_type = s.get("nodeType").and_then(|v| v.as_str()).unwrap_or("node");
        let time = s.get("timeMs").and_then(|v| v.as_f64()).unwrap_or(0.0);
        recommendations.push(format!(
            "Expensive {node_type} detected ({time:.1}ms). Evaluate join rewrite or pre-aggregation."
        ));
    }
    if recommendations.is_empty() {
        recommendations.push(
            "No deep function/CTE/subquery anti-patterns detected in current plan.".into(),
        );
    }
    recommendations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_explain_analyze_wraps() {
        assert_eq!(
            build_explain_sql("SELECT 1", true),
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT 1"
        );
        assert_eq!(
            build_explain_sql("SELECT 1", false),
            "EXPLAIN (FORMAT JSON) SELECT 1"
        );
    }

    #[test]
    fn extract_metrics_from_seq_scan_plan() {
        let plan = json!({
            "Plan": {
                "Node Type": "Seq Scan",
                "Relation Name": "users",
                "Total Cost": 25.0,
                "Plan Rows": 100,
                "Actual Rows": 100,
                "Actual Total Time": 1.5
            },
            "Planning Time": 0.1,
            "Execution Time": 1.6
        });
        let metrics = extract_plan_metrics(&plan).expect("metrics");
        assert_eq!(metrics["sequentialScans"], 1);
        assert_eq!(metrics["indexScans"], 0);
        let recs = metrics["recommendations"].as_array().unwrap();
        assert!(recs.iter().any(|r| r.as_str().unwrap().contains("indexes")));
    }

    #[test]
    fn deep_plan_flags_estimate_skew() {
        let plan = json!({
            "Plan": {
                "Node Type": "Seq Scan",
                "Relation Name": "users",
                "Total Cost": 100.0,
                "Plan Rows": 10,
                "Actual Rows": 1000,
                "Actual Total Time": 50.0,
                "Actual Loops": 1
            },
            "Execution Time": 50.0
        });
        let deep = analyze_deep_plan(&plan, "SELECT * FROM users").expect("deep");
        let skew = deep["estimateSkew"].as_array().expect("skew arr");
        assert!(!skew.is_empty());
        assert_eq!(skew[0]["severity"], "critical");
        assert!(
            deep["recommendations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r.as_str().unwrap().contains("Severe estimate skew"))
        );
    }
}
