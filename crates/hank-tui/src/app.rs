use crossterm::event::{self, KeyCode, KeyModifiers};
use hank_core::permission::PermissionResponse;
use hank_core::query::{EngineCommand, QueryEvent, SpinnerMode};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tokio::sync::{mpsc, oneshot};
use tui_textarea::TextArea;

pub enum AppEvent {
    Tick,
    Key(event::KeyEvent),
    Query(QueryEvent),
}

const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show available commands"),
    ("/compact", "Compress conversation context"),
];

pub struct App<'a> {
    pub running: bool,
    pub messages_text: String,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
    pub input: TextArea<'a>,
    pub is_streaming: bool,
    pub spinner_label: String,
    pub spinner_tick: usize,
    pub show_permission: bool,
    pub permission_info: String,
    pub permission_tx: Option<oneshot::Sender<PermissionResponse>>,
    pub model_name: String,
    pub msg_count: usize,
    /// Slash command completions to show above input
    pub slash_suggestions: Vec<(&'static str, &'static str)>,
    /// Abort signal sender for cancelling engine streaming
    pub abort_tx: Option<tokio::sync::watch::Sender<bool>>,
}

impl<'a> App<'a> {
    pub fn new(model_name: String) -> Self {
        let mut input = TextArea::default();
        input.set_block(Block::default().borders(Borders::ALL).title(" Input "));
        input.set_cursor_line_style(Style::default());
        Self {
            running: true, messages_text: String::new(), scroll_offset: 0, auto_scroll: true,
            input, is_streaming: false,
            spinner_label: String::new(), spinner_tick: 0,
            show_permission: false, permission_info: String::new(),
            permission_tx: None, model_name, msg_count: 0,
            slash_suggestions: Vec::new(),
            abort_tx: None,
        }
    }

    fn input_height(&self) -> u16 {
        let lines = self.input.lines().len() as u16;
        (lines + 2).clamp(3, 10) // +2 for border, min 3, max 10
    }

    /// Render the entire UI — ratatui handles diff rendering automatically.
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let input_h = self.input_height();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(input_h), Constraint::Length(1)])
            .split(area);

        // Message display
        Paragraph::new(self.messages_text.as_str())
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0))
            .render(chunks[0], buf);

        // Slash command suggestions (rendered above input)
        if !self.slash_suggestions.is_empty() {
            let items: Vec<Line> = self.slash_suggestions.iter().map(|(cmd, desc)| {
                Line::from(vec![
                    Span::styled(format!("  {cmd}"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(format!("  {desc}")),
                ])
            }).collect();
            let height = items.len() as u16 + 2;
            let suggestion_area = Rect {
                x: chunks[1].x,
                y: chunks[1].y.saturating_sub(height),
                width: chunks[1].width.min(50),
                height,
            };
            Clear.render(suggestion_area, buf);
            Paragraph::new(items)
                .block(Block::default().borders(Borders::ALL).title(" Commands ").border_style(Style::default().fg(Color::DarkGray)))
                .render(suggestion_area, buf);
        }

        // Input
        (&self.input).render(chunks[1], buf);

        // Status bar
        let status = if self.is_streaming {
            let phase = (self.spinner_tick / 15) % 3;
            let dots = ".".repeat(phase + 1);
            format!(" {}{}", self.spinner_label, dots)
        } else {
            format!(" hank | {} messages | {} | Shift+Enter: newline", self.msg_count, self.model_name)
        };
        Paragraph::new(status).style(Style::default().bg(Color::DarkGray))
            .render(chunks[2], buf);

        // Permission popup
        if self.show_permission {
            let popup_area = centered_rect(50, 30, area);
            Clear.render(popup_area, buf);
            Paragraph::new(format!(
                "{}\n\n[1] Allow  [2] Deny  [3] Always Allow", self.permission_info
            )).block(Block::default().borders(Borders::ALL).title(" Permission "))
                .render(popup_area, buf);
        }
    }

    pub fn handle_key(&mut self, key: event::KeyEvent, engine_tx: &mpsc::Sender<EngineCommand>) {
        if self.show_permission {
            match key.code {
                KeyCode::Char('1') => self.respond_permission(PermissionResponse::Allow),
                KeyCode::Char('2') => self.respond_permission(PermissionResponse::Deny),
                KeyCode::Char('3') => self.respond_permission(PermissionResponse::AlwaysAllow("*".into())),
                _ => {}
            }
            return;
        }
        match key.code {
            // Enter submits; Shift+Enter inserts newline
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.submit(engine_tx);
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.input.insert_newline();
                self.slash_suggestions.clear();
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.insert_newline();
                self.slash_suggestions.clear();
            }
            // Tab applies first slash suggestion
            KeyCode::Tab if !self.slash_suggestions.is_empty() => {
                let cmd = self.slash_suggestions[0].0.to_string();
                self.reset_input();
                self.input.insert_str(&cmd);
                self.slash_suggestions.clear();
            }
            // Esc interrupts streaming
            KeyCode::Esc if self.is_streaming => {
                if let Some(ref abort_tx) = self.abort_tx {
                    let _ = abort_tx.send(true);
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.is_streaming {
                    // Hard cancel — also abort engine
                    if let Some(ref abort_tx) = self.abort_tx {
                        let _ = abort_tx.send(true);
                    }
                } else {
                    self.running = false;
                }
            }
            // Scroll: Up/Down arrows (from alternate scroll mode) and PageUp/PageDown
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(3);
                self.auto_scroll = false;
            }
            KeyCode::Down => {
                let max_scroll = self.max_scroll_offset();
                self.scroll_offset = self.scroll_offset.saturating_add(3).min(max_scroll);
                self.auto_scroll = self.scroll_offset >= max_scroll;
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                self.auto_scroll = false;
            }
            KeyCode::PageDown => {
                let max_scroll = self.max_scroll_offset();
                self.scroll_offset = self.scroll_offset.saturating_add(10).min(max_scroll);
                self.auto_scroll = self.scroll_offset >= max_scroll;
            }
            _ => {
                self.input.input(key);
                self.update_slash_suggestions();
            }
        }
    }

    pub fn handle_query_event(&mut self, event: QueryEvent) {
        match event {
            QueryEvent::TextDelta(text) => {
                self.messages_text.push_str(&text);
                if self.auto_scroll { self.scroll_to_bottom(); }
            }
            QueryEvent::ThinkingDelta(text) => {
                self.messages_text.push_str(&text);
                if self.auto_scroll { self.scroll_to_bottom(); }
            }
            QueryEvent::ToolStart { name, .. } => {
                self.messages_text.push_str(&format!("\n[tool: {name}]\n"));
                if self.auto_scroll { self.scroll_to_bottom(); }
            }
            QueryEvent::ToolComplete { output, .. } => {
                self.messages_text.push_str(&format!("{output}\n"));
                if self.auto_scroll { self.scroll_to_bottom(); }
            }
            QueryEvent::Spinner(mode) => {
                self.is_streaming = true;
                self.spinner_label = match mode {
                    SpinnerMode::Requesting => "Waiting for API".into(),
                    SpinnerMode::Thinking => "Thinking".into(),
                    SpinnerMode::Responding => "Responding".into(),
                    SpinnerMode::ToolInput => "Reading tool input".into(),
                    SpinnerMode::ToolExecuting => "Running tool".into(),
                };
            }
            QueryEvent::TurnComplete => {
                self.is_streaming = false;
                self.messages_text.push('\n');
                self.msg_count += 1;
                if let Some(ref abort_tx) = self.abort_tx {
                    let _ = abort_tx.send(false);
                }
            }
            QueryEvent::Interrupted => {
                self.is_streaming = false;
                self.messages_text.push_str("\n[Interrupted]\n");
                self.scroll_to_bottom();
                if let Some(ref abort_tx) = self.abort_tx {
                    let _ = abort_tx.send(false);
                }
            }
            QueryEvent::PermissionRequest { tool_name, input, respond } => {
                self.show_permission = true;
                self.permission_info = format!("Tool: {tool_name}\nInput: {}", serde_json::to_string_pretty(&input).unwrap_or_default());
                self.permission_tx = Some(respond);
            }
            QueryEvent::Error(msg) => {
                self.messages_text.push_str(&format!("\n[error: {msg}]\n"));
                self.is_streaming = false;
            }
            QueryEvent::ModelDegraded { from, to } => {
                self.messages_text.push_str(&format!("\n[Model degraded: {from} -> {to} due to overload]\n"));
                if self.auto_scroll { self.scroll_to_bottom(); }
            }
        }
    }

    fn respond_permission(&mut self, response: PermissionResponse) {
        if let Some(tx) = self.permission_tx.take() {
            let _ = tx.send(response);
        }
        self.show_permission = false;
    }

    fn submit(&mut self, engine_tx: &mpsc::Sender<EngineCommand>) {
        let text: String = self.input.lines().join("\n");
        let trimmed = text.trim();
        if trimmed.is_empty() { return; }

        let cmd = match trimmed {
            "/help" => {
                self.messages_text.push_str("\n> /help\n");
                self.messages_text.push_str(concat!(
                    "Available commands:\n",
                    "  /help     - Show this help message\n",
                    "  /compact  - Compress conversation context\n",
                    "  Ctrl+C    - Cancel streaming or exit\n",
                    "  Shift+Enter - Insert newline\n",
                    "  PageUp/PageDown or mouse wheel - Scroll messages\n",
                    "\n",
                ));
                self.msg_count += 1;
                self.scroll_to_bottom();
                None
            }
            "/compact" => {
                self.messages_text.push_str("\n> /compact\n");
                self.scroll_to_bottom();
                Some(EngineCommand::Compact)
            }
            _ => {
                self.messages_text.push_str(&format!("\n> {text}\n"));
                self.msg_count += 1;
                Some(EngineCommand::UserMessage(text))
            }
        };

        if let Some(cmd) = cmd {
            let _ = engine_tx.try_send(cmd);
        }

        self.reset_input();
        self.slash_suggestions.clear();
    }

    fn reset_input(&mut self) {
        self.input = TextArea::default();
        self.input.set_block(Block::default().borders(Borders::ALL).title(" Input "));
        self.input.set_cursor_line_style(Style::default());
    }

    fn update_slash_suggestions(&mut self) {
        let text: String = self.input.lines().join("\n");
        let trimmed = text.trim();
        if trimmed.starts_with('/') && !trimmed.contains(' ') {
            self.slash_suggestions = SLASH_COMMANDS
                .iter()
                .filter(|(cmd, _)| cmd.starts_with(trimmed))
                .copied()
                .collect();
        } else {
            self.slash_suggestions.clear();
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll_offset();
    }

    fn max_scroll_offset(&self) -> u16 {
        let lines = self.messages_text.lines().count() as u16;
        // Terminal height minus input area and status bar (1)
        let (_, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
        let visible = term_h.saturating_sub(self.input_height() + 1);
        lines.saturating_sub(visible)
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ]).split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ]).split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> event::KeyEvent {
        event::KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Visible lines in the message area (terminal height - 4 for input+statusbar).
    /// In test env, crossterm::terminal::size() may return defaults.
    fn visible_lines() -> u16 {
        let (_, h) = crossterm::terminal::size().unwrap_or((80, 24));
        h.saturating_sub(4)
    }

    fn app_with_lines(line_count: usize) -> App<'static> {
        let mut app = App::new("test-model".into());
        app.messages_text = (1..=line_count)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.scroll_to_bottom();
        app
    }

    #[test]
    fn text_delta_auto_scrolls_to_bottom() {
        let vis = visible_lines();
        let mut app = App::new("test-model".into());
        // Generate enough lines to exceed visible area
        let line_count = (vis + 5) as usize;
        let text = (1..=line_count)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        app.handle_query_event(QueryEvent::TextDelta(text));

        assert_eq!(app.scroll_offset, (line_count as u16).saturating_sub(vis));
        assert!(app.auto_scroll);
    }

    #[test]
    fn page_up_disables_auto_scroll_while_streaming() {
        let vis = visible_lines();
        let line_count = (vis + 20) as usize;
        let mut app = app_with_lines(line_count);
        let (tx, _rx) = mpsc::channel::<EngineCommand>(1);

        let max = app.max_scroll_offset();
        app.handle_query_event(QueryEvent::Spinner(SpinnerMode::Responding));
        app.handle_key(key(KeyCode::PageUp), &tx);

        assert_eq!(app.scroll_offset, max.saturating_sub(10));
        assert!(!app.auto_scroll);

        let before = app.scroll_offset;
        app.handle_query_event(QueryEvent::TextDelta("\nnew line".into()));
        // scroll_offset unchanged because auto_scroll is off
        assert_eq!(app.scroll_offset, before);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn page_down_reenables_auto_scroll_at_bottom() {
        let vis = visible_lines();
        let line_count = (vis + 10) as usize;
        let mut app = app_with_lines(line_count);
        let (tx, _rx) = mpsc::channel::<EngineCommand>(1);

        // Scroll up, then back down to bottom
        app.handle_key(key(KeyCode::PageUp), &tx);
        assert!(!app.auto_scroll);

        app.handle_key(key(KeyCode::PageDown), &tx);
        assert!(app.auto_scroll);
    }

    #[test]
    fn returning_to_bottom_resumes_following_new_text() {
        let vis = visible_lines();
        let line_count = (vis + 10) as usize;
        let mut app = app_with_lines(line_count);
        let (tx, _rx) = mpsc::channel::<EngineCommand>(1);

        app.handle_key(key(KeyCode::PageUp), &tx);
        assert!(!app.auto_scroll);

        app.handle_key(key(KeyCode::PageDown), &tx);
        assert!(app.auto_scroll);

        app.handle_query_event(QueryEvent::TextDelta("\nextra line 1\nextra line 2".into()));

        // Should have scrolled to the new bottom
        let expected_max = app.max_scroll_offset();
        assert_eq!(app.scroll_offset, expected_max);
    }
}
