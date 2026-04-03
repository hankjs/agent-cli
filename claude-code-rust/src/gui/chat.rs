//! Chat UI Component - Chat interface for the GUI

use egui::{Color32, RichText, ScrollArea, TextEdit, Ui, Vec2};
use chrono::{DateTime, Utc};

/// A chat message
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub is_streaming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl ChatMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role,
            content: content.into(),
            timestamp: Utc::now(),
            is_streaming: false,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageRole::System, content)
    }
}

/// Chat panel state
pub struct ChatPanel {
    pub messages: Vec<ChatMessage>,
    pub input_text: String,
    pub is_loading: bool,
    pub scroll_to_bottom: bool,
}

impl Default for ChatPanel {
    fn default() -> Self {
        Self {
            messages: vec![ChatMessage::system(
                "Welcome to Claude Code! How can I help you today?"
            )],
            input_text: String::new(),
            is_loading: false,
            scroll_to_bottom: true,
        }
    }
}

impl ChatPanel {
    /// Render the chat panel
    pub fn ui(&mut self, ui: &mut Ui, theme: &super::Theme) {
        let available_height = ui.available_height();
        
        // Chat messages area
        let messages_height = available_height - 80.0;
        
        egui::Frame::none()
            .fill(theme.background_color())
            .show(ui, |ui| {
                ui.set_min_height(messages_height);
                
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(self.scroll_to_bottom)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            for message in &self.messages {
                                self.render_message(ui, message, theme);
                            }
                            
                            if self.is_loading {
                                ui.horizontal(|ui| {
                                    ui.add_space(16.0);
                                    ui.spinner();
                                    ui.label(RichText::new("Thinking...")
                                        .color(theme.muted_text_color())
                                        .italics());
                                });
                            }
                        });
                    });
            });

        ui.add_space(8.0);

        // Input area
        egui::Frame::none()
            .fill(theme.surface_color())
            .inner_margin(12.0)
            .rounding(8.0)
            .stroke(egui::Stroke::new(1.0, theme.border_color()))
            .show(ui, |ui| {
                self.render_input(ui, theme);
            });
    }

    fn render_message(&self, ui: &mut Ui, message: &ChatMessage, theme: &super::Theme) {
        let (bg_color, text_color) = match message.role {
            MessageRole::User => (theme.primary_color(), Color32::WHITE),
            MessageRole::Assistant => (theme.surface_color(), theme.text_color()),
            MessageRole::System => (theme.background_color(), theme.muted_text_color()),
        };

        let is_user = message.role == MessageRole::User;

        ui.horizontal(|ui| {
            if is_user {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    self.render_message_bubble(ui, message, bg_color, text_color, theme, is_user);
                });
            } else {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                    self.render_message_bubble(ui, message, bg_color, text_color, theme, is_user);
                });
            }
        });

        ui.add_space(8.0);
    }

    fn render_message_bubble(
        &self,
        ui: &mut Ui,
        message: &ChatMessage,
        bg_color: Color32,
        text_color: Color32,
        theme: &super::Theme,
        is_user: bool,
    ) {
        let max_width = ui.available_width() * 0.8;

        // Avatar/Icon
        if !is_user {
            ui.vertical(|ui| {
                ui.set_width(32.0);
                ui.set_height(32.0);
                ui.label(RichText::new("🤖").size(20.0));
            });
            ui.add_space(8.0);
        }

        // Message bubble
        egui::Frame::none()
            .fill(bg_color)
            .inner_margin(12.0)
            .rounding(12.0)
            .show(ui, |ui| {
                ui.set_max_width(max_width);

                // Role label
                let role_text = match message.role {
                    MessageRole::User => "You",
                    MessageRole::Assistant => "Claude",
                    MessageRole::System => "System",
                };
                
                if message.role != MessageRole::User {
                    ui.label(RichText::new(role_text)
                        .strong()
                        .color(theme.primary_color())
                        .size(12.0));
                    ui.add_space(4.0);
                }

                // Message content
                let content = if message.is_streaming {
                    format!("{}▌", message.content)
                } else {
                    message.content.clone()
                };

                // Parse and render markdown-like content
                self.render_formatted_text(ui, &content, text_color);

                // Timestamp
                ui.add_space(4.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::BOTTOM), |ui| {
                    let time_str = message.timestamp.format("%H:%M").to_string();
                    ui.label(RichText::new(time_str)
                        .color(theme.muted_text_color())
                        .size(10.0));
                });
            });

        if is_user {
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(32.0);
                ui.set_height(32.0);
                ui.label(RichText::new("👤").size(20.0));
            });
        }
    }

    fn render_formatted_text(&self, ui: &mut Ui, text: &str, color: Color32) {
        // Simple formatting - in a real implementation, use a proper markdown parser
        let mut in_code_block = false;
        let mut code_buffer = String::new();

        for line in text.lines() {
            if line.starts_with("```") {
                if in_code_block {
                    // End code block
                    egui::Frame::none()
                        .fill(Color32::from_rgb(40, 40, 40))
                        .inner_margin(8.0)
                        .rounding(4.0)
                        .show(ui, |ui| {
                            ui.monospace(RichText::new(&code_buffer).color(Color32::from_rgb(200, 200, 200)));
                        });
                    code_buffer.clear();
                    in_code_block = false;
                } else {
                    in_code_block = true;
                }
                continue;
            }

            if in_code_block {
                code_buffer.push_str(line);
                code_buffer.push('\n');
            } else {
                // Regular text with inline formatting
                if line.starts_with("# ") {
                    ui.label(RichText::new(&line[2..]).heading().color(color));
                } else if line.starts_with("## ") {
                    ui.label(RichText::new(&line[3..]).heading().color(color).size(18.0));
                } else if line.starts_with("- ") {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("•").color(color));
                        ui.label(RichText::new(&line[2..]).color(color));
                    });
                } else if line.starts_with("**") && line.ends_with("**") {
                    let content = &line[2..line.len()-2];
                    ui.label(RichText::new(content).strong().color(color));
                } else if !line.is_empty() {
                    ui.label(RichText::new(line).color(color));
                }
            }
        }

        // Handle unclosed code block
        if in_code_block && !code_buffer.is_empty() {
            egui::Frame::none()
                .fill(Color32::from_rgb(40, 40, 40))
                .inner_margin(8.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.monospace(RichText::new(&code_buffer).color(Color32::from_rgb(200, 200, 200)));
                });
        }
    }

    fn render_input(&mut self, ui: &mut Ui, theme: &super::Theme) {
        ui.horizontal(|ui| {
            // Text input
            let text_edit = TextEdit::multiline(&mut self.input_text)
                .hint_text("Type your message...")
                .desired_width(ui.available_width() - 100.0)
                .min_size(Vec2::new(0.0, 40.0))
                .margin(egui::vec2(8.0, 8.0));

            let response = ui.add(text_edit);

            // Send button
            ui.add_space(8.0);
            
            let button_enabled = !self.input_text.trim().is_empty() && !self.is_loading;
            let button_text = if self.is_loading { "⏳" } else { "Send" };
            
            let button = egui::Button::new(
                RichText::new(button_text)
                    .strong()
                    .color(if button_enabled { Color32::WHITE } else { theme.muted_text_color() })
            )
            .fill(if button_enabled { theme.primary_color() } else { theme.surface_color() })
            .min_size(Vec2::new(80.0, 40.0))
            .rounding(8.0);

            let button_response = ui.add_enabled(button_enabled, button);

            // Handle send action
            let enter_pressed = response.lost_focus() 
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);

            if (button_response.clicked() || enter_pressed) && button_enabled {
                self.send_message();
            }
        });
    }

    fn send_message(&mut self) {
        let content = self.input_text.trim().to_string();
        if content.is_empty() {
            return;
        }

        // Add user message
        self.messages.push(ChatMessage::user(&content));
        self.input_text.clear();
        self.is_loading = true;
        self.scroll_to_bottom = true;

        // In a real implementation, this would trigger an async API call
        // For now, we'll add a placeholder response
        self.messages.push(ChatMessage::assistant(
            "I'm processing your request. In the full implementation, this would connect to the AI API and stream the response."
        ));
        self.is_loading = false;
    }

    /// Add a message to the chat
    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.scroll_to_bottom = true;
    }

    /// Clear all messages
    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.messages.push(ChatMessage::system(
            "Welcome to Claude Code! How can I help you today?"
        ));
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.is_loading = loading;
    }

    /// Update the last message (for streaming)
    pub fn update_last_message(&mut self, content: impl Into<String>) {
        if let Some(last) = self.messages.last_mut() {
            last.content = content.into();
        }
    }
}
