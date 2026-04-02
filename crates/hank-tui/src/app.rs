use crossterm::event::{self, KeyCode, KeyModifiers};
use hank_core::permission::PermissionResponse;
use hank_core::query::{QueryEvent, SpinnerMode};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tokio::sync::{mpsc, oneshot};
use tui_textarea::TextArea;

pub enum AppEvent {
    Tick,
    Key(event::KeyEvent),
    Query(QueryEvent),
}

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
}

impl<'a> App<'a> {
    pub fn new(model_name: String) -> Self {
        let mut input = TextArea::default();
        input.set_block(Block::default().borders(Borders::ALL).title(" Input "));
        Self {
            running: true, messages_text: String::new(), scroll_offset: 0,
            auto_scroll: true, input, is_streaming: false,
            spinner_label: String::new(), spinner_tick: 0,
            show_permission: false, permission_info: String::new(),
            permission_tx: None, model_name, msg_count: 0,
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3), Constraint::Length(1)])
            .split(frame.area());

        // Message display
        let para = Paragraph::new(self.messages_text.as_str())
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));
        frame.render_widget(para, chunks[0]);

        // Input
        frame.render_widget(&self.input, chunks[1]);

        // Status bar
        let status = if self.is_streaming {
            let dots = ".".repeat((self.spinner_tick % 3) + 1);
            format!(" {}{}", self.spinner_label, dots)
        } else {
            format!(" hank | {} messages | {}", self.msg_count, self.model_name)
        };
        let bar = Paragraph::new(status).style(Style::default().bg(Color::DarkGray));
        frame.render_widget(bar, chunks[2]);

        // Permission popup
        if self.show_permission {
            let area = centered_rect(50, 30, frame.area());
            frame.render_widget(Clear, area);
            let popup = Paragraph::new(format!(
                "{}\n\n[Y] Allow  [N] Deny  [A] Always Allow", self.permission_info
            )).block(Block::default().borders(Borders::ALL).title(" Permission "));
            frame.render_widget(popup, area);
        }
    }

    pub fn handle_key(&mut self, key: event::KeyEvent, engine_tx: &mpsc::Sender<String>) {
        if self.show_permission {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.respond_permission(PermissionResponse::Allow),
                KeyCode::Char('n') | KeyCode::Char('N') => self.respond_permission(PermissionResponse::Deny),
                KeyCode::Char('a') | KeyCode::Char('A') => self.respond_permission(PermissionResponse::AlwaysAllow("*".into())),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let text: String = self.input.lines().join("\n");
                if !text.trim().is_empty() {
                    self.messages_text.push_str(&format!("\n> {text}\n"));
                    self.msg_count += 1;
                    let _ = engine_tx.try_send(text);
                    self.input = TextArea::default();
                    self.input.set_block(Block::default().borders(Borders::ALL).title(" Input "));
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.is_streaming {
                    self.is_streaming = false;
                } else {
                    self.running = false;
                }
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
            _ => { self.input.input(key); }
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
            }
            QueryEvent::ToolStart { name, .. } => {
                self.messages_text.push_str(&format!("\n[tool: {name}]\n"));
            }
            QueryEvent::ToolComplete { output, .. } => {
                self.messages_text.push_str(&format!("{output}\n"));
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
        }
    }

    fn respond_permission(&mut self, response: PermissionResponse) {
        if let Some(tx) = self.permission_tx.take() {
            let _ = tx.send(response);
        }
        self.show_permission = false;
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll_offset();
    }

    fn max_scroll_offset(&self) -> u16 {
        let lines = self.messages_text.lines().count() as u16;
        lines.saturating_sub(10)
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
        let mut app = App::new("test-model".into());
        let text = (1..=15)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        app.handle_query_event(QueryEvent::TextDelta(text));

        assert_eq!(app.scroll_offset, 5);
        assert!(app.auto_scroll);
    }

    #[test]
    fn page_up_disables_auto_scroll_while_streaming() {
        let mut app = app_with_lines(30);
        let (tx, _rx) = mpsc::channel(1);

        app.handle_query_event(QueryEvent::Spinner(SpinnerMode::Responding));
        app.handle_key(key(KeyCode::PageUp), &tx);

        assert_eq!(app.scroll_offset, 10);
        assert!(!app.auto_scroll);

        app.handle_query_event(QueryEvent::TextDelta("\nnew line".into()));

        assert_eq!(app.scroll_offset, 10);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn page_down_only_reenables_auto_scroll_at_bottom() {
        let mut app = app_with_lines(35);
        let (tx, _rx) = mpsc::channel(1);

        app.handle_key(key(KeyCode::PageUp), &tx);
        app.handle_key(key(KeyCode::PageUp), &tx);
        assert_eq!(app.scroll_offset, 5);
        assert!(!app.auto_scroll);

        app.handle_key(key(KeyCode::PageDown), &tx);
        assert_eq!(app.scroll_offset, 15);
        assert!(!app.auto_scroll);

        app.handle_key(key(KeyCode::PageDown), &tx);
        assert_eq!(app.scroll_offset, 25);
        assert!(app.auto_scroll);
    }

    #[test]
    fn returning_to_bottom_resumes_following_new_text() {
        let mut app = app_with_lines(20);
        let (tx, _rx) = mpsc::channel(1);

        app.handle_key(key(KeyCode::PageUp), &tx);
        assert!(!app.auto_scroll);

        app.handle_key(key(KeyCode::PageDown), &tx);
        assert!(app.auto_scroll);

        app.handle_query_event(QueryEvent::TextDelta("\nline 21\nline 22".into()));

        assert_eq!(app.scroll_offset, 12);
    }
}
