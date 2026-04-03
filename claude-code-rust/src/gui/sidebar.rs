//! Sidebar Component - Navigation sidebar for the GUI

use egui::{Color32, RichText, Ui, Vec2};

/// Sidebar state and configuration
pub struct Sidebar {
    pub selected_tab: Tab,
    pub collapsed: bool,
    pub width: f32,
    pub conversations: Vec<ConversationItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Chat,
    History,
    Plugins,
    Settings,
    Tools,
}

#[derive(Debug, Clone)]
pub struct ConversationItem {
    pub id: String,
    pub title: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            selected_tab: Tab::Chat,
            collapsed: false,
            width: 260.0,
            conversations: vec![
                ConversationItem {
                    id: "1".to_string(),
                    title: "New Conversation".to_string(),
                    timestamp: chrono::Utc::now(),
                    message_count: 0,
                },
            ],
        }
    }
}

impl Sidebar {
    /// Render the sidebar
    pub fn ui(&mut self, ui: &mut Ui, theme: &super::Theme) {
        let width = if self.collapsed { 60.0 } else { self.width };

        egui::SidePanel::left("sidebar")
            .resizable(!self.collapsed)
            .min_width(width)
            .max_width(400.0)
            .default_width(width)
            .show_inside(ui, |ui| {
                egui::Frame::none()
                    .fill(theme.surface_color())
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.set_min_height(ui.available_height());

                        // Header with collapse button
                        ui.horizontal(|ui| {
                            if !self.collapsed {
                                ui.heading(RichText::new("Claude Code")
                                    .color(theme.primary_color())
                                    .size(18.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("◀").clicked() {
                                        self.collapsed = true;
                                    }
                                });
                            } else {
                                if ui.button("▶").clicked() {
                                    self.collapsed = false;
                                }
                            }
                        });

                        ui.add_space(16.0);

                        if self.collapsed {
                            self.render_collapsed(ui, theme);
                        } else {
                            self.render_expanded(ui, theme);
                        }
                    });
            });
    }

    fn render_collapsed(&mut self, ui: &mut Ui, theme: &super::Theme) {
        ui.vertical_centered(|ui| {
            // New chat button
            if ui.button("➕").clicked() {
                self.selected_tab = Tab::Chat;
            }
            ui.add_space(8.0);

            // Tab buttons
            let tabs = vec![
                (Tab::Chat, "💬", "Chat"),
                (Tab::History, "📜", "History"),
                (Tab::Plugins, "🔌", "Plugins"),
                (Tab::Tools, "🛠️", "Tools"),
                (Tab::Settings, "⚙️", "Settings"),
            ];

            for (tab, icon, _tooltip) in tabs {
                let is_selected = self.selected_tab == tab;
                let button = egui::Button::new(RichText::new(icon).size(20.0))
                    .fill(if is_selected { theme.primary_color() } else { theme.surface_color() })
                    .min_size(Vec2::new(40.0, 40.0))
                    .rounding(8.0);

                if ui.add(button).clicked() {
                    self.selected_tab = tab;
                }
                ui.add_space(4.0);
            }
        });
    }

    fn render_expanded(&mut self, ui: &mut Ui, theme: &super::Theme) {
        // New conversation button
        let new_chat_button = egui::Button::new(
            RichText::new("➕ New Conversation")
                .strong()
                .color(Color32::WHITE)
        )
        .fill(theme.primary_color())
        .min_size(Vec2::new(ui.available_width(), 40.0))
        .rounding(8.0);

        if ui.add(new_chat_button).clicked() {
            self.create_new_conversation();
        }

        ui.add_space(16.0);

        // Tab buttons
        ui.horizontal(|ui| {
            let tabs = vec![
                (Tab::Chat, "💬 Chat"),
                (Tab::History, "📜"),
                (Tab::Plugins, "🔌"),
                (Tab::Tools, "🛠️"),
            ];

            for (tab, label) in tabs {
                let is_selected = self.selected_tab == tab;
                let button = egui::SelectableLabel::new(is_selected, label);
                if ui.add(button).clicked() {
                    self.selected_tab = tab;
                }
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Tab content
        match self.selected_tab {
            Tab::Chat => self.render_conversations_list(ui, theme),
            Tab::History => self.render_history(ui, theme),
            Tab::Plugins => self.render_plugins(ui, theme),
            Tab::Tools => self.render_tools(ui, theme),
            Tab::Settings => self.render_settings_link(ui, theme),
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        // Settings button at bottom
        let settings_button = egui::Button::new(
            RichText::new("⚙️ Settings")
                .color(theme.text_color())
        )
        .fill(theme.surface_color())
        .min_size(Vec2::new(ui.available_width(), 36.0))
        .rounding(8.0);

        if ui.add(settings_button).clicked() {
            self.selected_tab = Tab::Settings;
        }
    }

    fn render_conversations_list(&mut self, ui: &mut Ui, theme: &super::Theme) {
        ui.label(RichText::new("Conversations")
            .strong()
            .color(theme.muted_text_color())
            .size(12.0));
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for conversation in &self.conversations {
                    self.render_conversation_item(ui, conversation, theme);
                }
            });
    }

    fn render_conversation_item(&self, ui: &mut Ui, conversation: &ConversationItem, theme: &super::Theme) {
        let button = egui::Button::new(
            RichText::new(format!("💬 {}", conversation.title))
                .color(theme.text_color())
        )
        .fill(theme.background_color())
        .min_size(Vec2::new(ui.available_width(), 48.0))
        .rounding(8.0);

        ui.add(button);
        ui.add_space(4.0);
    }

    fn render_history(&mut self, ui: &mut Ui, theme: &super::Theme) {
        ui.label(RichText::new("History")
            .strong()
            .color(theme.muted_text_color())
            .size(12.0));
        ui.add_space(8.0);

        // Placeholder history items
        let history_items = vec![
            "Yesterday",
            "Last Week",
            "Last Month",
        ];

        for item in history_items {
            let button = egui::Button::new(
                RichText::new(format!("📅 {}", item))
                    .color(theme.text_color())
            )
            .fill(theme.background_color())
            .min_size(Vec2::new(ui.available_width(), 40.0))
            .rounding(8.0);

            ui.add(button);
            ui.add_space(4.0);
        }
    }

    fn render_plugins(&mut self, ui: &mut Ui, theme: &super::Theme) {
        ui.label(RichText::new("Plugins")
            .strong()
            .color(theme.muted_text_color())
            .size(12.0));
        ui.add_space(8.0);

        // Placeholder plugin items
        let plugins = vec![
            ("🔌 File System", "Enabled"),
            ("🔌 Git Integration", "Enabled"),
            ("🔌 Code Analysis", "Disabled"),
        ];

        for (name, status) in plugins {
            let color = if status == "Enabled" { 
                theme.success_color() 
            } else { 
                theme.muted_text_color() 
            };

            ui.horizontal(|ui| {
                ui.label(RichText::new(name).color(theme.text_color()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(status).color(color).size(11.0));
                });
            });
            ui.add_space(4.0);
        }

        ui.add_space(8.0);
        if ui.button("➕ Install Plugin").clicked() {
            // Open plugin marketplace
        }
    }

    fn render_tools(&mut self, ui: &mut Ui, theme: &super::Theme) {
        ui.label(RichText::new("Tools")
            .strong()
            .color(theme.muted_text_color())
            .size(12.0));
        ui.add_space(8.0);

        let tools = vec![
            "📁 File Explorer",
            "🔍 Search",
            "⚡ Execute Command",
            "📝 Code Editor",
        ];

        for tool in tools {
            let button = egui::Button::new(
                RichText::new(tool).color(theme.text_color())
            )
            .fill(theme.background_color())
            .min_size(Vec2::new(ui.available_width(), 36.0))
            .rounding(8.0);

            ui.add(button);
            ui.add_space(4.0);
        }
    }

    fn render_settings_link(&mut self, ui: &mut Ui, theme: &super::Theme) {
        ui.label(RichText::new("Quick Settings")
            .strong()
            .color(theme.muted_text_color())
            .size(12.0));
        ui.add_space(8.0);

        let settings = vec![
            "🔑 API Configuration",
            "🎨 Appearance",
            "🔔 Notifications",
            "💾 Data & Storage",
        ];

        for setting in settings {
            let button = egui::Button::new(
                RichText::new(setting).color(theme.text_color())
            )
            .fill(theme.background_color())
            .min_size(Vec2::new(ui.available_width(), 36.0))
            .rounding(8.0);

            ui.add(button);
            ui.add_space(4.0);
        }
    }

    fn create_new_conversation(&mut self) {
        let new_conversation = ConversationItem {
            id: uuid::Uuid::new_v4().to_string(),
            title: format!("Conversation {}", self.conversations.len() + 1),
            timestamp: chrono::Utc::now(),
            message_count: 0,
        };
        self.conversations.push(new_conversation);
    }

    /// Get the currently selected tab
    pub fn selected_tab(&self) -> Tab {
        self.selected_tab
    }

    /// Set the selected tab
    pub fn set_selected_tab(&mut self, tab: Tab) {
        self.selected_tab = tab;
    }

    /// Toggle sidebar collapse state
    pub fn toggle_collapse(&mut self) {
        self.collapsed = !self.collapsed;
    }
}
