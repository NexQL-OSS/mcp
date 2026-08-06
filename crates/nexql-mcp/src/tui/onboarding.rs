//! Interactive model onboarding & client configuration wizard TUI.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use nexql_conn::ConfigFile;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};
use similar::TextDiff;

use crate::client_targets;
use crate::init_clients;

const TICK: Duration = Duration::from_millis(50);

pub async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = OnboardingApp::new(config_path);

    loop {
        terminal.draw(|frame| draw(frame, &app))?;

        if event::poll(TICK)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

pub enum OnboardingScreen {
    ModelPicker,
    ConnectionConfig,
    DiffReview,
    Summary,
}

pub struct ModelItem {
    pub key: &'static str,
    pub display_name: &'static str,
    pub mergeable: bool,
    pub selected: bool,
    pub config_path: Option<PathBuf>,
    pub found_on_disk: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolsOption {
    Full,
    Query,
    Dba,
    Meta,
}

impl ToolsOption {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full (all 41 tools & discovery)",
            Self::Query => "Query (read-only query & catalog search)",
            Self::Dba => "DBA (monitoring, admin & maintenance)",
            Self::Meta => "Meta (schema introspection & indexing)",
        }
    }

    pub fn flag_value(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Query => "query",
            Self::Dba => "dba",
            Self::Meta => "meta",
        }
    }
}

pub struct OnboardingApp {
    pub config_path: PathBuf,
    pub _config: ConfigFile,
    pub profile_names: Vec<String>,
    pub screen: OnboardingScreen,

    // Screen 1: Model Picker
    pub model_items: Vec<ModelItem>,
    pub picker_idx: usize,

    // Screen 2: Connection Config
    pub connection_idx: usize, // 0 = select profile, 1 = custom url, 2 = tools, 3 = embeddings
    pub selected_profiles: Vec<bool>,
    pub profile_focus_idx: usize,
    pub custom_url: String,
    pub use_custom_url: bool,
    pub tools_option: ToolsOption,
    pub local_embeddings: bool,

    // Screen 3: Diff Review
    pub diffs: Vec<DiffEntry>,
    pub diff_idx: usize,

    // Screen 4: Summary
    pub summary: Vec<SummaryEntry>,

    pub status: Option<String>,
    pub should_quit: bool,
}

impl OnboardingApp {
    pub fn new(config_path: PathBuf) -> Self {
        let config = if config_path.exists() {
            ConfigFile::load_path(&config_path).unwrap_or_default()
        } else {
            ConfigFile::default()
        };

        let mut profile_names: Vec<String> = config.profiles.keys().cloned().collect();
        profile_names.sort();

        let targets = client_targets::mergeable_targets();
        let mut model_items: Vec<ModelItem> = targets
            .into_iter()
            .map(|t| {
                let path = (t.config_path)();
                let found = path.as_ref().map(|p| p.exists()).unwrap_or(false);
                ModelItem {
                    key: t.key,
                    display_name: t.display_name,
                    mergeable: true,
                    selected: found, // auto-select models detected on disk!
                    config_path: path,
                    found_on_disk: found,
                }
            })
            .collect();

        // Non-mergeable / copy-only targets
        let manual = [
            ("continue", "Continue"),
            ("jetbrains", "JetBrains AI Assistant"),
            ("openai-agents", "OpenAI Agents SDK"),
        ];
        for (key, display_name) in manual {
            model_items.push(ModelItem {
                key,
                display_name,
                mergeable: false,
                selected: false,
                config_path: None,
                found_on_disk: false,
            });
        }

        let num_profiles = profile_names.len();

        Self {
            config_path,
            _config: config,
            profile_names,
            screen: OnboardingScreen::ModelPicker,
            model_items,
            picker_idx: 0,
            connection_idx: 0,
            selected_profiles: vec![true; num_profiles],
            profile_focus_idx: 0,
            custom_url: String::new(),
            use_custom_url: false,
            tools_option: ToolsOption::Full,
            local_embeddings: false,
            diffs: Vec::new(),
            diff_idx: 0,
            summary: Vec::new(),
            status: None,
            should_quit: false,
        }
    }

    pub fn selected_models_count(&self) -> usize {
        self.model_items.iter().filter(|i| i.selected).count()
    }

    pub fn current_diff_text(&self) -> Option<String> {
        let d = self.diffs.get(self.diff_idx)?;
        let diff = TextDiff::from_lines(&d.old_content, &d.new_content);
        Some(diff.unified_diff().context_radius(2).to_string())
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        self.status = None;
        match self.screen {
            OnboardingScreen::ModelPicker => self.on_key_picker(key),
            OnboardingScreen::ConnectionConfig => self.on_key_connection(key),
            OnboardingScreen::DiffReview => self.on_key_diff_review(key),
            OnboardingScreen::Summary => self.on_key_summary(key),
        }
    }

    fn on_key_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.picker_idx > 0 {
                    self.picker_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.picker_idx + 1 < self.model_items.len() {
                    self.picker_idx += 1;
                }
            }
            KeyCode::Char(' ') => {
                if let Some(item) = self.model_items.get_mut(self.picker_idx) {
                    item.selected = !item.selected;
                }
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                let all_selected = self.model_items.iter().all(|i| i.selected);
                for item in &mut self.model_items {
                    item.selected = !all_selected;
                }
            }
            KeyCode::Enter => {
                if self.selected_models_count() == 0 {
                    self.status = Some("Please select at least one model/client to onboard.".into());
                } else {
                    self.screen = OnboardingScreen::ConnectionConfig;
                }
            }
            _ => {}
        }
    }

    fn on_key_connection(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = OnboardingScreen::ModelPicker,
            KeyCode::Tab | KeyCode::Down => {
                self.connection_idx = (self.connection_idx + 1) % 4;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.connection_idx = (self.connection_idx + 3) % 4;
            }
            KeyCode::Left | KeyCode::Char('h') => match self.connection_idx {
                0 => {
                    self.use_custom_url = false;
                    if self.profile_focus_idx > 0 {
                        self.profile_focus_idx -= 1;
                    }
                }
                1 => {
                    self.use_custom_url = true;
                }
                2 => {
                    self.tools_option = match self.tools_option {
                        ToolsOption::Full => ToolsOption::Meta,
                        ToolsOption::Query => ToolsOption::Full,
                        ToolsOption::Dba => ToolsOption::Query,
                        ToolsOption::Meta => ToolsOption::Dba,
                    };
                }
                3 => {
                    self.local_embeddings = !self.local_embeddings;
                }
                _ => {}
            },
            KeyCode::Right | KeyCode::Char('l') => match self.connection_idx {
                0 => {
                    self.use_custom_url = false;
                    if !self.profile_names.is_empty()
                        && self.profile_focus_idx + 1 < self.profile_names.len()
                    {
                        self.profile_focus_idx += 1;
                    }
                }
                1 => {
                    self.use_custom_url = true;
                }
                2 => {
                    self.tools_option = match self.tools_option {
                        ToolsOption::Full => ToolsOption::Query,
                        ToolsOption::Query => ToolsOption::Dba,
                        ToolsOption::Dba => ToolsOption::Meta,
                        ToolsOption::Meta => ToolsOption::Full,
                    };
                }
                3 => {
                    self.local_embeddings = !self.local_embeddings;
                }
                _ => {}
            },
            KeyCode::Char(' ') if self.connection_idx == 0 => {
                self.use_custom_url = false;
                if let Some(s) = self.selected_profiles.get_mut(self.profile_focus_idx) {
                    *s = !*s;
                }
            }
            KeyCode::Char('a') | KeyCode::Char('A') if self.connection_idx == 0 => {
                self.use_custom_url = false;
                let all_selected = self.selected_profiles.iter().all(|&b| b);
                for b in &mut self.selected_profiles {
                    *b = !all_selected;
                }
            }
            KeyCode::Char(c) if self.connection_idx == 1 => {
                self.use_custom_url = true;
                self.custom_url.push(c);
            }
            KeyCode::Backspace if self.connection_idx == 1 => {
                self.use_custom_url = true;
                self.custom_url.pop();
            }
            KeyCode::Char(' ') if self.connection_idx == 3 => {
                self.local_embeddings = !self.local_embeddings;
            }
            KeyCode::Enter => {
                self.build_diffs_and_snippets();
            }
            _ => {}
        }
    }

    fn build_diffs_and_snippets(&mut self) {
        self.diffs.clear();
        self.summary.clear();

        // Construct CLI args for nexql-mcp command inside model configs
        let mut server_args: Vec<String> = Vec::new();

        if self.use_custom_url && !self.custom_url.trim().is_empty() {
            server_args.push(self.custom_url.trim().to_string());
        } else {
            for (name, &selected) in self.profile_names.iter().zip(&self.selected_profiles) {
                if selected {
                    server_args.push("--profile".to_string());
                    server_args.push(name.clone());
                }
            }
        }

        if self.tools_option != ToolsOption::Full {
            server_args.push("--tools".to_string());
            server_args.push(self.tools_option.flag_value().to_string());
        }

        if self.local_embeddings {
            server_args.push("--embeddings".to_string());
            server_args.push("local".to_string());
        }

        let targets = client_targets::mergeable_targets();

        // Process mergeable targets
        for item in self.model_items.iter().filter(|i| i.selected && i.mergeable) {
            let Some(target) = targets.iter().find(|t| t.key == item.key) else {
                continue;
            };
            let Some(path) = (target.config_path)() else {
                self.summary.push(SummaryEntry {
                    display_name: item.display_name.to_string(),
                    path: None,
                    backup: None,
                    snippet: None,
                    error: Some("Could not resolve config path on this OS".into()),
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
                &server_args,
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

        // Process copy-only / manual targets
        let url_arg = if self.use_custom_url && !self.custom_url.trim().is_empty() {
            Some(self.custom_url.trim())
        } else {
            None
        };

        for item in self.model_items.iter().filter(|i| i.selected && !i.mergeable) {
            match init_clients::init_snippet(item.key, url_arg) {
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
            self.screen = OnboardingScreen::Summary;
        } else {
            self.screen = OnboardingScreen::DiffReview;
        }
    }

    fn on_key_diff_review(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = OnboardingScreen::ConnectionConfig,
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(d) = self.diffs.get_mut(self.diff_idx) {
                    d.apply = Some(true);
                }
                self.advance_diff();
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                if let Some(d) = self.diffs.get_mut(self.diff_idx) {
                    d.apply = Some(false);
                }
                self.advance_diff();
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                for d in &mut self.diffs {
                    if d.apply.is_none() {
                        d.apply = Some(true);
                    }
                }
                self.finalize_diffs();
            }
            KeyCode::Right | KeyCode::Char('n') => {
                if self.diff_idx + 1 < self.diffs.len() {
                    self.diff_idx += 1;
                }
            }
            KeyCode::Left | KeyCode::Char('p') => {
                if self.diff_idx > 0 {
                    self.diff_idx -= 1;
                }
            }
            KeyCode::Enter => {
                self.finalize_diffs();
            }
            _ => {}
        }
    }

    fn advance_diff(&mut self) {
        if self.diff_idx + 1 < self.diffs.len() {
            self.diff_idx += 1;
        } else {
            self.finalize_diffs();
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
        self.screen = OnboardingScreen::Summary;
    }

    fn on_key_summary(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                self.should_quit = true;
            }
            _ => {}
        }
    }
}

// -----------------------------------------------------------------------------
// UI Rendering
// -----------------------------------------------------------------------------

pub fn draw(frame: &mut Frame, app: &OnboardingApp) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    draw_header(frame, app, chunks[0]);

    match app.screen {
        OnboardingScreen::ModelPicker => draw_picker(frame, app, chunks[1]),
        OnboardingScreen::ConnectionConfig => draw_connection(frame, app, chunks[1]),
        OnboardingScreen::DiffReview => draw_diff_review(frame, app, chunks[1]),
        OnboardingScreen::Summary => draw_summary(frame, app, chunks[1]),
    }

    draw_status_line(frame, app, chunks[2]);
}

fn draw_header(frame: &mut Frame, app: &OnboardingApp, area: Rect) {
    let step_str = match app.screen {
        OnboardingScreen::ModelPicker => "Step 1/3: Select Models & Clients",
        OnboardingScreen::ConnectionConfig => "Step 2/3: Configure Connection & Flags",
        OnboardingScreen::DiffReview => "Step 3/3: Review & Apply Config Diffs",
        OnboardingScreen::Summary => "Onboarding Summary & Reload Steps",
    };

    let selected_cnt = app.selected_models_count();

    let header_lines = vec![Line::from(vec![
        Span::styled(
            " 🤖 NexQL MCP ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Model Onboarding Wizard",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled(step_str, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("Selected: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{selected_cnt} model(s)"),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("Config: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.config_path.display().to_string(), Style::default().fg(Color::White)),
    ])];

    let header_widget = Paragraph::new(header_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header_widget, area);
}

fn draw_picker(frame: &mut Frame, app: &OnboardingApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(4)])
        .split(area);

    let intro = Paragraph::new(Line::from(vec![
        Span::styled(" Select the AI models, IDEs, and agent SDKs ", Style::default().fg(Color::White)),
        Span::styled("you want to onboard NexQL MCP into:", Style::default().fg(Color::Yellow)),
    ]));
    frame.render_widget(intro, chunks[0]);

    let items: Vec<ListItem> = app
        .model_items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_focused = idx == app.picker_idx;
            let check = if item.selected { "[x] " } else { "[ ] " };
            let check_style = if item.selected {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let name_style = if is_focused {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let status_badge = if item.found_on_disk {
                Span::styled(" [Detected on Disk] ", Style::default().fg(Color::Green))
            } else if item.mergeable {
                Span::styled(" [Config Auto-Merge] ", Style::default().fg(Color::Cyan))
            } else {
                Span::styled(" [Copy Snippet] ", Style::default().fg(Color::LightYellow))
            };

            let path_info = if let Some(ref path) = item.config_path {
                Span::styled(
                    format!(" ({})", path.display()),
                    Style::default().fg(Color::DarkGray),
                )
            } else {
                Span::raw("")
            };

            let cursor = if is_focused { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(cursor, Style::default().fg(Color::Yellow)),
                Span::styled(check, check_style),
                Span::styled(item.display_name, name_style),
                status_badge,
                path_info,
            ]);

            ListItem::new(line)
        })
        .collect();

    let list_widget = List::new(items).block(
        Block::default()
            .title(" Supported AI Models & Clients ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(list_widget, chunks[1]);
}

fn draw_connection(frame: &mut Frame, app: &OnboardingApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Profile selection
            Constraint::Length(4), // Custom URL input
            Constraint::Length(4), // Tool Profile
            Constraint::Length(4), // Embeddings
            Constraint::Min(2),
        ])
        .split(area);

    // Option 1: Saved Profiles
    let focused_0 = app.connection_idx == 0;
    let mut profile_spans: Vec<Span> = Vec::new();
    if app.profile_names.is_empty() {
        profile_spans.push(Span::styled(
            "No saved profiles found (use custom URL input below)",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        for (idx, (name, &selected)) in app
            .profile_names
            .iter()
            .zip(&app.selected_profiles)
            .enumerate()
        {
            let is_focused_item = focused_0 && idx == app.profile_focus_idx;
            let check = if selected { "[x] " } else { "[ ] " };
            let check_style = if selected {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let name_style = if is_focused_item {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            profile_spans.push(Span::styled(check, check_style));
            profile_spans.push(Span::styled(name, name_style));
            profile_spans.push(Span::styled("   ", Style::default()));
        }
    }

    let border_style_0 = if focused_0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let profile_widget = Paragraph::new(vec![
        Line::from(profile_spans),
        Line::from(Span::styled(
            "Use ← / → to move focus, Space to toggle profile, A to toggle all",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .title(" 1. Saved Database Profiles (Multi-Select) ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style_0),
    );
    frame.render_widget(profile_widget, chunks[0]);

    // Option 2: Custom URL
    let focused_1 = app.connection_idx == 1;
    let url_display = if app.custom_url.is_empty() {
        "postgres://user:pass@host:5432/dbname"
    } else {
        &app.custom_url
    };
    let border_style_1 = if focused_1 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let url_widget = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Custom PostgreSQL URL: ", Style::default().fg(Color::White)),
            Span::styled(url_display, Style::default().fg(if app.custom_url.is_empty() { Color::DarkGray } else { Color::Green })),
        ]),
        Line::from(Span::styled("Type to enter custom URL (overrides selected profile)", Style::default().fg(Color::DarkGray))),
    ])
    .block(
        Block::default()
            .title(" 2. Custom Connection URL (Optional) ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style_1),
    );
    frame.render_widget(url_widget, chunks[1]);

    // Option 3: Tool surface profile
    let focused_2 = app.connection_idx == 2;
    let border_style_2 = if focused_2 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let tool_widget = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Tool Profile: ", Style::default().fg(Color::White)),
            Span::styled(app.tools_option.label(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(Span::styled("Use ← / → arrows to cycle tool profiles (full/query/dba/meta)", Style::default().fg(Color::DarkGray))),
    ])
    .block(
        Block::default()
            .title(" 3. MCP Tool Surface Profile ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style_2),
    );
    frame.render_widget(tool_widget, chunks[2]);

    // Option 4: Local Embeddings
    let focused_3 = app.connection_idx == 3;
    let border_style_3 = if focused_3 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let emb_str = if app.local_embeddings {
        "[x] Enabled (MiniLM vector search & semantic schema search)"
    } else {
        "[ ] Disabled (off)"
    };
    let emb_widget = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Semantic Embeddings: ", Style::default().fg(Color::White)),
            Span::styled(emb_str, Style::default().fg(if app.local_embeddings { Color::Green } else { Color::DarkGray })),
        ]),
        Line::from(Span::styled("Press Spacebar to toggle semantic embeddings index", Style::default().fg(Color::DarkGray))),
    ])
    .block(
        Block::default()
            .title(" 4. MiniLM Embeddings ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style_3),
    );
    frame.render_widget(emb_widget, chunks[3]);
}

fn draw_diff_review(frame: &mut Frame, app: &OnboardingApp, area: Rect) {
    if app.diffs.is_empty() {
        let msg = Paragraph::new("No mergeable configs to review.").block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        );
        frame.render_widget(msg, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    let d = &app.diffs[app.diff_idx];

    let status_str = match d.apply {
        Some(true) => "APPLY",
        Some(false) => "SKIP",
        None => "PENDING REVIEW",
    };
    let status_color = match d.apply {
        Some(true) => Color::Green,
        Some(false) => Color::Red,
        None => Color::Yellow,
    };

    let title_line = Line::from(vec![
        Span::styled(
            format!(" Config Diff {}/{} — ", app.diff_idx + 1, app.diffs.len()),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(d.display_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" (", Style::default().fg(Color::DarkGray)),
        Span::styled(d.config_path.display().to_string(), Style::default().fg(Color::White)),
        Span::styled(") │ Status: ", Style::default().fg(Color::DarkGray)),
        Span::styled(status_str, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
    ]);
    let title_bar = Paragraph::new(title_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(title_bar, chunks[0]);

    let diff_text = app.current_diff_text().unwrap_or_default();
    let lines: Vec<Line> = diff_text
        .lines()
        .map(|l| {
            if l.starts_with('+') {
                Line::styled(l, Style::default().fg(Color::Green))
            } else if l.starts_with('-') {
                Line::styled(l, Style::default().fg(Color::Red))
            } else if l.starts_with('@') {
                Line::styled(l, Style::default().fg(Color::Cyan))
            } else {
                Line::styled(l, Style::default().fg(Color::DarkGray))
            }
        })
        .collect();

    let diff_widget = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .title(" Proposed Config Merge ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(diff_widget, chunks[1]);
}

fn draw_summary(frame: &mut Frame, app: &OnboardingApp, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "🎉 Onboarding Complete!",
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::raw(""));

    for s in &app.summary {
        if s.skipped {
            lines.push(Line::from(vec![
                Span::styled(" ⏭  ", Style::default().fg(Color::Yellow)),
                Span::styled(&s.display_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(" — Skipped ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    s.error.as_deref().unwrap_or(""),
                    Style::default().fg(Color::Red),
                ),
            ]));
        } else if let Some(ref path) = s.path {
            lines.push(Line::from(vec![
                Span::styled(" ✅ ", Style::default().fg(Color::Green)),
                Span::styled(&s.display_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(" — Written to ", Style::default().fg(Color::DarkGray)),
                Span::styled(path.display().to_string(), Style::default().fg(Color::Cyan)),
            ]));
            if let Some(ref backup) = s.backup {
                lines.push(Line::from(vec![
                    Span::styled("    Backup saved: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(backup.display().to_string(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        } else if let Some(ref snippet) = s.snippet {
            lines.push(Line::from(vec![
                Span::styled(" 📋 ", Style::default().fg(Color::Yellow)),
                Span::styled(&s.display_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(" — Manual Snippet Required:", Style::default().fg(Color::Yellow)),
            ]));
            for snip_line in snippet.lines() {
                lines.push(Line::styled(
                    format!("    {snip_line}"),
                    Style::default().fg(Color::LightYellow),
                ));
            }
        }
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(Span::styled(
        "💡 Next Steps:",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::styled(
        "   1. Restart your AI client/IDE (Claude Desktop, Cursor, Zed, Windsurf, VS Code).",
        Style::default().fg(Color::White),
    ));
    lines.push(Line::styled(
        "   2. The 'nexql-mcp' tool server will automatically connect to your database.",
        Style::default().fg(Color::White),
    ));

    let summary_widget = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .title(" Summary & Next Steps ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(summary_widget, area);
}

fn draw_status_line(frame: &mut Frame, app: &OnboardingApp, area: Rect) {
    let line = if let Some(msg) = &app.status {
        Line::from(vec![
            Span::styled(" ℹ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(msg, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ])
    } else {
        match app.screen {
            OnboardingScreen::ModelPicker => Line::from(vec![
                Span::styled(" [Space]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" Toggle ", Style::default().fg(Color::White)),
                Span::styled("[A]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" Select/Deselect All ", Style::default().fg(Color::White)),
                Span::styled("[Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" Next: Connection ", Style::default().fg(Color::White)),
                Span::styled("[Esc/Q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(" Quit", Style::default().fg(Color::White)),
            ]),
            OnboardingScreen::ConnectionConfig => Line::from(vec![
                Span::styled(" [Tab/Shift+Tab]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" Switch Option ", Style::default().fg(Color::White)),
                Span::styled("[←/→]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" Change ", Style::default().fg(Color::White)),
                Span::styled("[Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" Review Diffs ", Style::default().fg(Color::White)),
                Span::styled("[Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(" Back", Style::default().fg(Color::White)),
            ]),
            OnboardingScreen::DiffReview => Line::from(vec![
                Span::styled(" [Y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" Apply ", Style::default().fg(Color::White)),
                Span::styled("[S]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" Skip ", Style::default().fg(Color::White)),
                Span::styled("[A]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" Apply All ", Style::default().fg(Color::White)),
                Span::styled("[Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" Finalize ", Style::default().fg(Color::White)),
                Span::styled("[Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(" Back", Style::default().fg(Color::White)),
            ]),
            OnboardingScreen::Summary => Line::from(vec![
                Span::styled(" [Enter/Esc/Q]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" Exit Wizard", Style::default().fg(Color::White)),
            ]),
        }
    };

    let footer_widget = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(footer_widget, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_app_initialization() {
        let temp_dir = std::env::temp_dir().join("nexql_onboarding_test");
        let config_path = temp_dir.join("config.toml");
        let app = OnboardingApp::new(config_path);

        assert!(!app.model_items.is_empty());
        assert_eq!(app.picker_idx, 0);
        assert_eq!(app.tools_option, ToolsOption::Full);
        assert!(!app.local_embeddings);
    }

    #[test]
    fn toggle_and_select_all_models() {
        let temp_dir = std::env::temp_dir().join("nexql_onboarding_test2");
        let config_path = temp_dir.join("config.toml");
        let mut app = OnboardingApp::new(config_path);

        let initial_count = app.selected_models_count();
        app.on_key(KeyEvent::from(KeyCode::Char('a')));
        let count_after_a = app.selected_models_count();
        assert!(count_after_a == 0 || count_after_a == app.model_items.len());
        let _ = initial_count;
    }
}
