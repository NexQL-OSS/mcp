//! Shortest join-path BFS over an undirected FK edge list.
//!
//! Port of `pro/src/features/dbindex/joinPath.ts` (+ unreachable message from
//! `ToolExecutor.getJoinPath`). Pure CPU — no I/O.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{JoinEdge, JoinGraph};

/// Maximum hop count (TS: `path.length >= 3` → stop expanding).
pub const MAX_JOIN_HOPS: usize = 3;

/// Path bookkeeping type from TS (`PathStep`) — unused by BFS but kept for parity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStep {
    pub table: String,
    pub edges: Vec<JoinEdge>,
}

/// Shortest undirected path as an edge list, capped at [`MAX_JOIN_HOPS`] hops.
///
/// Returns `Some([])` when `from_table == to_table`. Returns `None` when
/// unreachable within the hop limit (TS `null`).
///
/// **Note:** TS does not skip `disabled` edges; neither does this port.
pub fn find_shortest_join_path(
    from_table: &str,
    to_table: &str,
    graph: &JoinGraph,
) -> Option<Vec<JoinEdge>> {
    if from_table == to_table {
        return Some(Vec::new());
    }

    let mut adj: HashMap<&str, Vec<&JoinEdge>> = HashMap::new();
    for edge in &graph.edges {
        adj.entry(edge.from.as_str()).or_default().push(edge);
        adj.entry(edge.to.as_str()).or_default().push(edge);
    }

    let mut queue: VecDeque<(String, Vec<JoinEdge>)> = VecDeque::new();
    queue.push_back((from_table.to_owned(), Vec::new()));
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(from_table.to_owned());

    while let Some((curr, path)) = queue.pop_front() {
        if path.len() >= MAX_JOIN_HOPS {
            continue;
        }

        let Some(edges) = adj.get(curr.as_str()) else {
            continue;
        };

        for edge in edges {
            let neighbor = if edge.from == curr {
                edge.to.as_str()
            } else {
                edge.from.as_str()
            };

            if visited.contains(neighbor) {
                continue;
            }

            let mut new_path = path.clone();
            new_path.push((*edge).clone());

            if neighbor == to_table {
                return Some(new_path);
            }

            visited.insert(neighbor.to_owned());
            queue.push_back((neighbor.to_owned(), new_path));
        }
    }

    None
}

/// User-facing message when no path exists within [`MAX_JOIN_HOPS`] hops.
///
/// Matches `ToolExecutor.getJoinPath`:  
/// `No join path found between "{a}" and "{b}" within 3 hops.`
pub fn unreachable_join_path_message(a: &str, b: &str) -> String {
    format!("No join path found between \"{a}\" and \"{b}\" within 3 hops.")
}

/// Convenience wrapper: path edges, or the unreachable message string.
pub fn get_join_path(a: &str, b: &str, graph: &JoinGraph) -> Result<Vec<JoinEdge>, String> {
    match find_shortest_join_path(a, b, graph) {
        Some(path) => Ok(path),
        None => Err(unreachable_join_path_message(a, b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(from: &str, to: &str, via: &str) -> JoinEdge {
        JoinEdge {
            from: from.into(),
            to: to.into(),
            via: via.into(),
            cols: vec![("id".into(), "id".into())],
            inferred: None,
            disabled: None,
        }
    }

    fn graph(edges: Vec<JoinEdge>) -> JoinGraph {
        JoinGraph { edges }
    }

    #[test]
    fn find_shortest_join_path_table_driven() {
        // A—B—C—D—E (4 hops A→E)
        let g = graph(vec![
            edge("a", "b", "fk_ab"),
            edge("b", "c", "fk_bc"),
            edge("c", "d", "fk_cd"),
            edge("d", "e", "fk_de"),
            edge("a", "x", "fk_ax"), // dead branch
        ]);

        let cases: &[(&str, &str, Option<&[&str]>)] = &[
            ("a", "a", Some(&[])),                         // same table
            ("a", "b", Some(&["fk_ab"])),                  // 1 hop
            ("a", "c", Some(&["fk_ab", "fk_bc"])),         // 2 hops
            ("a", "d", Some(&["fk_ab", "fk_bc", "fk_cd"])), // 3 hops
            ("a", "e", None),                              // 4 hops — capped
            ("a", "z", None),                              // disconnected
            ("e", "a", None),                              // reverse also 4 hops
            ("d", "a", Some(&["fk_cd", "fk_bc", "fk_ab"])), // reverse 3 hops
        ];

        for (from, to, expected_vias) in cases {
            let got = find_shortest_join_path(from, to, &g);
            match expected_vias {
                None => assert!(got.is_none(), "{from}->{to} expected None, got {got:?}"),
                Some(vias) => {
                    let path = got.unwrap_or_else(|| panic!("{from}->{to} expected Some"));
                    let got_vias: Vec<&str> = path.iter().map(|e| e.via.as_str()).collect();
                    assert_eq!(got_vias, *vias, "{from}->{to}");
                }
            }
        }
    }

    #[test]
    fn inferred_edge_usable() {
        let mut e = edge("orders", "customers", "inferred_oc");
        e.inferred = Some(true);
        let g = graph(vec![e]);
        let path = find_shortest_join_path("orders", "customers", &g).unwrap();
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].inferred, Some(true));
    }

    #[test]
    fn disabled_edges_still_traversed_matching_ts() {
        // Divergence note for callers: TS does not filter disabled.
        let mut e = edge("a", "b", "fk");
        e.disabled = Some(true);
        let g = graph(vec![e]);
        assert!(find_shortest_join_path("a", "b", &g).is_some());
    }

    #[test]
    fn unreachable_message_and_get_join_path() {
        let g = graph(vec![edge("a", "b", "fk")]);
        let msg = unreachable_join_path_message("a", "c");
        assert_eq!(msg, "No join path found between \"a\" and \"c\" within 3 hops.");

        assert!(get_join_path("a", "b", &g).is_ok());
        let err = get_join_path("a", "c", &g).unwrap_err();
        assert_eq!(err, msg);
    }
}
