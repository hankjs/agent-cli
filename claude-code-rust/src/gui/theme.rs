//! Theme System - Customizable UI themes

use egui::{Color32, FontFamily, FontId, TextStyle, Visuals};

/// Application theme
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    System,
}

impl Theme {
    /// Apply theme to egui context
    pub fn apply(&self, ctx: &egui::Context) {
        match self {
            Theme::Light => {
                let mut visuals = Visuals::light();
                visuals.override_text_color = Some(Color32::from_rgb(33, 33, 33));
                ctx.set_visuals(visuals);
            }
            Theme::Dark => {
                let mut visuals = Visuals::dark();
                visuals.override_text_color = Some(Color32::from_rgb(230, 230, 230));
                ctx.set_visuals(visuals);
            }
            Theme::System => {
                // Use system preference (default to dark)
                ctx.set_visuals(Visuals::dark());
            }
        }

        // Configure fonts
        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(24.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(16.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(14.0, FontFamily::Monospace),
            ),
            (
                TextStyle::Button,
                FontId::new(16.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
        ]
        .into();
        ctx.set_style(style);
    }

    /// Get primary color
    pub fn primary_color(&self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(25, 118, 210),
            Theme::Dark => Color32::from_rgb(100, 181, 246),
            Theme::System => Color32::from_rgb(100, 181, 246),
        }
    }

    /// Get secondary color
    pub fn secondary_color(&self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(156, 39, 176),
            Theme::Dark => Color32::from_rgb(206, 147, 216),
            Theme::System => Color32::from_rgb(206, 147, 216),
        }
    }

    /// Get background color
    pub fn background_color(&self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(250, 250, 250),
            Theme::Dark => Color32::from_rgb(30, 30, 30),
            Theme::System => Color32::from_rgb(30, 30, 30),
        }
    }

    /// Get surface color
    pub fn surface_color(&self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(255, 255, 255),
            Theme::Dark => Color32::from_rgb(48, 48, 48),
            Theme::System => Color32::from_rgb(48, 48, 48),
        }
    }

    /// Get text color
    pub fn text_color(&self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(33, 33, 33),
            Theme::Dark => Color32::from_rgb(230, 230, 230),
            Theme::System => Color32::from_rgb(230, 230, 230),
        }
    }

    /// Get muted text color
    pub fn muted_text_color(&self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(117, 117, 117),
            Theme::Dark => Color32::from_rgb(158, 158, 158),
            Theme::System => Color32::from_rgb(158, 158, 158),
        }
    }

    /// Get border color
    pub fn border_color(&self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(224, 224, 224),
            Theme::Dark => Color32::from_rgb(64, 64, 64),
            Theme::System => Color32::from_rgb(64, 64, 64),
        }
    }

    /// Get success color
    pub fn success_color(&self) -> Color32 {
        Color32::from_rgb(76, 175, 80)
    }

    /// Get warning color
    pub fn warning_color(&self) -> Color32 {
        Color32::from_rgb(255, 152, 0)
    }

    /// Get error color
    pub fn error_color(&self) -> Color32 {
        Color32::from_rgb(244, 67, 54)
    }

    /// Get info color
    pub fn info_color(&self) -> Color32 {
        Color32::from_rgb(33, 150, 243)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}
