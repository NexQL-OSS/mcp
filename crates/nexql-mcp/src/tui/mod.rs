// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! `nexql-mcp tui` — interactive profile editor + multi-client wiring.

mod app;
mod onboarding;
mod ui;

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use nexql_conn::{ConfigFile, SecretMigrationReport};

/// User-visible status after auto-migrating legacy plaintext credentials on load.
pub(crate) fn secret_migration_status(report: &SecretMigrationReport) -> Option<String> {
    if report.migrated.is_empty() && report.failed.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !report.migrated.is_empty() {
        parts.push(format!(
            "migrated {} profile(s) to OS keyring",
            report.migrated.len()
        ));
    }
    if !report.failed.is_empty() {
        parts.push(format!(
            "{} profile(s) still have plaintext passwords (keyring unavailable)",
            report.failed.len()
        ));
    }
    Some(parts.join("; "))
}

pub(crate) fn load_config_migrated(
    config_path: &Path,
) -> (ConfigFile, Option<String>) {
    if !config_path.exists() {
        return (ConfigFile::default(), None);
    }
    match ConfigFile::load_path_migrated(config_path) {
        Ok((config, report)) => (config, secret_migration_status(&report)),
        Err(e) => (
            ConfigFile::default(),
            Some(format!("failed to load config: {e}")),
        ),
    }
}
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::App;

const TICK: Duration = Duration::from_millis(50);

pub async fn run(config_path: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = config_path
        .or_else(ConfigFile::default_path)
        .ok_or("could not resolve a config path — set $HOME or $NEXQL_MCP_CONFIG")?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, config_path).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

pub async fn run_onboarding(
    config_path: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = config_path
        .or_else(ConfigFile::default_path)
        .ok_or("could not resolve a config path — set $HOME or $NEXQL_MCP_CONFIG")?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = onboarding::run_loop(&mut terminal, config_path).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new(config_path);

    loop {
        app.poll_test();
        terminal.draw(|frame| ui::draw(frame, &app))?;

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.on_key(key);
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
