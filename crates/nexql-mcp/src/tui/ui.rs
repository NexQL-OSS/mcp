//! Rendering for each TUI screen. Pure — takes `&App`, draws, no state mutation.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, FIELD_LABELS, Screen, TestOutcome};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    match app.screen {
        Screen::ProfileList => draw_profile_list(frame, app, chunks[0]),
        Screen::ProfileForm => draw_profile_form(frame, app, chunks[0]),
        Screen::Testing => draw_testing(frame, app, chunks[0]),
        Screen::SaveConfirm => draw_save_confirm(frame, app, chunks[0]),
        Screen::ConfirmDelete => draw_confirm_delete(frame, app, chunks[0]),
        Screen::ClientPicker => draw_client_picker(frame, app, chunks[0]),
        Screen::DiffReview => draw_diff_review(frame, app, chunks[0]),
        Screen::Summary => draw_summary(frame, app, chunks[0]),
    }

    draw_status_line(frame, app, chunks[1]);
}

fn draw_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let text = app.status.clone().unwrap_or_else(|| match app.screen {
        Screen::ProfileList => {
            "n new  e/Enter edit  d delete  t test  w wire into clients  q quit".into()
        }
        Screen::ProfileForm => {
            "Tab/Shift+Tab move  type to edit  Enter test+continue  Esc cancel".into()
        }
        Screen::Testing => "Enter continue  r retry  Esc back".into(),
        Screen::SaveConfirm => "y save  n/Esc back to form".into(),
        Screen::ConfirmDelete => "y confirm delete  n/Esc cancel".into(),
        Screen::ClientPicker => "space toggle  a select all  Enter continue  Esc back".into(),
        Screen::DiffReview => "y apply  s skip  a apply all remaining  Esc cancel wiring".into(),
        Screen::Summary => "Enter/Esc back to profile list".into(),
    });
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn draw_profile_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = if app.profile_names.is_empty() {
        vec![ListItem::new("no profiles yet — press 'n' to create one")]
    } else {
        app.profile_names
            .iter()
            .map(|name| {
                let profile = &app.config.profiles[name];
                let desc = profile.url.clone().unwrap_or_else(|| {
                    format!(
                        "{}@{}:{}/{}",
                        profile.user.as_deref().unwrap_or("?"),
                        profile.host.as_deref().unwrap_or("?"),
                        profile
                            .port
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "5432".into()),
                        profile.dbname.as_deref().unwrap_or("?"),
                    )
                });
                let default_marker = if app.config.default_profile.as_deref() == Some(name.as_str())
                {
                    " (default)"
                } else {
                    ""
                };
                ListItem::new(format!("{name}{default_marker} — {desc}"))
            })
            .collect()
    };
    let mut state = ListState::default();
    if !app.profile_names.is_empty() {
        state.select(Some(app.selected));
    }
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("nexql-mcp — profiles"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_profile_form(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::with_capacity(FIELD_LABELS.len() + 2);
    let name_style = if app.form.focused == 0 {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    lines.push(Line::from(vec![
        Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.form.name.clone(), name_style),
    ]));
    lines.push(Line::from(""));
    for (i, label) in FIELD_LABELS.iter().enumerate() {
        let focused = app.form.focused == i + 1;
        let style = if focused {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{label}: "), Style::default().fg(Color::DarkGray)),
            Span::styled(app.form.fields[i].clone(), style),
        ]));
    }
    let title = if app.form.name_editable {
        "New profile"
    } else {
        "Edit profile"
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn draw_testing(frame: &mut Frame, app: &App, area: Rect) {
    let text = match &app.test_outcome {
        TestOutcome::Idle => "idle".to_string(),
        TestOutcome::Running => "connecting…".to_string(),
        TestOutcome::Done(Ok(report)) => format!(
            "connected in {:.0}ms\nserver: {}\nsuperuser: {}",
            report.latency.as_secs_f64() * 1000.0,
            report.server_version,
            report.is_superuser,
        ),
        TestOutcome::Done(Err(e)) => format!("connection failed:\n{e}"),
    };
    let style = match &app.test_outcome {
        TestOutcome::Done(Err(_)) => Style::default().fg(Color::Red),
        TestOutcome::Done(Ok(_)) => Style::default().fg(Color::Green),
        _ => Style::default(),
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(style)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Test connection"),
            ),
        area,
    );
}

fn draw_save_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let rendered = app
        .pending_profile
        .as_ref()
        .and_then(|p| toml::to_string_pretty(p).ok())
        .unwrap_or_default();
    let text = format!(
        "Save profile '{}' to {}?\n\n{rendered}",
        app.form.name,
        app.config_path.display()
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Save?")),
        area,
    );
}

fn draw_confirm_delete(frame: &mut Frame, app: &App, area: Rect) {
    let name = app
        .profile_names
        .get(app.selected)
        .cloned()
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(format!("Delete profile '{name}'? This cannot be undone."))
            .style(Style::default().fg(Color::Red))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Confirm delete"),
            ),
        area,
    );
}

fn draw_client_picker(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .picker_items
        .iter()
        .map(|item| {
            let checkbox = if item.selected { "[x]" } else { "[ ]" };
            let kind = if item.mergeable { "" } else { " (copy-only)" };
            ListItem::new(format!("{checkbox} {}{kind}", item.display_name))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.picker_idx));
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Wire profile into clients"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_diff_review(frame: &mut Frame, app: &App, area: Rect) {
    let Some(entry) = app.diffs.get(app.diff_idx) else {
        frame.render_widget(Paragraph::new("no diffs"), area);
        return;
    };
    let diff_text = app.current_diff_text().unwrap_or_default();
    let text = format!(
        "{} of {}: {}\n{}\n\n{diff_text}",
        app.diff_idx + 1,
        app.diffs.len(),
        entry.display_name,
        entry.config_path.display(),
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Diff review")),
        area,
    );
}

fn draw_summary(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();
    for entry in &app.summary {
        if let Some(err) = &entry.error {
            lines.push(Line::from(Span::styled(
                format!("✗ {} — {err}", entry.display_name),
                Style::default().fg(Color::Red),
            )));
        } else if entry.skipped {
            lines.push(Line::from(format!("- {} skipped", entry.display_name)));
        } else if let Some(path) = &entry.path {
            let backup_note = entry
                .backup
                .as_ref()
                .map(|b| format!(" (backup: {})", b.display()))
                .unwrap_or_default();
            lines.push(Line::from(Span::styled(
                format!(
                    "✓ {} — wrote {}{backup_note}",
                    entry.display_name,
                    path.display()
                ),
                Style::default().fg(Color::Green),
            )));
        } else if let Some(snippet) = &entry.snippet {
            lines.push(Line::from(format!(
                "— {} (copy manually):",
                entry.display_name
            )));
            for line in snippet.lines() {
                lines.push(Line::from(format!("    {line}")));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from("nothing to report"));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Summary")),
        area,
    );
}
