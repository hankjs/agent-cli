//! GUI Module - Desktop GUI using egui/eframe
//!
//! This module provides a native desktop GUI for Claude Code
//! with a modern, responsive interface.

pub mod app;
pub mod chat;
pub mod sidebar;
pub mod settings;
pub mod theme;

pub use app::ClaudeCodeApp;
pub use theme::Theme;
