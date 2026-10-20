use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs};
use ratatui::Frame;

use crate::tui::app::App;

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    // Header
    let header = format!(
        "repo: {}  daemon: {}  wt:{} plans:{} findings:{} sessions:{}",
        app.repo_key,
        app.daemon_version,
        app.counts.worktrees,
        app.counts.plans,
        app.counts.findings,
        app.counts.sessions
    );
    frame.render_widget(Paragraph::new(header), chunks[0]);

    // Tab bar
    let titles: Vec<Line> = ["Worktrees", "Plans", "Findings", "Sessions"]
        .iter()
        .map(|t| Line::from(*t))
        .collect();
    let tabs = Tabs::new(titles)
        .select(app.tab.index())
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, chunks[1]);

    // Rows
    let rows = app.filtered_rows();
    let selected = app.selected_row();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|r| {
            let style = if selected.map(|s| std::ptr::eq(s, *r)).unwrap_or(false) {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(r.text.clone(), style)))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(app.tab.title()),
    );
    frame.render_widget(list, chunks[2]);

    // Filter footer (or detail pane content)
    let mid = if let Some(d) = &app.detail {
        format!("DETAIL: {}", d.lines.join(" | "))
    } else if app.filter[app.tab.index()].is_empty() {
        String::new()
    } else {
        format!("filter: {}", app.filter[app.tab.index()])
    };
    frame.render_widget(Paragraph::new(mid), chunks[3]);

    // Status / key hints
    let bottom = if !app.status_line.is_empty() {
        app.status_line.clone()
    } else {
        "j/k move  Enter detail  a ack  Tab/1-4 tabs  / filter  r refresh  q quit".to_string()
    };
    frame.render_widget(Paragraph::new(bottom), chunks[4]);
}
