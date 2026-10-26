use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::tui::app::{App, Tab};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Top pane: plan list.
    let header = "title | worktree | declared X/Y | observed X/Y | stalled";
    let mut items: Vec<ListItem> = vec![ListItem::new(Line::from(Span::styled(
        header.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )))];

    let rows = app.filtered_rows_for(Tab::Plans.index());
    let selected = app.selected_row();
    for r in rows.iter() {
        let style = if selected.map(|s| std::ptr::eq(s, *r)).unwrap_or(false) {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default()
        };
        let text = r.cells.join(" | ");
        items.push(ListItem::new(Line::from(Span::styled(text, style))));
    }
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Plans"));
    frame.render_widget(list, chunks[0]);

    // Bottom pane: detail (tasks + evidence) for the selected plan.
    let detail_text = if let Some(d) = &app.detail {
        d.lines.join("\n")
    } else {
        "select a plan and press Enter for tasks/evidence".to_string()
    };
    let para = Paragraph::new(detail_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Tasks / Evidence"),
    );
    frame.render_widget(para, chunks[1]);
}
