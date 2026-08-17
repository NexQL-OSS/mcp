// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Rendering for each TUI screen. Pure — takes `&App`, draws, no state mutation.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, FIELD_DESCRIPTIONS, FIELD_LABELS, Screen, TestOutcome};

pub fn draw(frame: &mut Frame, app: &App) {
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
        Screen::ProfileList => draw_profile_list(frame, app, chunks[1]),
        Screen::ProfileForm => draw_profile_form(frame, app, chunks[1]),
        Screen::Testing => draw_testing(frame, app, chunks[1]),
        Screen::SaveConfirm => draw_save_confirm(frame, app, chunks[1]),
        Screen::ConfirmDelete => draw_confirm_delete(frame, app, chunks[1]),
        Screen::ClientPicker => draw_client_picker(frame, app, chunks[1]),
        Screen::DiffReview => draw_diff_review(frame, app, chunks[1]),
        Screen::Summary => draw_summary(frame, app, chunks[1]),
    }

    draw_status_line(frame, app, chunks[2]);
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let total_profiles = app.profile_names.len();
    let default_name = app.config.default_profile.as_deref().unwrap_or("none");

    let header_lines = vec![Line::from(vec![
        Span::styled(
            " ⚡ NexQL MCP ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Setup & Profile Manager",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("Config: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.config_path.display().to_string(),
            Style::default().fg(Color::White),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("Profiles: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{total_profiles}"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("Default: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            default_name,
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    let header_widget = Paragraph::new(header_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header_widget, area);
}

fn draw_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(msg) = &app.status {
        Line::from(vec![
            Span::styled(
                " ℹ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                msg,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        match app.screen {
            Screen::ProfileList => Line::from(vec![
                Span::styled(
                    " [Space]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Toggle ", Style::default().fg(Color::White)),
                Span::styled(
                    "[N]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" New ", Style::default().fg(Color::White)),
                Span::styled(
                    "[E/Enter]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Edit ", Style::default().fg(Color::White)),
                Span::styled(
                    "[T]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Test ", Style::default().fg(Color::White)),
                Span::styled(
                    "[W]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Wire Clients ", Style::default().fg(Color::White)),
                Span::styled(
                    "[S]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Default ", Style::default().fg(Color::White)),
                Span::styled(
                    "[D]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Del ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Q]",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Quit", Style::default().fg(Color::White)),
            ]),
            Screen::ProfileForm => Line::from(vec![
                Span::styled(
                    " [Tab/Shift+Tab]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Move ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Type]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Edit ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Enter]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Test & Continue ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Esc]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Cancel", Style::default().fg(Color::White)),
            ]),
            Screen::Testing => Line::from(vec![
                Span::styled(
                    " [Enter]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Continue ", Style::default().fg(Color::White)),
                Span::styled(
                    "[R]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Retry ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Esc]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Back", Style::default().fg(Color::White)),
            ]),
            Screen::SaveConfirm => Line::from(vec![
                Span::styled(
                    " [Y]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Save Profile ", Style::default().fg(Color::White)),
                Span::styled(
                    "[N/Esc]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Back to Form", Style::default().fg(Color::White)),
            ]),
            Screen::ConfirmDelete => Line::from(vec![
                Span::styled(
                    " [Y]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Confirm Delete ", Style::default().fg(Color::White)),
                Span::styled(
                    "[N/Esc]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Cancel", Style::default().fg(Color::White)),
            ]),
            Screen::ClientPicker => Line::from(vec![
                Span::styled(
                    " [Space]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Toggle ", Style::default().fg(Color::White)),
                Span::styled(
                    "[A]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Select All ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Enter]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Review Diffs ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Esc]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Back", Style::default().fg(Color::White)),
            ]),
            Screen::DiffReview => Line::from(vec![
                Span::styled(
                    " [Y]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Apply ", Style::default().fg(Color::White)),
                Span::styled(
                    "[S]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Skip ", Style::default().fg(Color::White)),
                Span::styled(
                    "[A]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Apply All ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Esc]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Cancel", Style::default().fg(Color::White)),
            ]),
            Screen::Summary => Line::from(vec![
                Span::styled(
                    " [Enter/Esc]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " Back to Profile Manager",
                    Style::default().fg(Color::White),
                ),
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

fn draw_profile_list(frame: &mut Frame, app: &App, area: Rect) {
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let items: Vec<ListItem> = if app.profile_names.is_empty() {
        vec![ListItem::new(" No profiles yet — press 'n' to create one")]
    } else {
        app.profile_names
            .iter()
            .map(|name| {
                let profile = &app.config.profiles[name];
                let is_default = app.config.default_profile.as_deref() == Some(name.as_str());

                let is_checked = app.checked_profiles.contains(name);
                let check_badge = if is_checked { "[x] " } else { "[ ] " };
                let check_style = if is_checked {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let prefix = if is_default { "★ " } else { "  " };
                let access = profile.access_mode.as_deref().unwrap_or("read");
                let access_tag = match access {
                    "admin" => "[ADMIN]",
                    "write" => "[WRITE]",
                    _ => "[READ]",
                };

                let title_line = Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(check_badge, check_style),
                    Span::styled(
                        name,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(access_tag, Style::default().fg(Color::Cyan)),
                ]);

                ListItem::new(title_line)
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
                .border_type(BorderType::Rounded)
                .title(format!(" Profiles ({}) ", app.profile_names.len())),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, panels[0], &mut state);

    // Inspector Card (Right Panel)
    draw_profile_inspector(frame, app, panels[1]);
}

fn draw_profile_inspector(frame: &mut Frame, app: &App, area: Rect) {
    let inspector_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Profile Details Inspector ");

    let Some(selected_name) = app.profile_names.get(app.selected) else {
        let empty_msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                " No profile selected.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Press 'n' to launch the connection setup wizard.",
                Style::default().fg(Color::Yellow),
            )),
        ])
        .block(inspector_block);
        frame.render_widget(empty_msg, area);
        return;
    };

    let Some(profile) = app.config.profiles.get(selected_name) else {
        return;
    };

    let is_default = app.config.default_profile.as_deref() == Some(selected_name.as_str());

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(" Profile Name:   ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            selected_name,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        if is_default {
            Span::styled(
                "  ★ (Default)",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::from(""));

    let access = profile.access_mode.as_deref().unwrap_or("read");
    let (access_label, access_color) = match access {
        "admin" => ("ADMIN (Full DDL & Admin)", Color::Red),
        "write" => ("WRITE (Read & Data Mutations)", Color::Yellow),
        _ => ("READ-ONLY (Safe Querying)", Color::Green),
    };
    lines.push(Line::from(vec![
        Span::styled(" Access Policy:  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            access_label,
            Style::default()
                .fg(access_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(url) = &profile.url {
        lines.push(Line::from(vec![
            Span::styled(" Connection URL: ", Style::default().fg(Color::DarkGray)),
            Span::styled(url, Style::default().fg(Color::Cyan)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(" Host & Port:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{}:{}",
                    profile.host.as_deref().unwrap_or("127.0.0.1"),
                    profile.port.unwrap_or(5432)
                ),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Database Name:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                profile.dbname.as_deref().unwrap_or("(not set)"),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Database User:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                profile.user.as_deref().unwrap_or("(not set)"),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled(" SSL Mode:       ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            profile.sslmode.as_deref().unwrap_or("prefer"),
            Style::default().fg(Color::White),
        ),
    ]));

    let pass_info = if profile.credential_provider.as_deref() == Some("keyring") {
        "●●●●●●●● (OS keyring)"
    } else if profile.password.is_some() {
        "●●●●●●●● (inline — migrate to keyring on save)"
    } else if let Some(cmd) = &profile.password_command {
        cmd.as_str()
    } else {
        "None"
    };
    lines.push(Line::from(vec![
        Span::styled(" Password Auth:  ", Style::default().fg(Color::DarkGray)),
        Span::styled(pass_info, Style::default().fg(Color::White)),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Quick Actions:",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "  • Press [E] to modify connection fields",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "  • Press [T] to test database connection live",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "  • Press [W] to auto-wire into Cursor, Claude, Zed, VS Code",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "  • Press [S] to set as default profile",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).block(inspector_block);
    frame.render_widget(paragraph, area);
}

fn draw_profile_form(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(12), Constraint::Length(4)])
        .split(area);

    let title = if app.form.name_editable {
        " New Profile — Connection Wizard "
    } else {
        " Edit Profile "
    };

    let mut lines = Vec::with_capacity(FIELD_LABELS.len() + 3);

    // Profile Name line
    let name_focused = app.form.focused == 0;
    let name_prefix = if name_focused { " ▶ " } else { "   " };
    let name_style = if name_focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    lines.push(Line::from(vec![
        Span::styled(
            name_prefix,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Profile Name:     ",
            Style::default().fg(if name_focused {
                Color::Cyan
            } else {
                Color::DarkGray
            }),
        ),
        Span::styled(&app.form.name, name_style),
        if !app.form.name_editable {
            Span::styled(" (read-only)", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::from(""));

    for (i, label) in FIELD_LABELS.iter().enumerate() {
        let focused = app.form.focused == i + 1;
        let prefix = if focused { " ▶ " } else { "   " };
        let label_style = Style::default().fg(if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        });
        let val_style = if focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let val = &app.form.fields[i];
        let display_val = if val.is_empty() {
            match i {
                1 => "(e.g., 127.0.0.1)",
                2 => "(default: 5432)",
                7 => "(default: prefer)",
                8 => "(default: read)",
                _ => "(optional)",
            }
        } else if i == 5 {
            "●●●●●●●●"
        } else {
            val.as_str()
        };

        let val_span_style = if val.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            val_style
        };

        lines.push(Line::from(vec![
            Span::styled(
                prefix,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{label:<32} "), label_style),
            Span::styled(display_val, val_span_style),
        ]));
    }

    let form_widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title),
    );
    frame.render_widget(form_widget, chunks[0]);

    // Field Context / Help Card
    let guide_idx = app.form.focused.min(FIELD_DESCRIPTIONS.len() - 1);
    let help_text = FIELD_DESCRIPTIONS[guide_idx];

    let help_widget = Paragraph::new(vec![Line::from(vec![
        Span::styled(" 💡 ", Style::default().fg(Color::Yellow)),
        Span::styled(help_text, Style::default().fg(Color::White)),
    ])])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Field Guide "),
    );
    frame.render_widget(help_widget, chunks[1]);

    // Cursor placement
    let (label_len, text_len) = if app.form.focused == 0 {
        ("   Profile Name:     ".len(), app.form.name.chars().count())
    } else {
        let i = app.form.focused - 1;
        (
            format!("   {:<32} ", FIELD_LABELS[i]).len(),
            app.form.fields[i].chars().count(),
        )
    };
    let line_idx = if app.form.focused == 0 {
        0
    } else {
        app.form.focused + 1
    };
    let cursor_x = chunks[0].x + 1 + label_len as u16 + text_len as u16;
    let cursor_y = chunks[0].y + 1 + line_idx as u16;
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_testing(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Connection Diagnostic Tester ");

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    match &app.test_outcome {
        TestOutcome::Idle => {
            lines.push(Line::from(Span::styled(
                " Ready to initiate connection test.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        TestOutcome::Running => {
            lines.push(Line::from(vec![
                Span::styled(" ⏳ ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    "Initiating PostgreSQL connection handshake...",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "    Resolving profile credentials and validating socket/TLS parameters...",
                Style::default().fg(Color::DarkGray),
            )));
        }
        TestOutcome::Done(Ok(report)) => {
            lines.push(Line::from(vec![
                Span::styled(
                    " ✔ ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "CONNECTION SUCCESSFUL",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "   • Handshake Latency: ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:.1} ms", report.latency.as_secs_f64() * 1000.0),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "   • Server Version:    ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(&report.server_version, Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "   • Superuser Status:  ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    if report.is_superuser {
                        "Yes (Warning: elevated permissions)"
                    } else {
                        "No (Standard user)"
                    },
                    Style::default().fg(if report.is_superuser {
                        Color::Yellow
                    } else {
                        Color::Green
                    }),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "   Press [Enter] to proceed and save profile.",
                Style::default().fg(Color::Yellow),
            )));
        }
        TestOutcome::Done(Err(e)) => {
            lines.push(Line::from(vec![
                Span::styled(
                    " ✖ ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "CONNECTION FAILED",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("   Error Message: ", Style::default().fg(Color::DarkGray)),
                Span::styled(e, Style::default().fg(Color::Red)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Troubleshooting Tips:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "   • Verify host & port are reachable from this machine.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "   • Check database name, username, and password.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "   • Ensure pg_hba.conf allows connections for this user.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "   Press [R] to retry, or [Esc] to return to profile editor.",
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    let widget = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

fn draw_save_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let rendered = app
        .pending_profile
        .as_ref()
        .and_then(|p| toml::to_string_pretty(p).ok())
        .unwrap_or_default();

    let mut lines = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Save profile ", Style::default().fg(Color::White)),
        Span::styled(
            format!("'{}'", app.form.name),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to ", Style::default().fg(Color::White)),
        Span::styled(
            app.config_path.display().to_string(),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("?", Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " TOML Profile Configuration Preview:",
        Style::default().fg(Color::DarkGray),
    )));

    for line in rendered.lines() {
        lines.push(Line::from(Span::styled(
            format!("   {line}"),
            Style::default().fg(Color::Green),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Press ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "[Y / Enter]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to confirm save, or ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "[N / Esc]",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to back to form.", Style::default().fg(Color::DarkGray)),
    ]));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Save Profile Confirmation "),
    );
    frame.render_widget(widget, area);
}

fn draw_confirm_delete(frame: &mut Frame, app: &App, area: Rect) {
    let name = app
        .profile_names
        .get(app.selected)
        .cloned()
        .unwrap_or_default();

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " ⚠️  Are you sure you want to delete profile ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("'{name}'"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "?",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "    This operation will remove the profile from configuration and cannot be undone.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    Press ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "[Y]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " to confirm delete, or ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                "[N / Esc]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to cancel.", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Red))
            .title(" Confirm Delete "),
    );
    frame.render_widget(widget, area);
}

fn draw_client_picker(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .picker_items
        .iter()
        .map(|item| {
            let checkbox = if item.selected { "[✓]" } else { "[ ]" };
            let (kind_tag, kind_color) = if item.mergeable {
                ("[AUTO-MERGE]", Color::Green)
            } else {
                ("[COPY-SNIPPET]", Color::Yellow)
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{checkbox} "),
                    Style::default().fg(if item.selected {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::styled(
                    format!("{:<30} ", item.display_name),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(kind_tag, Style::default().fg(kind_color)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.picker_idx));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Wire Profile into AI Clients & Coding Tools "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_diff_review(frame: &mut Frame, app: &App, area: Rect) {
    let Some(entry) = app.diffs.get(app.diff_idx) else {
        frame.render_widget(Paragraph::new("No configuration diffs to review."), area);
        return;
    };

    let diff_raw = app.current_diff_text().unwrap_or_default();

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            " Reviewing Diff ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("({} of {}): ", app.diff_idx + 1, app.diffs.len()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            entry.display_name,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" Target File: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            entry.config_path.display().to_string(),
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(""));

    for line in diff_raw.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::Green),
            )));
        } else if line.starts_with('-') && !line.starts_with("---") {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::Red),
            )));
        } else if line.starts_with("@@") {
            lines.push(Line::from(Span::styled(
                line,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
        } else if line.starts_with("diff") || line.starts_with("---") || line.starts_with("+++") {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::White),
            )));
        }
    }

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Configuration Diff Inspector "),
    );
    frame.render_widget(widget, area);
}

fn draw_summary(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();
    lines.push(Line::from(""));

    for entry in &app.summary {
        if let Some(err) = &entry.error {
            lines.push(Line::from(vec![
                Span::styled(
                    " ✖ ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}: ", entry.display_name),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(err, Style::default().fg(Color::Red)),
            ]));
        } else if entry.skipped {
            lines.push(Line::from(vec![
                Span::styled(" ➖ ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} (skipped)", entry.display_name),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        } else if let Some(path) = &entry.path {
            let backup_note = entry
                .backup
                .as_ref()
                .map(|b| format!(" (backup: {})", b.display()))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(
                    " ✔ ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}: ", entry.display_name),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("Updated {}", path.display()),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(backup_note, Style::default().fg(Color::DarkGray)),
            ]));
        } else if let Some(snippet) = &entry.snippet {
            lines.push(Line::from(vec![
                Span::styled(" 📋 ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{} (Copy snippet below):", entry.display_name),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            for line in snippet.lines() {
                lines.push(Line::from(Span::styled(
                    format!("    {line}"),
                    Style::default().fg(Color::Cyan),
                )));
            }
        }
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            " No actions performed.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(Span::styled(
        " ✨ Client integration complete! You can now use NexQL MCP in your configured AI tools.",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Integration Summary "),
    );
    frame.render_widget(widget, area);
}
