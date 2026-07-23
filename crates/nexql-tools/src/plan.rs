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
}
