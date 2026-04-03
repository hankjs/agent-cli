//! Main Application - GUI Application State and Logic

use eframe::Frame;
use egui::{CentralPanel, Context, SidePanel, TopBottomPanel};

use super::{chat::ChatPanel, sidebar::{Sidebar, Tab}, settings::SettingsPanel, Theme};

/// Main application state
pub struct ClaudeCodeApp {
    theme: Theme,
    sidebar: Sidebar,
    chat_panel: ChatPanel,
    settings_panel: SettingsPanel,
    show_settings: bool,
    status_message: Option<String>,
    status_timer: Option<std::time::Instant>,
}

impl Default for ClaudeCodeApp {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            sidebar: Sidebar::default(),
            chat_panel: ChatPanel::default(),
            settings_panel: SettingsPanel::default(),
            show_settings: false,
            status_message: None,
            status_timer: None,
        }
    }
}

impl ClaudeCodeApp {
    /// Create a new application instance
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load previous app state if available
        let mut app = Self::default();
        
        // Apply theme
        app.theme.apply(&cc.egui_ctx);
        
        // Load custom fonts if needed
        Self::configure_fonts(&cc.egui_ctx);

        app.show_status("Ready");
        
        app
    }

    /// Configure custom fonts
    fn configure_fonts(ctx: &Context) {
        let fonts = egui::FontDefinitions::default();
        
        // Add custom fonts here if needed
        // fonts.font_data.insert("my_font".to_owned(), ...);
        
        ctx.set_fonts(fonts);
    }

    /// Show a status message
    fn show_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
        self.status_timer = Some(std::time::Instant::now());
    }

    /// Update status timer
    fn update_status(&mut self) {
        if let Some(timer) = self.status_timer {
            if timer.elapsed().as_secs() > 3 {
                self.status_message = None;
                self.status_timer = None;
            }
        }
    }
}

impl eframe::App for ClaudeCodeApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        // Apply theme
        self.theme.apply(ctx);
        
        // Update status
        self.update_status();

        // Top panel - Title bar
        TopBottomPanel::top("top_panel")
            .exact_height(48.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Title
                    ui.heading(
                        egui::RichText::new("Claude Code")
                            .color(self.theme.primary_color())
                            .size(20.0)
                    );
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Window controls
                        if ui.button("➖").clicked() {
                            // Minimize window
                        }
                        if ui.button("⬜").clicked() {
                            // Maximize/restore window
                        }
                        if ui.button("✕").clicked() {
                            // Close window
                        }
                        
                        ui.add_space(8.0);
                        
                        // Settings button
                        let settings_text = if self.show_settings { "✓ ⚙️" } else { "⚙️" };
                        if ui.button(settings_text).clicked() {
                            self.show_settings = !self.show_settings;
                        }
                        
                        ui.add_space(8.0);
                        
                        // Theme toggle
                        let theme_icon = match self.theme {
                            Theme::Light => "☀️",
                            Theme::Dark => "🌙",
                            Theme::System => "💻",
                        };
                        if ui.button(theme_icon).clicked() {
                            self.theme = match self.theme {
                                Theme::Light => Theme::Dark,
                                Theme::Dark => Theme::System,
                                Theme::System => Theme::Light,
                            };
                            self.settings_panel.set_theme(self.theme);
                        }
                    });
                });
            });

        // Main content area
        if self.show_settings {
            // Show settings panel
            CentralPanel::default().show(ctx, |ui| {
                self.settings_panel.ui(ui, &self.theme);
            });
        } else {
            // Show main chat interface
            SidePanel::left("sidebar_panel")
                .resizable(true)
                .default_width(260.0)
                .min_width(200.0)
                .max_width(400.0)
                .show(ctx, |ui| {
                    self.sidebar.ui(ui, &self.theme);
                });

            CentralPanel::default().show(ctx, |ui| {
                match self.sidebar.selected_tab() {
                    Tab::Chat => {
                        self.chat_panel.ui(ui, &self.theme);
                    }
                    Tab::Settings => {
                        self.settings_panel.ui(ui, &self.theme);
                    }
                    _ => {
                        // Other tabs - show placeholder
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() / 2.0 - 50.0);
                            ui.heading(
                                egui::RichText::new("Coming Soon")
                                    .color(self.theme.muted_text_color())
                                    .size(24.0)
                            );
                            ui.label(
                                egui::RichText::new("This feature is under development")
                                    .color(self.theme.muted_text_color())
                            );
                        });
                    }
                }
            });
        }

        // Bottom panel - Status bar
        TopBottomPanel::bottom("bottom_panel")
            .exact_height(28.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Status message
                    if let Some(ref message) = self.status_message {
                        ui.label(
                            egui::RichText::new(message)
                                .color(self.theme.info_color())
                                .size(11.0)
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Ready")
                                .color(self.theme.muted_text_color())
                                .size(11.0)
                        );
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Version info
                        ui.label(
                            egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .color(self.theme.muted_text_color())
                                .size(11.0)
                        );
                        
                        ui.add_space(16.0);
                        
                        // Connection status
                        ui.label(
                            egui::RichText::new("● Connected")
                                .color(self.theme.success_color())
                                .size(11.0)
                        );
                    });
                });
            });
    }

    fn on_exit(&mut self, _ctx: Option<&eframe::glow::Context>) {
        // Save app state before exit
        // In a real implementation, save to disk
    }
}

/// Run the GUI application
pub fn run_gui() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Claude Code",
        options,
        Box::new(|cc| Ok(Box::new(ClaudeCodeApp::new(cc)))),
    )
}
