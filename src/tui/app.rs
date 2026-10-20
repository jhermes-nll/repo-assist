//! Pure TUI state — no I/O, unit-testable.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Worktrees,
    Plans,
    Findings,
    Sessions,
}

impl Tab {
    pub fn index(self) -> usize {
        match self {
            Tab::Worktrees => 0,
            Tab::Plans => 1,
            Tab::Findings => 2,
            Tab::Sessions => 3,
        }
    }
    pub fn from_index(i: usize) -> Self {
        match i % 4 {
            0 => Tab::Worktrees,
            1 => Tab::Plans,
            2 => Tab::Findings,
            _ => Tab::Sessions,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Tab::Worktrees => "Worktrees",
            Tab::Plans => "Plans",
            Tab::Findings => "Findings",
            Tab::Sessions => "Sessions",
        }
    }
}

/// A row displayed in a tab.
#[derive(Debug, Clone, Default)]
pub struct Row {
    pub id: String,
    pub text: String,
    pub cells: Vec<String>,
}

impl Row {
    pub fn new(text: impl Into<String>) -> Self {
        let t = text.into();
        Self {
            id: t.clone(),
            text: t.clone(),
            cells: vec![t],
        }
    }

    pub fn with_id(id: impl Into<String>, text: impl Into<String>) -> Self {
        let i = id.into();
        let t = text.into();
        Self {
            id: i,
            text: t.clone(),
            cells: vec![t],
        }
    }
}

impl From<String> for Row {
    fn from(s: String) -> Self {
        Row::new(s)
    }
}
impl From<&str> for Row {
    fn from(s: &str) -> Self {
        Row::new(s)
    }
}

/// Detail pane content.
#[derive(Debug, Clone, Default)]
pub struct Detail {
    pub title: String,
    pub lines: Vec<String>,
}

/// Count summary for the header.
#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub worktrees: usize,
    pub plans: usize,
    pub findings: usize,
    pub sessions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    NextTab,
    PrevTab,
    SelectTab(Tab),
    Down,
    Up,
    Enter,
    Ack,
    Filter(char),
    Backspace,
    ClearFilter,
    Refresh,
}

#[derive(Debug, Clone, Default)]
pub struct App {
    pub tab: Tab,
    pub cursor: [usize; 4],
    pub filter: [String; 4],
    pub rows: [Vec<Row>; 4],
    pub detail: Option<Detail>,
    pub status_line: String,
    pub should_quit: bool,
    pub repo_key: String,
    pub daemon_version: String,
    pub counts: Counts,
    pub filtering: bool,
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_cursor(&self) -> usize {
        self.cursor[self.tab.index()]
    }

    pub fn active_filter(&self) -> &str {
        &self.filter[self.tab.index()]
    }

    pub fn filtered_rows(&self) -> Vec<&Row> {
        let idx = self.tab.index();
        let f = self.filter[idx].to_lowercase();
        self.rows[idx]
            .iter()
            .filter(|r| {
                if f.is_empty() {
                    true
                } else {
                    r.text.to_lowercase().contains(&f)
                        || r.id.to_lowercase().contains(&f)
                        || r.cells.iter().any(|c| c.to_lowercase().contains(&f))
                }
            })
            .collect()
    }

    pub fn clamp_cursor(&mut self) {
        let idx = self.tab.index();
        let n = self.filtered_rows().len();
        if n == 0 {
            self.cursor[idx] = 0;
        } else if self.cursor[idx] >= n {
            self.cursor[idx] = n - 1;
        }
    }

    pub fn apply_snapshot(&mut self, snap: &crate::tui::client::Snapshot) {
        self.repo_key = snap.repo_key.clone();
        self.daemon_version = snap.daemon_version.clone();
        self.counts = snap.counts.clone();
        self.rows = snap.rows.clone();
        for i in 0..4 {
            let n = self.rows[i].len();
            if self.cursor[i] >= n && n > 0 {
                self.cursor[i] = n - 1;
            }
            if n == 0 {
                self.cursor[i] = 0;
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Filter-input mode.
        if self.filtering {
            match key.code {
                KeyCode::Esc => {
                    self.filtering = false;
                    self.filter[self.tab.index()].clear();
                    self.clamp_cursor();
                    return Some(Action::ClearFilter);
                }
                KeyCode::Enter => {
                    self.filtering = false;
                    return Some(Action::Enter);
                }
                KeyCode::Backspace => {
                    let idx = self.tab.index();
                    self.filter[idx].pop();
                    self.clamp_cursor();
                    return Some(Action::Backspace);
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let idx = self.tab.index();
                    self.filter[idx].push(c);
                    self.clamp_cursor();
                    return Some(Action::Filter(c));
                }
                _ => return None,
            }
        }

        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                Some(Action::Quit)
            }
            KeyCode::Tab => {
                self.tab = Tab::from_index(self.tab.index() + 1);
                self.clamp_cursor();
                Some(Action::NextTab)
            }
            KeyCode::BackTab => {
                let i = if self.tab.index() == 0 {
                    3
                } else {
                    self.tab.index() - 1
                };
                self.tab = Tab::from_index(i);
                self.clamp_cursor();
                Some(Action::PrevTab)
            }
            KeyCode::Char('1') => {
                self.tab = Tab::Worktrees;
                self.clamp_cursor();
                Some(Action::SelectTab(Tab::Worktrees))
            }
            KeyCode::Char('2') => {
                self.tab = Tab::Plans;
                self.clamp_cursor();
                Some(Action::SelectTab(Tab::Plans))
            }
            KeyCode::Char('3') => {
                self.tab = Tab::Findings;
                self.clamp_cursor();
                Some(Action::SelectTab(Tab::Findings))
            }
            KeyCode::Char('4') => {
                self.tab = Tab::Sessions;
                self.clamp_cursor();
                Some(Action::SelectTab(Tab::Sessions))
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let idx = self.tab.index();
                let n = self.filtered_rows().len();
                if n > 0 && self.cursor[idx] + 1 < n {
                    self.cursor[idx] += 1;
                }
                Some(Action::Down)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let idx = self.tab.index();
                if self.cursor[idx] > 0 {
                    self.cursor[idx] -= 1;
                }
                Some(Action::Up)
            }
            KeyCode::Enter => {
                // open detail from selected row if present
                let rows = self.filtered_rows();
                let idx = self.tab.index();
                if !rows.is_empty() {
                    let r = rows[self.cursor[idx].min(rows.len() - 1)];
                    self.detail = Some(Detail {
                        title: r.text.clone(),
                        lines: r.cells.clone(),
                    });
                }
                Some(Action::Enter)
            }
            KeyCode::Esc => {
                if self.detail.is_some() {
                    self.detail = None;
                    Some(Action::ClearFilter) // reuse? spec says Esc dismisses detail
                } else {
                    self.filter[self.tab.index()].clear();
                    self.filtering = false;
                    self.clamp_cursor();
                    Some(Action::ClearFilter)
                }
            }
            KeyCode::Char('/') => {
                self.filtering = true;
                Some(Action::Filter('/'))
            }
            KeyCode::Char('a') => Some(Action::Ack),
            KeyCode::Char('r') => Some(Action::Refresh),
            _ => None,
        }
    }
}
