//! CID CLI/TUI shell (Phase 4, Part A).
//!
//! A thin terminal client over Core's existing JSON-RPC API (Part 15) —
//! Sessions, chat, tool-call approvals, and diffs from a shell, for the
//! CLI-first developer persona Phases 0–3 didn't serve. No new Core
//! functionality: every capability here already exists behind `/api/rpc` and
//! `/ws`, used the same way the desktop and web shells use it.

mod api;
mod app;
mod events;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use api::CoreClient;
use app::{App, Focus};

const REFRESH_INTERVAL: Duration = Duration::from_millis(1500);

struct Args {
    host: String,
    port: u16,
    token: Option<String>,
}

fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().collect();
    let mut args = Args {
        host: "127.0.0.1".to_string(),
        port: 5919,
        token: std::env::var("CID_AUTH_TOKEN").ok(),
    };
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--host" => {
                if let Some(v) = argv.get(i + 1) {
                    args.host = v.clone();
                    i += 1;
                }
            }
            "--port" | "-p" => {
                if let Some(v) = argv.get(i + 1) {
                    args.port = v.parse().unwrap_or(5919);
                    i += 1;
                }
            }
            "--token" => {
                if let Some(v) = argv.get(i + 1) {
                    args.token = Some(v.clone());
                    i += 1;
                }
            }
            "--help" | "-h" => {
                println!("cid-tui — terminal client for CID Core\n");
                println!("Usage: cid-tui [--host ADDR] [--port PORT] [--token TOKEN]");
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }
    args
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args();
    let client = CoreClient::new(&args.host, args.port, args.token.clone());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, client, args).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: CoreClient,
    args: Args,
) -> Result<()> {
    let mut app = App::new(client);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    tokio::spawn(events::listen(
        args.host.clone(),
        args.port,
        args.token.clone(),
        event_tx,
    ));

    app.refresh().await;
    let mut last_refresh = tokio::time::Instant::now();

    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        if app.should_quit {
            return Ok(());
        }

        while let Ok(event) = event_rx.try_recv() {
            app.apply_event(event);
        }

        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            app.refresh().await;
            last_refresh = tokio::time::Instant::now();
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(&mut app, key.code).await;
                }
            }
        }
    }
}

async fn handle_key(app: &mut App, code: KeyCode) {
    if app.focus == Focus::Composer {
        match code {
            KeyCode::Esc => app.focus = Focus::Thread,
            KeyCode::Enter => app.send_message().await,
            KeyCode::Backspace => {
                app.composer.pop();
            }
            KeyCode::Char(c) => app.composer.push(c),
            _ => {}
        }
        return;
    }

    if app.focus == Focus::Diff {
        match code {
            KeyCode::Esc | KeyCode::Char('v') => app.focus = Focus::Thread,
            KeyCode::Char('j') | KeyCode::Down => app.select_next_diff_file(),
            KeyCode::Char('k') | KeyCode::Up => app.select_prev_diff_file(),
            KeyCode::Char('r') => app.refresh_diff().await,
            KeyCode::Char('q') => app.should_quit = true,
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::SessionList => Focus::Thread,
                Focus::Thread => Focus::Composer,
                Focus::Composer => Focus::SessionList,
                Focus::Diff => Focus::Thread,
            }
        }
        KeyCode::Char('i') => app.focus = Focus::Composer,
        // Diff view replaces the session-list/thread body — available from
        // either of those panes, not from the composer (see the early
        // return above) so typing "v" into a message is never intercepted.
        KeyCode::Char('v') if app.focus == Focus::SessionList || app.focus == Focus::Thread => {
            app.focus = Focus::Diff;
            app.refresh_diff().await;
        }
        KeyCode::Char('j') | KeyCode::Down => match app.focus {
            Focus::SessionList => {
                app.select_next_session();
                app.refresh().await;
            }
            Focus::Thread if !app.pending_approvals.is_empty() => {
                app.selected_approval_index =
                    (app.selected_approval_index + 1) % app.pending_approvals.len();
            }
            _ => {}
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focus {
            Focus::SessionList => {
                app.select_prev_session();
                app.refresh().await;
            }
            Focus::Thread if !app.pending_approvals.is_empty() => {
                let n = app.pending_approvals.len();
                app.selected_approval_index = (app.selected_approval_index + n - 1) % n;
            }
            _ => {}
        },
        KeyCode::Enter if app.focus == Focus::SessionList => {
            app.focus = Focus::Thread;
        }
        KeyCode::Char('a') if app.focus == Focus::Thread => app.approve_selected(true).await,
        KeyCode::Char('d') if app.focus == Focus::Thread => app.approve_selected(false).await,
        _ => {}
    }
}
