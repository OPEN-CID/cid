//! Rendering — a three-pane layout echoing the desktop/web shells' shape
//! (Part 19) at terminal scale: Mission list, thread, composer, plus a
//! pending-approvals strip when one exists.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Focus};

pub fn draw(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(frame.area());

    if app.focus == Focus::Diff {
        draw_diff(frame, app, root[0]);
    } else {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(36), Constraint::Min(20)])
            .split(root[0]);

        draw_mission_list(frame, app, body[0]);
        draw_thread(frame, app, body[1]);
    }
    draw_status_bar(frame, app, root[1]);
}

fn draw_mission_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .missions
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let repo_name = app
                .repos
                .iter()
                .find(|r| r.id == m.repo_channel_id)
                .map(|r| r.name.as_str())
                .unwrap_or("?");
            let marker = if m.status == "blocked_on_approval" {
                "● "
            } else {
                "  "
            };
            let style = if i == app.selected_mission_index {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if m.status == "blocked_on_approval" {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::styled(format!("{repo_name}/{}", m.title), style),
            ]))
            .style(style)
        })
        .collect();

    let border_style = if app.focus == Focus::MissionList {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let list = List::new(items).block(
        Block::default()
            .title(" Missions (j/k, Enter) ")
            .borders(Borders::ALL)
            .border_style(border_style),
    );
    frame.render_widget(list, area);
}

fn draw_thread(frame: &mut Frame, app: &App, area: Rect) {
    let has_approvals = !app.pending_approvals.is_empty();
    let constraints = if has_approvals {
        vec![
            Constraint::Min(5),
            Constraint::Length(6),
            Constraint::Length(3),
        ]
    } else {
        vec![Constraint::Min(5), Constraint::Length(3)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let title = match app.selected_mission() {
        Some(m) => format!(" {} — {} ({}) ", m.title, m.status, m.autonomy_level),
        None => " No Mission selected ".to_string(),
    };

    let lines: Vec<Line> = app
        .messages
        .iter()
        .map(|m| {
            let color = match m.role.as_str() {
                "user" => Color::Green,
                "system" => Color::Red,
                _ => Color::White,
            };
            Line::from(vec![
                Span::styled(
                    format!("{:>9}: ", m.role),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(m.content.clone()),
            ])
        })
        .collect();

    let border_style = if app.focus == Focus::Thread {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let thread = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style),
    );
    frame.render_widget(thread, chunks[0]);

    let mut next = 1;
    if has_approvals {
        draw_approvals(frame, app, chunks[1]);
        next = 2;
    }

    draw_composer(frame, app, chunks[next]);
}

fn draw_approvals(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .pending_approvals
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let style = if i == app.selected_approval_index {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::Yellow)
            };
            ListItem::new(format!("{}  {}", a.tool_name, a.arguments)).style(style)
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .title(" Pending approval — a: approve, d: deny ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(list, area);
}

fn draw_composer(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Composer {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let composer = Paragraph::new(app.composer.as_str()).block(
        Block::default()
            .title(" Composer (i to type, Enter to send, Esc to leave) ")
            .borders(Borders::ALL)
            .border_style(border_style),
    );
    frame.render_widget(composer, area);
}

/// review_prompt.md / Gemini-checklist follow-up: `cid-tui` had chat, tool
/// approvals, and Mission threads, per its own module doc's claim to handle
/// "diffs from a shell" — but no diff rendering existed anywhere in the
/// crate. Read-only for now: hunk accept/reject stay web/desktop-only, since
/// this view's job is letting you actually *see* what an Autonomous or
/// unattended Mission changed from a terminal, which was the real gap.
fn draw_diff(frame: &mut Frame, app: &App, area: Rect) {
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(36), Constraint::Min(20)])
        .split(area);

    let file_items: Vec<ListItem> = app
        .diff_files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let style = if i == app.selected_diff_file_index {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", f.status),
                    Style::default().fg(Color::Magenta),
                ),
                Span::raw(f.path.clone()),
                Span::styled(
                    format!(" +{}", f.additions),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!(" -{}", f.deletions),
                    Style::default().fg(Color::Red),
                ),
            ]))
            .style(style)
        })
        .collect();

    let files_title = format!(
        " Changed files ({}) — j/k, r: refresh ",
        app.diff_files.len()
    );
    let files_list = List::new(file_items).block(
        Block::default()
            .title(files_title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(files_list, body[0]);

    let mut lines: Vec<Line> = Vec::new();
    if let Some(file) = app.diff_files.get(app.selected_diff_file_index) {
        for hunk in &file.hunks {
            lines.push(Line::from(Span::styled(
                hunk.header.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in hunk.content.split('\n') {
                let style = if line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Gray)
                };
                lines.push(Line::from(Span::styled(line.to_string(), style)));
            }
            lines.push(Line::from(""));
        }
        if file.hunks.is_empty() {
            lines.push(Line::from("(no hunks)"));
        }
    } else if app.diff_files.is_empty() {
        lines.push(Line::from(
            "No changes detected — clean working tree, or no Mission selected.",
        ));
    }

    let title = match app.diff_files.get(app.selected_diff_file_index) {
        Some(f) => format!(" {} — Esc/v: back ", f.path),
        None => " Diff — Esc/v: back ".to_string(),
    };
    let diff_view = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(diff_view, body[1]);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let dot = if app.connected { "●" } else { "○" };
    let color = if app.connected {
        Color::Green
    } else {
        Color::Red
    };
    let mut spans = vec![
        Span::styled(dot, Style::default().fg(color)),
        Span::raw(format!(" {}", app.status_line)),
    ];
    if let Some(err) = &app.last_error {
        spans.push(Span::styled(
            format!("  |  {err}"),
            Style::default().fg(Color::Red),
        ));
    }
    spans.push(Span::raw("  |  Tab: switch pane  v: diff  q: quit"));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
