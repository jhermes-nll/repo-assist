pub mod app;
pub mod client;
pub mod tabs;
pub mod ui;

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{init, restore};

use app::{Action, App, Tab};
use client::DataSource;

pub fn run(sock: std::path::PathBuf) -> anyhow::Result<()> {
    let mut terminal = init();
    let res = run_inner(&mut terminal, sock);
    restore();
    res
}

fn run_inner(
    terminal: &mut ratatui::DefaultTerminal,
    sock: std::path::PathBuf,
) -> anyhow::Result<()> {
    use crossterm::event::KeyModifiers;
    let mut app = App::new();
    let mut ds = DataSource::new(sock);

    // initial fetch
    match ds.fetch_all() {
        Ok(snap) => app.apply_snapshot(&snap),
        Err(e) => app.status_line = e.to_string(),
    }

    let tick_rate = Duration::from_millis(1000);
    let mut last_tick = Instant::now();
    let mut status_until: Option<Instant> = None;

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == crossterm::event::KeyCode::Char('c')
                    {
                        app.should_quit = true;
                        continue;
                    }
                    if let Some(action) = app.on_key(key) {
                        match action {
                            Action::Quit => app.should_quit = true,
                            Action::Refresh => match ds.fetch_all() {
                                Ok(snap) => {
                                    app.apply_snapshot(&snap);
                                    app.status_line.clear();
                                    status_until = None;
                                }
                                Err(e) => {
                                    app.status_line = e.to_string();
                                    status_until = Some(Instant::now() + Duration::from_secs(2));
                                }
                            },
                            Action::Ack => {
                                if app.tab == Tab::Findings {
                                    if let Some(row) = app.selected_row() {
                                        let id = row.id.clone();
                                        match ds.ack_finding(&id) {
                                            Ok(()) => {
                                                app.status_line = format!("acked finding {id}")
                                            }
                                            Err(e) => {
                                                app.status_line = format!("nothing acked: {e}")
                                            }
                                        }
                                    } else {
                                        app.status_line = "nothing to ack".to_string();
                                    }
                                } else {
                                    app.status_line =
                                        "a acks findings — switch to the Findings tab".to_string();
                                }
                                status_until = Some(Instant::now() + Duration::from_secs(2));
                                match ds.fetch_all() {
                                    Ok(snap) => {
                                        app.apply_snapshot(&snap);
                                    }
                                    Err(e) => app.status_line = e.to_string(),
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            match ds.fetch_all() {
                Ok(snap) => {
                    app.apply_snapshot(&snap);
                    // only clear expired status lines; keep messages readable briefly
                    if status_until.map(|t| Instant::now() >= t).unwrap_or(true) {
                        app.status_line.clear();
                        status_until = None;
                    }
                }
                Err(e) => {
                    app.status_line = e.to_string();
                    status_until = Some(Instant::now() + Duration::from_secs(2));
                }
            }
            last_tick = Instant::now();
        }
    }

    Ok(())
}
