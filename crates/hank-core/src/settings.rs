use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::permission::{PermissionDecision, PermissionMode, PermissionRule};

/// Claude Code compatible settings.json structure.
/// Supports the same format used by `~/.claude/settings.json`,
/// `<project>/.claude/settings.json`, and `<project>/.claude/settings.local.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    #[serde(rename = "$schema")]
    pub schema: Option<String>,

    // Model
    pub model: Option<String>,
    pub available_models: Option<Vec<String>>,
    pub always_thinking_enabled: Option<bool>,

    // Permissions
    pub permissions: Option<PermissionSettings>,

    // Environment variables
    pub env: Option<HashMap<String, String>>,

    // MCP
    pub enable_all_project_mcp_servers: Option<bool>,
    pub enabled_mcpjson_servers: Option<Vec<String>>,

    // Hooks (stored as raw JSON — not interpreted by hank yet)
    pub hooks: Option<serde_json::Value>,

    // Git
    pub include_git_instructions: Option<bool>,

    // UI
    pub language: Option<String>,

    // Plugins (passthrough)
    pub enabled_plugins: Option<HashMap<String, bool>>,

    // Catch-all for unknown fields
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionSettings {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    #[serde(default)]
    pub ask: Vec<String>,
    pub default_mode: Option<String>,
    pub additional_directories: Option<Vec<String>>,
}

/// Sources loaded in priority order (low → high).
#[derive(Debug)]
pub struct SettingsSources {
    pub user: Option<Settings>,
    pub project: Option<Settings>,
    pub local: Option<Settings>,
}

impl Settings {
    /// Load from a single JSON file. Returns None if file doesn't exist.
    pub fn load_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Load all settings sources and merge them (user → project → local).
    pub fn load_merged(project_dir: &Path) -> Self {
        let config_home = std::env::var("CLAUDE_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs_home().map(|h| h.join(".claude")).unwrap_or_default()
            });

        let user = Self::load_file(&config_home.join("settings.json"));
        let project = Self::load_file(&project_dir.join(".claude").join("settings.json"));
        let local = Self::load_file(&project_dir.join(".claude").join("settings.local.json"));

        let mut merged = Settings::default();
        for source in [user, project, local].into_iter().flatten() {
            merged.merge(source);
        }
        merged
    }

    /// Merge another settings on top of self (higher priority wins for scalars,
    /// arrays are concatenated and deduped).
    fn merge(&mut self, other: Settings) {
        if other.model.is_some() { self.model = other.model; }
        if other.available_models.is_some() { self.available_models = other.available_models; }
        if other.always_thinking_enabled.is_some() { self.always_thinking_enabled = other.always_thinking_enabled; }
        if other.language.is_some() { self.language = other.language; }
        if other.include_git_instructions.is_some() { self.include_git_instructions = other.include_git_instructions; }
        if other.enable_all_project_mcp_servers.is_some() { self.enable_all_project_mcp_servers = other.enable_all_project_mcp_servers; }
        if other.hooks.is_some() { self.hooks = other.hooks; }

        // Env: merge maps
        if let Some(other_env) = other.env {
            self.env.get_or_insert_with(HashMap::new).extend(other_env);
        }

        // Plugins: merge maps
        if let Some(other_plugins) = other.enabled_plugins {
            self.enabled_plugins.get_or_insert_with(HashMap::new).extend(other_plugins);
        }

        // Permissions: concatenate and dedup arrays
        if let Some(other_perms) = other.permissions {
            let perms = self.permissions.get_or_insert_with(PermissionSettings::default);
            merge_dedup(&mut perms.allow, other_perms.allow);
            merge_dedup(&mut perms.deny, other_perms.deny);
            merge_dedup(&mut perms.ask, other_perms.ask);
            if other_perms.default_mode.is_some() { perms.default_mode = other_perms.default_mode; }
            if other_perms.additional_directories.is_some() { perms.additional_directories = other_perms.additional_directories; }
        }

        // Extra fields: merge
        self.extra.extend(other.extra);
    }

    /// Apply env vars from settings to the process environment.
    pub fn apply_env(&self) {
        if let Some(env) = &self.env {
            for (k, v) in env {
                // SAFETY: called once at startup before spawning threads
                unsafe { std::env::set_var(k, v); }
            }
        }
    }

    /// Resolve the effective model name.
    /// Checks settings.model, then ANTHROPIC_MODEL env var, then falls back to default.
    pub fn resolve_model(&self) -> String {
        if let Some(m) = &self.model {
            // Handle aliases like "haiku", "sonnet", "opus"
            return resolve_model_alias(m);
        }
        if let Ok(m) = std::env::var("ANTHROPIC_MODEL") {
            return m;
        }
        "claude-sonnet-4-20250514".into()
    }

    /// Convert permission settings to PermissionMode + PermissionRules.
    pub fn to_permission_config(&self) -> (PermissionMode, Vec<PermissionRule>) {
        let mut rules = Vec::new();
        let mut mode = PermissionMode::Default;

        if let Some(perms) = &self.permissions {
            for pattern in &perms.allow {
                rules.push(PermissionRule {
                    tool_pattern: pattern.clone(),
                    behavior: PermissionDecision::Allow,
                });
            }
            for pattern in &perms.deny {
                rules.push(PermissionRule {
                    tool_pattern: pattern.clone(),
                    behavior: PermissionDecision::Deny("Denied by settings".into()),
                });
            }
            for pattern in &perms.ask {
                rules.push(PermissionRule {
                    tool_pattern: pattern.clone(),
                    behavior: PermissionDecision::Ask,
                });
            }
            if let Some(dm) = &perms.default_mode {
                mode = match dm.as_str() {
                    "acceptEdits" => PermissionMode::AcceptEdits,
                    "bypassPermissions" => PermissionMode::Bypass,
                    _ => PermissionMode::Default,
                };
            }
        }

        (mode, rules)
    }

    /// Resolve API base URL from env (set by settings.env or process env).
    pub fn resolve_base_url(&self) -> Option<String> {
        std::env::var("ANTHROPIC_BASE_URL").ok()
    }

    /// Resolve API key from env.
    pub fn resolve_api_key(&self) -> Option<String> {
        std::env::var("ANTHROPIC_AUTH_TOKEN").ok()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
    }
}

fn resolve_model_alias(model: &str) -> String {
    match model {
        "haiku" => std::env::var("ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .unwrap_or_else(|_| "claude-haiku-4-5-20251001".into()),
        "sonnet" => std::env::var("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".into()),
        "opus" => std::env::var("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .unwrap_or_else(|_| "claude-opus-4-20250514".into()),
        other => other.to_string(),
    }
}

fn merge_dedup(target: &mut Vec<String>, source: Vec<String>) {
    for item in source {
        if !target.contains(&item) {
            target.push(item);
        }
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
