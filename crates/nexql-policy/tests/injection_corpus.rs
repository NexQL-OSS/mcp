//! Table-driven injection corpus — 100% must pass for Phase 1 exit.

use nexql_policy::{AccessMode, SqlDecision, validate_readonly_sql, validate_write_sql};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Corpus {
    case: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct WriteCorpus {
    case: Vec<WriteCase>,
}

#[derive(Debug, Deserialize)]
struct Case {
    sql: String,
    expect: String,
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WriteCase {
    sql: String,
    mode: String,
    expect: String,
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
}

#[test]
fn sql_corpus_all_cases() {
    let raw = include_str!("fixtures/sql_corpus.toml");
    let corpus: Corpus = toml::from_str(raw).expect("parse corpus");
    assert!(
        corpus.case.len() >= 40,
        "corpus too small: {}",
        corpus.case.len()
    );

    let mut failures = Vec::new();
    for (i, case) in corpus.case.iter().enumerate() {
        let expect = match case.expect.as_str() {
            "allow" => SqlDecision::Allow,
            "reject" => SqlDecision::Reject,
            other => panic!("case {i}: bad expect {other}"),
        };
        match validate_readonly_sql(&case.sql) {
            Ok(got) if got == expect => {}
            Ok(got) => failures.push(format!(
                "#{i}: expected {expect:?}, got {got:?}\n  sql: {}\n  note: {:?}",
                case.sql, case.note
            )),
            Err(e) => {
                if expect == SqlDecision::Reject {
                    continue;
                }
                failures.push(format!(
                    "#{i}: expected Allow, got parse/policy error {e}\n  sql: {}",
                    case.sql
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} corpus failure(s):\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

#[test]
fn sql_write_corpus_all_cases() {
    let raw = include_str!("fixtures/sql_write_corpus.toml");
    let corpus: WriteCorpus = toml::from_str(raw).expect("parse write corpus");
    assert!(
        corpus.case.len() >= 30,
        "write corpus too small: {}",
        corpus.case.len()
    );

    let mut failures = Vec::new();
    for (i, case) in corpus.case.iter().enumerate() {
        let mode: AccessMode = case
            .mode
            .parse()
            .unwrap_or_else(|_| panic!("case {i}: bad mode {}", case.mode));
        let expect = match case.expect.as_str() {
            "allow" => SqlDecision::Allow,
            "reject" => SqlDecision::Reject,
            other => panic!("case {i}: bad expect {other}"),
        };
        match validate_write_sql(mode, &case.sql) {
            Ok(got) if got == expect => {}
            Ok(got) => failures.push(format!(
                "#{i} [{mode:?}]: expected {expect:?}, got {got:?}\n  sql: {}\n  note: {:?}",
                case.sql, case.note
            )),
            Err(e) => {
                if expect == SqlDecision::Reject {
                    continue;
                }
                failures.push(format!(
                    "#{i} [{mode:?}]: expected Allow, got parse/policy error {e}\n  sql: {}",
                    case.sql
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} write corpus failure(s):\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}
