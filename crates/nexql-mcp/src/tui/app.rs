// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! TUI state machine — screens, form state, and transitions. No rendering or
//! terminal I/O here (see `super::ui` and `super::run`).

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nexql_conn::{ConfigFile, ConnectionReport, ProfileConfig};
use similar::TextDiff;
use tokio::sync::oneshot;

use crate::client_targets;
use crate::init_clients;

pub enum Screen {
    ProfileList,
    ProfileForm,
    Testing,
    SaveConfirm,
    ConfirmDelete,
    ClientPicker,
    DiffReview,
    Summary,
}

/// Where `Esc` on the `Testing` screen should return to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TestReturn {
    List,
    Form,
}

pub enum TestOutcome {
    Idle,
    Running,
    Done(Result<ConnectionReport, String>),
}

pub const FIELD_LABELS: [&str; 9] = [
    "URL (overrides fields below)",
    "Host",
    "Port",
    "Database",
    "User",
    "Password",
    "Password command",
    "SSL mode (disable/prefer/require/verify-full)",
    "Access mode (read/write/admin)",
];

pub const FIELD_DESCRIPTIONS: [&str; 10] = [
    "Profile Name: Unique identifier for this connection profile (e.g., 'dev', 'staging', 'production').",
    "Database URL: PostgreSQL connection string (postgres://user:pass@host:5432/dbname). Overrides individual fields below.",
    "Host: Server address or IP (e.g. 127.0.0.1 or db.example.com). Default: 127.0.0.1.",
    "Port: PostgreSQL server port number. Default: 5432.",
    "Database Name: Target database name to connect to.",
    "User: Username for database authentication.",
    "Password: User password for database authentication (stored securely in local config).",
    "Password Command: Shell command outputting password dynamically (e.g. 'aws secretsmanager ...').",
    "SSL Mode: Security level: disable (local dev) | prefer | require (cloud DB) | verify-full.",
    "Access Mode: MCP permissions policy: read (read-only queries) | write (queries + DML) | admin (full DDL).",
];

const FIELD_COUNT: usize = FIELD_LABELS.len();
/// Focus stops = name field (0) + FIELD_COUNT data fields (1..=FIELD_COUNT).
const FOCUS_STOPS: usize = FIELD_COUNT + 1;

pub struct ProfileForm {
    pub name: String,
    pub name_editable: bool,
    pub fields: [String; FIELD_COUNT],
    pub focused: usize,
}

impl ProfileForm {
    fn empty() -> Self {
        Self {
            name: String::new(),
            name_editable: true,
            fields: std::array::from_fn(|_| String::new()),
            focused: 0,
        }
    }

    fn from_profile(name: &str, profile: &ProfileConfig) -> Self {
        Self {
            name: name.to_string(),
            name_editable: false,
            fields: [
                profile.url.clone().unwrap_or_default(),
                profile.host.clone().unwrap_or_default(),
                profile.port.map(|p| p.to_string()).unwrap_or_default(),
                profile.dbname.clone().unwrap_or_default(),
                profile.user.clone().unwrap_or_default(),
                profile.password.clone().unwrap_or_default(),
                profile.password_command.clone().unwrap_or_default(),
                profile.sslmode.clone().unwrap_or_default(),
                profile.access_mode.clone().unwrap_or_default(),
            ],
            focused: 0,
        }
    }

    /// Currently-focused field as mutable text, if the name field isn't the
    /// one focused (or is editable).
    fn focused_text_mut(&mut self) -> Option<&mut String> {
        if self.focused == 0 {
            return self.name_editable.then_some(&mut self.name);
        }
        self.fields.get_mut(self.focused - 1)
    }

    fn to_profile_config(&self) -> ProfileConfig {
        let get = |i: usize| {
            let s = self.fields[i].trim();
            (!s.is_empty()).then(|| s.to_string())
        };
        ProfileConfig {
            url: get(0),
            host: get(1),
            port: get(2).and_then(|s| s.parse().ok()),
            dbname: get(3),
            user: get(4),
            password: get(5),
            password_command: get(6),
            sslmode: get(7),
            access_mode: get(8),
            ..Default::default()
        }
    }
}

pub struct ClientPickerItem {
    pub key: &'static str,
    pub display_name: &'static str,
    pub mergeable: bool,
    pub selected: bool,
}

pub struct DiffEntry {
    pub display_name: &'static str,
    pub config_path: PathBuf,
    pub old_content: String,
    pub new_content: String,
    pub apply: Option<bool>,
}

pub struct SummaryEntry {
    pub display_name: String,
    pub path: Option<PathBuf>,
    pub backup: Option<PathBuf>,
    pub snippet: Option<String>,
    pub error: Option<String>,
    pub skipped: bool,
}

pub struct App {
    pub config_path: PathBuf,
    pub config: ConfigFile,
    pub profile_names: Vec<String>,
    pub selected: usize,
    pub screen: Screen,
    pub status: Option<String>,
    pub form: ProfileForm,
    pub test_outcome: TestOutcome,
    pub test_rx: Option<oneshot::Receiver<Result<ConnectionReport, String>>>,
    pub test_return: TestReturn,
    pub pending_profile: Option<ProfileConfig>,
    pub picker_items: Vec<ClientPickerItem>,
    pub picker_idx: usize,
    pub diffs: Vec<DiffEntry>,
    pub diff_idx: usize,
    pub summary: Vec<SummaryEntry>,
    pub checked_profiles: std::collections::HashSet<String>,
    pub should_quit: bool,
}

impl App {
    pub fn new(config_path: PathBuf) -> Self {
        let config = if config_path.exists() {
            ConfigFile::load_path(&config_path).unwrap_or_default()
        } else {
            ConfigFile::default()
        };
        let mut profile_names: Vec<String> = config.profiles.keys().cloned().collect();
        profile_names.sort();
        Self {
            config_path,
            config,
            profile_names,
            selected: 0,
            screen: Screen::ProfileList,
            status: None,
            form: ProfileForm::empty(),
            test_outcome: TestOutcome::Idle,
            test_rx: None,
            test_return: TestReturn::List,
            pending_profile: None,
            picker_items: Vec::new(),
            picker_idx: 0,
            diffs: Vec::new(),
            diff_idx: 0,
            summary: Vec::new(),
            checked_profiles: std::collections::HashSet::new(),
            should_quit: false,
        }
    }

    fn refresh_profile_names(&mut self) {
        self.profile_names = self.config.profiles.keys().cloned().collect();
        self.profile_names.sort();
        if self.selected >= self.profile_names.len() {
            self.selected = self.profile_names.len().saturating_sub(1);
        }
    }

    /// Poll the in-flight test-connection task, if any. Call once per UI tick.
    pub fn poll_test(&mut self) {
        if let Some(rx) = &mut self.test_rx
            && let Ok(result) = rx.try_recv()
        {
            self.test_outcome = TestOutcome::Done(result);
            self.test_rx = None;
        }
    }

    fn start_test(&mut self, profile: ProfileConfig, return_to: TestReturn) {
        self.test_return = return_to;
        match nexql_conn::resolve_profile(&profile) {
            Ok(params) => {
                let (tx, rx) = oneshot::channel();
                self.test_rx = Some(rx);
                self.test_outcome = TestOutcome::Running;
                tokio::spawn(async move {
                    let result = nexql_conn::test_connection(&params)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(result);
                });
            }
            Err(e) => {
                self.test_rx = None;
                self.test_outcome = TestOutcome::Done(Err(e.to_string()));
            }
        }
        self.pending_profile = Some(profile);
        self.screen = Screen::Testing;
    }

    fn selected_name(&self) -> Option<String> {
        self.profile_names.get(self.selected).cloned()
    }

    fn open_client_picker(&mut self) {
        self.picker_items = client_targets::mergeable_targets()
            .into_iter()
            .map(|t| ClientPickerItem {
                key: t.key,
                display_name: t.display_name,
                mergeable: true,
                selected: false,
            })
            .chain(
                [
                    ("continue", "Continue (copy YAML snippet)"),
                    ("jetbrains", "JetBrains AI Assistant (copy snippet)"),
                    ("openai-agents", "OpenAI Agents SDK (copy snippet)"),
                ]
                .into_iter()
                .map(|(key, display_name)| ClientPickerItem {
                    key,
                    display_name,
                    mergeable: false,
                    selected: false,
                }),
            )
            .collect();
        self.picker_idx = 0;
        self.screen = Screen::ClientPicker;
    }

    fn build_diffs_and_snippets(&mut self) {
        let mut selected_names: Vec<String> = self
            .profile_names
            .iter()
            .filter(|n| self.checked_profiles.contains(*n))
            .cloned()
            .collect();
        if selected_names.is_empty()
            && let Some(name) = self.selected_name()
        {
            selected_names.push(name);
        }
        if selected_names.is_empty() {
            return;
        }

        let first_profile = self.config.profiles.get(&selected_names[0]).cloned();

        self.diffs.clear();
        self.summary.clear();

        let targets = client_targets::mergeable_targets();
        let mut profile_args: Vec<String> = Vec::new();
        for name in &selected_names {
            profile_args.push("--profile".to_string());
            profile_args.push(name.clone());
        }
        for item in self
            .picker_items
            .iter()
            .filter(|i| i.selected && i.mergeable)
        {
            let Some(target) = targets.iter().find(|t| t.key == item.key) else {
                continue;
            };
            let Some(path) = (target.config_path)() else {
                self.summary.push(SummaryEntry {
                    display_name: item.display_name.to_string(),
                    path: None,
                    backup: None,
                    snippet: None,
                    error: Some("could not resolve a config path on this OS".into()),
                    skipped: true,
                });
                continue;
            };
            let old_content = std::fs::read_to_string(&path).unwrap_or_default();
            match client_targets::merge_entry(
                &old_content,
                target.shape,
                "nexql-mcp",
                "nexql-mcp",
                &profile_args,
            ) {
                Ok(new_content) => self.diffs.push(DiffEntry {
                    display_name: target.display_name,
                    config_path: path,
                    old_content,
                    new_content,
                    apply: None,
                }),
                Err(e) => self.summary.push(SummaryEntry {
                    display_name: item.display_name.to_string(),
                    path: Some(path),
                    backup: None,
                    snippet: None,
                    error: Some(e),
                    skipped: true,
                }),
            }
        }

        // Copy-only clients: build a paste-ready snippet with a resolved URL.
        let url = first_profile
            .as_ref()
            .and_then(|p| nexql_conn::resolve_profile(p).ok())
            .and_then(|p| p.to_url().ok());
        for item in self
            .picker_items
            .iter()
            .filter(|i| i.selected && !i.mergeable)
        {
            match init_clients::init_snippet(item.key, url.as_deref()) {
                Ok(snippet) => self.summary.push(SummaryEntry {
                    display_name: item.display_name.to_string(),
                    path: None,
                    backup: None,
                    snippet: Some(snippet),
                    error: None,
                    skipped: false,
                }),
                Err(e) => self.summary.push(SummaryEntry {
                    display_name: item.display_name.to_string(),
                    path: None,
                    backup: None,
                    snippet: None,
                    error: Some(e),
                    skipped: true,
                }),
            }
        }

        self.diff_idx = 0;
        if self.diffs.is_empty() {
            self.screen = Screen::Summary;
        } else {
            self.screen = Screen::DiffReview;
        }
    }

    fn finalize_diffs(&mut self) {
        for d in &self.diffs {
            if d.apply == Some(true) {
                match nexql_conn::write_with_backup(&d.config_path, &d.new_content) {
                    Ok(backup) => self.summary.push(SummaryEntry {
                        display_name: d.display_name.to_string(),
                        path: Some(d.config_path.clone()),
                        backup,
                        snippet: None,
                        error: None,
                        skipped: false,
                    }),
                    Err(e) => self.summary.push(SummaryEntry {
                        display_name: d.display_name.to_string(),
                        path: Some(d.config_path.clone()),
                        backup: None,
                        snippet: None,
                        error: Some(e.to_string()),
                        skipped: true,
                    }),
                }
            } else {
                self.summary.push(SummaryEntry {
                    display_name: d.display_name.to_string(),
                    path: Some(d.config_path.clone()),
                    backup: None,
                    snippet: None,
                    error: None,
                    skipped: true,
                });
            }
        }
        self.screen = Screen::Summary;
    }

    /// Unified diff text for the diff currently under review.
    pub fn current_diff_text(&self) -> Option<String> {
        let d = self.diffs.get(self.diff_idx)?;
        let diff = TextDiff::from_lines(&d.old_content, &d.new_content);
        Some(diff.unified_diff().context_radius(2).to_string())
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        self.status = None;
        match self.screen {
            Screen::ProfileList => self.on_key_list(key),
            Screen::ProfileForm => self.on_key_form(key),
            Screen::Testing => self.on_key_testing(key),
            Screen::SaveConfirm => self.on_key_save_confirm(key),
            Screen::ConfirmDelete => self.on_key_confirm_delete(key),
            Screen::ClientPicker => self.on_key_picker(key),
            Screen::DiffReview => self.on_key_diff_review(key),
            Screen::Summary => self.on_key_summary(key),
        }
    }

    fn on_key_list(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.profile_names.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Char('n') => {
                self.form = ProfileForm::empty();
                self.screen = Screen::ProfileForm;
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(name) = self.selected_name()
                    && let Some(profile) = self.config.profiles.get(&name).cloned()
                {
                    self.form = ProfileForm::from_profile(&name, &profile);
                    self.screen = Screen::ProfileForm;
                }
            }
            KeyCode::Char('d') => {
                if self.selected_name().is_some() {
                    self.screen = Screen::ConfirmDelete;
                }
            }
            KeyCode::Char('t') => {
                if let Some(name) = self.selected_name()
                    && let Some(profile) = self.config.profiles.get(&name).cloned()
                {
                    self.start_test(profile, TestReturn::List);
                }
            }
            KeyCode::Char(' ') => {
                if let Some(name) = self.selected_name() {
                    if self.checked_profiles.contains(&name) {
                        self.checked_profiles.remove(&name);
                    } else {
                        self.checked_profiles.insert(name);
                    }
                }
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if self.checked_profiles.len() == self.profile_names.len() {
                    self.checked_profiles.clear();
                } else {
                    self.checked_profiles = self.profile_names.iter().cloned().collect();
                }
            }
            KeyCode::Char('w') if self.selected_name().is_some() => {
                self.open_client_picker();
            }
            KeyCode::Char('s') => {
                if let Some(name) = self.selected_name() {
                    self.config.default_profile = Some(name.clone());
                    match self.config.save(&self.config_path) {
                        Ok(_) => self.status = Some(format!("set default profile to '{name}'")),
                        Err(e) => self.status = Some(format!("failed to save config: {e}")),
                    }
                }
            }
            _ => {}
        }
    }

    fn on_key_form(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::ProfileList,
            KeyCode::Tab | KeyCode::Down => {
                self.form.focused = (self.form.focused + 1) % FOCUS_STOPS;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.form.focused = (self.form.focused + FOCUS_STOPS - 1) % FOCUS_STOPS;
            }
            KeyCode::Enter => {
                let name = self.form.name.trim().to_string();
                if name.is_empty() {
                    self.status = Some("profile name cannot be empty".into());
                    return;
                }
                if self.form.name_editable && self.config.profiles.contains_key(&name) {
                    self.status = Some(format!("profile '{name}' already exists"));
                    return;
                }
                let profile = self.form.to_profile_config();
                self.form.name = name;
                self.start_test(profile, TestReturn::Form);
            }
            KeyCode::Backspace => {
                if let Some(text) = self.form.focused_text_mut() {
                    text.pop();
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(text) = self.form.focused_text_mut() {
                    text.push(c);
                }
            }
            _ => {}
        }
    }

    fn on_key_testing(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.test_rx = None;
                self.test_outcome = TestOutcome::Idle;
                self.screen = match self.test_return {
                    TestReturn::List => Screen::ProfileList,
                    TestReturn::Form => Screen::ProfileForm,
                };
            }
            KeyCode::Char('r') => {
                if let Some(profile) = self.pending_profile.clone() {
                    self.start_test(profile, self.test_return);
                }
            }
            KeyCode::Enter => {
                if matches!(self.test_outcome, TestOutcome::Running) {
                    return;
                }
                match self.test_return {
                    TestReturn::List => self.screen = Screen::ProfileList,
                    TestReturn::Form => self.screen = Screen::SaveConfirm,
                }
            }
            _ => {}
        }
    }

    fn on_key_save_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let Some(profile) = self.pending_profile.take() else {
                    self.screen = Screen::ProfileList;
                    return;
                };
                let name = self.form.name.clone();
                self.config.upsert_profile(name, profile);
                match self.config.save(&self.config_path) {
                    Ok(_backup) => {
                        self.refresh_profile_names();
                        self.status = Some(format!("saved to {}", self.config_path.display()));
                    }
                    Err(e) => self.status = Some(format!("save failed: {e}")),
                }
                self.screen = Screen::ProfileList;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.screen = Screen::ProfileForm;
            }
            _ => {}
        }
    }

    fn on_key_confirm_delete(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                if let Some(name) = self.selected_name() {
                    self.config.remove_profile(&name);
                    match self.config.save(&self.config_path) {
                        Ok(_) => self.status = Some(format!("deleted '{name}'")),
                        Err(e) => self.status = Some(format!("delete save failed: {e}")),
                    }
                    self.refresh_profile_names();
                }
                self.screen = Screen::ProfileList;
            }
            KeyCode::Char('n') | KeyCode::Esc => self.screen = Screen::ProfileList,
            _ => {}
        }
    }

    fn on_key_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::ProfileList,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.picker_idx > 0 {
                    self.picker_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.picker_idx + 1 < self.picker_items.len() {
                    self.picker_idx += 1;
                }
            }
            KeyCode::Char(' ') => {
                if let Some(item) = self.picker_items.get_mut(self.picker_idx) {
                    item.selected = !item.selected;
                }
            }
            KeyCode::Char('a') => {
                for item in &mut self.picker_items {
                    item.selected = true;
                }
            }
            KeyCode::Enter => {
                if self.picker_items.iter().any(|i| i.selected) {
                    self.build_diffs_and_snippets();
                } else {
                    self.status = Some("select at least one client with space".into());
                }
            }
            _ => {}
        }
    }

    fn on_key_diff_review(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::ClientPicker,
            KeyCode::Char('y') => {
                if let Some(d) = self.diffs.get_mut(self.diff_idx) {
                    d.apply = Some(true);
                }
                self.diff_idx += 1;
                if self.diff_idx >= self.diffs.len() {
                    self.finalize_diffs();
                }
            }
            KeyCode::Char('s') => {
                if let Some(d) = self.diffs.get_mut(self.diff_idx) {
                    d.apply = Some(false);
                }
                self.diff_idx += 1;
                if self.diff_idx >= self.diffs.len() {
                    self.finalize_diffs();
                }
            }
            KeyCode::Char('a') => {
                for d in self.diffs.iter_mut().skip(self.diff_idx) {
                    d.apply = Some(true);
                }
                self.finalize_diffs();
            }
            _ => {}
        }
    }

    fn on_key_summary(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                self.diffs.clear();
                self.summary.clear();
                self.screen = Screen::ProfileList;
            }
            _ => {}
        }
    }
}
