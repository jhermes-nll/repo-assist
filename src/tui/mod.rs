pub mod app;
pub mod client;
pub mod ui;

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{init, restore};

use app::{Action, App};
use client::DataSource;

pub fn run(sock: std::path::PathBuf) -> anyhow::Result<()> {
    let mut terminal = init();
    let mut app = App::new();
    let mut ds = DataSource::new(sock);

    // initial fetch
    match ds.fetch_all() {
        Ok(snap) => app.apply_snapshot(&snap),
        Err(e) => app.status_line = e.to_string(),
    }

    let tick_rate = Duration::from_millis(1000);
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let Some(action) = app.on_key(key) {
                        match action {
                            Action::Quit => app.should_quit = true,
                            Action::Refresh => match ds.fetch_all() {
                                Ok(snap) => {
                                    app.apply_snapshot(&snap);
                                    app.status_line.clear();
                                }
                                Err(e) => app.status_line = e.to_string(),
                            },
                            Action::Ack => {
                                // ack currently selected finding by id is handled via row id
                                let rows = app.filtered_rows();
                                let cur = app.active_cursor();
                                if !rows.is_empty() {
                                    let id = rows[cur.min(rows.len() - 1)].id.clone();
                                    let _ = ds.ack_finding(&id);
                                }
                                match ds.fetch_all() {
                                    Ok(snap) => app.apply_snapshot(&snap),
                                    Err(e) => app.status_line = e.to_string(),
                                }
                            }
                            Action::ClearFilter => {}
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
                    app.status_line.clear();
                }
                Err(e) => app.status_line = e.to_string(),
            }
            last_tick = Instant::now();
        }
    }

    restore();
    Ok(())
}
