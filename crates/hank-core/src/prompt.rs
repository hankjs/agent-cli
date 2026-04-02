use std::path::{Path, PathBuf};

// Base system prompt sections ported from Claude Code

pub const INTRO: &str = r#"You are Claude Code, an interactive CLI assistant that helps users with software engineering tasks. Use the instructions below and the tools available to you to assist the user.

IMPORTANT: Assist with authorized security testing, defensive security, CTF challenges, and educational contexts. Refuse requests for destructive techniques, DoS attacks, mass targeting, supply chain compromise, or detection evasion for malicious purposes."#;

pub const SYSTEM: &str = r#"# System
 - All text you output outside of tool use is displayed to the user.
 - Tools are executed in a user-selected permission mode. When you attempt to call a tool that is not automatically allowed, the user will be prompted to approve or deny.
 - Tool results may include <system-reminder> tags. Tags contain information from the system.
 - The system will automatically compress prior messages as it approaches context limits."#;

pub const DOING_TASKS: &str = r#"# Doing tasks
 - The user will primarily request software engineering tasks.
 - Do not propose changes to code you haven't read. Read files first.
 - Do not create files unless absolutely necessary. Prefer editing existing files.
 - Avoid giving time estimates or predictions.
 - Be careful not to introduce security vulnerabilities.
 - Avoid over-engineering. Only make changes that are directly requested or clearly necessary.
 - Don't add features, refactor code, or make "improvements" beyond what was asked.
 - Don't add error handling for scenarios that can't happen.
 - Don't create helpers or abstractions for one-time operations."#;

pub const ACTIONS: &str = r#"# Executing actions with care
Carefully consider the reversibility and blast radius of actions. For actions that are hard to reverse, affect shared systems, or could be destructive, check with the user before proceeding. Match the scope of your actions to what was actually requested."#;

pub const USING_TOOLS: &str = r#"# Using your tools
 - Do NOT use Bash to run commands when a relevant dedicated tool is provided:
  - To read files use Read instead of cat, head, tail, or sed
  - To edit files use Edit instead of sed or awk
  - To create files use Write instead of cat with heredoc or echo
  - To search for files use Glob instead of find or ls
  - To search file content use Grep instead of grep or rg
 - You can call multiple tools in a single response. Make independent calls in parallel."#;

pub const TONE_STYLE: &str = r#"# Tone and style
 - Only use emojis if the user explicitly requests it.
 - Your responses should be short and concise.
 - When referencing code include the pattern file_path:line_number."#;

pub const GIT_COMMIT: &str = r#"# Committing changes with git
Only create commits when requested. Follow these steps:
1. Run git status and git diff in parallel to see changes.
2. Draft a concise commit message focusing on "why" not "what".
3. Stage specific files (not git add -A). Create the commit with HEREDOC format.
4. If pre-commit hook fails, fix the issue and create a NEW commit (never amend).
Never force-push, never skip hooks, never amend published commits."#;

pub const GIT_PR: &str = r#"# Creating pull requests
Use gh CLI for GitHub tasks. Steps:
1. Run git status, git diff, and git log in parallel.
2. Draft a short PR title (<70 chars) and detailed body.
3. Push and create PR with gh pr create using HEREDOC body."#;

pub struct EnvironmentConfig {
    pub working_dir: PathBuf,
    pub is_git_repo: bool,
    pub platform: String,
    pub shell: String,
    pub os_version: String,
    pub model_name: String,
    pub model_id: String,
}

pub fn render_environment(config: &EnvironmentConfig) -> String {
    format!(
        "# Environment\n - Working directory: {}\n  - Is a git repository: {}\n - Platform: {}\n - Shell: {}\n - OS Version: {}\n - Model: {} ({})",
        config.working_dir.display(), config.is_git_repo,
        config.platform, config.shell, config.os_version,
        config.model_name, config.model_id,
    )
}

/// Discover CLAUDE.md files: ~/.claude/CLAUDE.md, walk up from cwd, .claude.local/CLAUDE.md
pub fn discover_claude_md(working_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();

    // User-level
    if let Some(home) = dirs_path() {
        let p = home.join(".claude").join("CLAUDE.md");
        if let Ok(content) = std::fs::read_to_string(&p) {
            results.push((p, content));
        }
    }

    // Walk up from working dir
    let mut dir = working_dir.to_path_buf();
    loop {
        for name in &["CLAUDE.md", ".claude/CLAUDE.md"] {
            let p = dir.join(name);
            if let Ok(content) = std::fs::read_to_string(&p) {
                results.push((p, content));
            }
        }
        if !dir.pop() { break; }
    }

    // Local override
    let local = working_dir.join(".claude.local").join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&local) {
        results.push((local, content));
    }

    results
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Collect git context: branch, status, recent commits.
pub fn collect_git_context(working_dir: &Path) -> Option<String> {
    use std::process::Command;

    let branch = Command::new("git").args(["branch", "--show-current"])
        .current_dir(working_dir).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;

    let status = Command::new("git").args(["status", "--short"])
        .current_dir(working_dir).output().ok()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout).to_string();
            if s.len() > 2048 { s[..2048].to_string() } else { s }
        }).unwrap_or_default();

    let log = Command::new("git").args(["log", "--oneline", "-5"])
        .current_dir(working_dir).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    Some(format!("Current branch: {branch}\n\nStatus:\n{status}\nRecent commits:\n{log}"))
}

/// Build the full system prompt from all layers.
pub fn build_system_prompt(
    tool_prompts: &[(String, String)], // (name, prompt_text)
    config: &EnvironmentConfig,
) -> String {
    let mut parts = vec![
        INTRO.to_string(),
        SYSTEM.to_string(),
        DOING_TASKS.to_string(),
        ACTIONS.to_string(),
        USING_TOOLS.to_string(),
        TONE_STYLE.to_string(),
        GIT_COMMIT.to_string(),
        GIT_PR.to_string(),
    ];

    // Tool prompts
    if !tool_prompts.is_empty() {
        let tool_section: Vec<String> = tool_prompts.iter()
            .filter(|(_, p)| !p.is_empty())
            .map(|(name, prompt)| format!("## {name}\n{prompt}"))
            .collect();
        if !tool_section.is_empty() {
            parts.push(format!("# Tool Instructions\n{}", tool_section.join("\n\n")));
        }
    }

    // Environment
    parts.push(render_environment(config));

    // Git context
    if config.is_git_repo {
        if let Some(git_ctx) = collect_git_context(&config.working_dir) {
            parts.push(format!("gitStatus: {git_ctx}"));
        }
    }

    // CLAUDE.md
    let claude_mds = discover_claude_md(&config.working_dir);
    for (path, content) in &claude_mds {
        parts.push(format!("# CLAUDE.md ({})\n{}", path.display(), content));
    }

    parts.join("\n\n")
}

/// Wrap content in system-reminder tags for user context injection.
pub fn wrap_system_reminder(content: &str) -> String {
    format!("<system-reminder>\n{content}\n</system-reminder>")
}

/// Build the user context injection message (CLAUDE.md + date).
pub fn build_user_context(working_dir: &Path, date: &str) -> Option<String> {
    let claude_mds = discover_claude_md(working_dir);
    if claude_mds.is_empty() && date.is_empty() {
        return None;
    }
    let mut parts = vec!["As you answer the user's questions, you can use the following context:".to_string()];
    for (path, content) in &claude_mds {
        parts.push(format!("# CLAUDE.md ({})\n{}", path.display(), content));
    }
    parts.push(format!("# currentDate\nToday's date is {date}."));
    Some(wrap_system_reminder(&parts.join("\n\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("hank-{prefix}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let status = Command::new("git").args(args).current_dir(dir).status().unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }

    #[test]
    fn build_system_prompt_keeps_sections_in_order_and_includes_context() {
        let dir = temp_dir("prompt-system");
        fs::write(dir.join("CLAUDE.md"), "Project rule: stay focused.").unwrap();
        run_git(&dir, &["init"]);

        let config = EnvironmentConfig {
            working_dir: dir.clone(),
            is_git_repo: true,
            platform: "macos".into(),
            shell: "zsh".into(),
            os_version: "test".into(),
            model_name: "test-model".into(),
            model_id: "test-model-id".into(),
        };

        let prompt = build_system_prompt(
            &[("bash".into(), "Run shell commands safely.".into())],
            &config,
        );

        let intro = prompt.find(INTRO).unwrap();
        let system = prompt.find(SYSTEM).unwrap();
        let doing = prompt.find(DOING_TASKS).unwrap();
        let actions = prompt.find(ACTIONS).unwrap();
        let tools = prompt.find(USING_TOOLS).unwrap();
        let tone = prompt.find(TONE_STYLE).unwrap();

        assert!(intro < system);
        assert!(system < doing);
        assert!(doing < actions);
        assert!(actions < tools);
        assert!(tools < tone);
        assert!(prompt.contains("# Tool Instructions"));
        assert!(prompt.contains("## bash\nRun shell commands safely."));
        assert!(prompt.contains("gitStatus: Current branch:"));
        assert!(prompt.contains("Project rule: stay focused."));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn build_user_context_wraps_claude_md_and_date() {
        let dir = temp_dir("prompt-user-context");
        fs::write(dir.join("CLAUDE.md"), "Local instruction").unwrap();

        let context = build_user_context(&dir, "2026-04-01").unwrap();

        assert!(context.starts_with("<system-reminder>"));
        assert!(context.ends_with("</system-reminder>"));
        assert!(context.contains("As you answer the user's questions"));
        assert!(context.contains("Local instruction"));
        assert!(context.contains("Today's date is 2026-04-01."));

        let _ = fs::remove_dir_all(dir);
    }
}
