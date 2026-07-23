//! Phase 0 spike binary — prints cold-start / embed / RSS metrics for the README.

use std::time::Instant;

use anyhow::{Context, Result};
use nexql_spike::embed::{EmbeddingModel, MODEL_DIM, MODEL_ID};
use nexql_spike::pg::{connect, sample_catalog, seed_users_orders};

#[tokio::main]
async fn main() -> Result<()> {
    // `--version` / `--help`: cold path without loading candle (startup budget probe).
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("nexql-spike 0.1.0 (phase-0 throwaway)");
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("nexql-spike — Phase 0 throwaway metrics harness");
        println!("  (no flags)     load MiniLM, embed 100 strings, optional PG catalog");
        println!("  --version      print version and exit (cold-start probe)");
        return Ok(());
    }

    let boot = Instant::now();
    println!("nexql-spike metrics");
    println!("model={MODEL_ID} dim={MODEL_DIM}");

    let load_start = Instant::now();
    let model = EmbeddingModel::load().context("load MiniLM")?;
    let load_ms = load_start.elapsed().as_secs_f64() * 1000.0;

    let embed_start = Instant::now();
    let mut texts: Vec<String> = Vec::with_capacity(100);
    texts.push("public.users.email".into());
    for i in 0..99 {
        texts.push(format!("public.misc.col_{i}"));
    }
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let vectors = model.embed_batch(&refs)?;
    let embed_ms = embed_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(vectors.len(), 100);
    assert!(vectors.iter().all(|v| v.len() == MODEL_DIM));

    println!("model_load_ms={load_ms:.1}");
    println!("embed_100_ms={embed_ms:.1}");
    println!("embed_per_obj_ms={:.2}", embed_ms / 100.0);

    match connect(None).await {
        Ok(client) => {
            let row = client.query_one("SELECT version()", &[]).await?;
            let version: String = row.get(0);
            println!(
                "pg_version={}",
                version.split(',').next().unwrap_or(&version)
            );
            seed_users_orders(&client).await?;
            let sample = sample_catalog(&client).await?;
            println!(
                "catalog relations={} columns={} fks={}",
                sample.relation_count, sample.column_count, sample.fk_count
            );
        }
        Err(e) => {
            println!("pg_skipped={e}");
        }
    }

    if let Some(rss_kb) = read_rss_kb() {
        println!("rss_mb={:.1}", rss_kb as f64 / 1024.0);
    }

    if let Ok(path) = std::env::current_exe() {
        if let Ok(meta) = std::fs::metadata(&path) {
            println!("binary_bytes={}", meta.len());
            println!("binary_mb={:.1}", meta.len() as f64 / (1024.0 * 1024.0));
        }
    }

    println!("wall_ms={:.1}", boot.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}

fn read_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb);
        }
    }
    None
}
