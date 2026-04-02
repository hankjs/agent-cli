use async_trait::async_trait;
use hank_core::permission::PermissionDecision;
use hank_core::tool::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DANGEROUS_PATTERNS: &[&str] = &["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];
const MAX_OUTPUT: usize = 50_000;

// ── BashTool ──

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }
    fn description(&self) -> &str { "Execute a bash command and return its output." }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"command":{"type":"string"},"timeout":{"type":"number"}},"required":["command"]})
    }
    fn is_read_only(&self, _: &Value) -> bool { false }
    fn check_permissions(&self, input: &Value) -> PermissionDecision {
        let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        for pat in DANGEROUS_PATTERNS {
            if cmd.contains(pat) {
                return PermissionDecision::Deny(format!("Blocked: dangerous command pattern '{pat}'"));
            }
        }
        PermissionDecision::Ask
    }
    fn prompt(&self) -> &str {
        "Execute bash commands. Supports timeout (default 120s). Quote paths with spaces. Prefer dedicated tools over bash for file operations."
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let cmd = input.get("command").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'command'".into()))?;
        let timeout_ms = input.get("timeout").and_then(|v| v.as_u64()).unwrap_or(120_000);

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            tokio::process::Command::new("bash").arg("-c").arg(cmd)
                .current_dir(&ctx.working_dir)
                .output()
        ).await;

        match result {
            Ok(Ok(output)) => {
                let mut out = String::from_utf8_lossy(&output.stdout).to_string();
                let err = String::from_utf8_lossy(&output.stderr);
                if !err.is_empty() { out.push_str(&format!("\nSTDERR:\n{err}")); }
                if out.len() > MAX_OUTPUT { out.truncate(MAX_OUTPUT); }
                Ok(ToolResult { data: Value::String(out), new_messages: None })
            }
            Ok(Err(e)) => Err(ToolError::ExecutionError(e.to_string())),
            Err(_) => Err(ToolError::Timeout),
        }
    }
}

// ── FileReadTool ──

pub struct FileReadTool;

fn resolve_path(base: &Path, file_path: &str) -> Result<PathBuf, ToolError> {
    let p = Path::new(file_path);
    let resolved = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    let canonical = resolved.canonicalize()
        .map_err(|_| ToolError::ExecutionError(format!("File not found: {file_path}")))?;
    Ok(canonical)
}

fn safe_path(base: &Path, file_path: &str) -> Result<PathBuf, ToolError> {
    let p = Path::new(file_path);
    let resolved = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    // Prevent traversal outside working dir for write operations
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    // For new files, check the parent
    let check = if resolved.exists() {
        resolved.canonicalize().unwrap_or(resolved.clone())
    } else {
        let parent = resolved.parent().unwrap_or(base);
        let _ = std::fs::create_dir_all(parent);
        parent.canonicalize().unwrap_or(parent.to_path_buf()).join(resolved.file_name().unwrap_or_default())
    };
    if !check.starts_with(&canonical_base) {
        return Err(ToolError::ExecutionError("Path traversal outside working directory".into()));
    }
    Ok(resolved)
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str { "read" }
    fn description(&self) -> &str { "Read a file and return contents with line numbers." }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"file_path":{"type":"string"},"offset":{"type":"number"},"limit":{"type":"number"}},"required":["file_path"]})
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool { true }
    fn is_read_only(&self, _: &Value) -> bool { true }
    fn prompt(&self) -> &str {
        "Read files with line numbers. Use absolute paths. Default limit 2000 lines. Supports offset/limit for large files."
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let fp = input.get("file_path").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'file_path'".into()))?;
        let path = resolve_path(&ctx.working_dir, fp)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read {fp}: {e}")))?;

        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;

        let lines: Vec<String> = content.lines().enumerate()
            .skip(offset).take(limit)
            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
            .collect();

        Ok(ToolResult { data: Value::String(lines.join("\n")), new_messages: None })
    }
}

// ── FileWriteTool ──

pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str { "write" }
    fn description(&self) -> &str { "Write content to a file, creating parent directories if needed." }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"file_path":{"type":"string"},"content":{"type":"string"}},"required":["file_path","content"]})
    }
    fn check_permissions(&self, _: &Value) -> PermissionDecision { PermissionDecision::Ask }
    fn prompt(&self) -> &str {
        "Write files. Overwrites existing files. Prefer Edit for modifications. Read file first before overwriting."
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let fp = input.get("file_path").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'file_path'".into()))?;
        let content = input.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'content'".into()))?;
        let path = safe_path(&ctx.working_dir, fp)?;
        if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
        std::fs::write(&path, content)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write {fp}: {e}")))?;
        Ok(ToolResult { data: Value::String(format!("Written to {fp}")), new_messages: None })
    }
}

// ── FileEditTool ──

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str { "edit" }
    fn description(&self) -> &str { "Replace a string in a file. old_string must be unique unless replace_all is true." }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"file_path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"},"replace_all":{"type":"boolean"}},"required":["file_path","old_string","new_string"]})
    }
    fn check_permissions(&self, _: &Value) -> PermissionDecision { PermissionDecision::Ask }
    fn prompt(&self) -> &str {
        "Edit files by replacing strings. Read file first. Preserve indentation. old_string must be unique or use replace_all."
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let fp = input.get("file_path").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'file_path'".into()))?;
        let old = input.get("old_string").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'old_string'".into()))?;
        let new = input.get("new_string").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'new_string'".into()))?;
        let replace_all = input.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);

        let path = resolve_path(&ctx.working_dir, fp)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read {fp}: {e}")))?;

        let count = content.matches(old).count();
        if count == 0 {
            return Err(ToolError::ExecutionError("old_string not found in file".into()));
        }
        if count > 1 && !replace_all {
            return Err(ToolError::ExecutionError(
                format!("old_string is not unique (found {count} times). Use replace_all or provide more context.")
            ));
        }

        let new_content = if replace_all { content.replace(old, new) } else { content.replacen(old, new, 1) };
        std::fs::write(&path, &new_content)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write {fp}: {e}")))?;
        Ok(ToolResult { data: Value::String(format!("Edited {fp} ({count} replacement(s))")), new_messages: None })
    }
}

// ── GlobTool ──

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str { "glob" }
    fn description(&self) -> &str { "Find files matching a glob pattern, sorted by modification time." }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern"]})
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool { true }
    fn is_read_only(&self, _: &Value) -> bool { true }
    fn prompt(&self) -> &str { "Search for files by glob pattern (e.g. **/*.rs). Sorted by mtime." }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let pattern = input.get("pattern").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'pattern'".into()))?;
        let base = input.get("path").and_then(|v| v.as_str())
            .map(|p| ctx.working_dir.join(p))
            .unwrap_or_else(|| ctx.working_dir.clone());

        let full_pattern = base.join(pattern).to_string_lossy().to_string();
        let mut entries: Vec<(PathBuf, std::time::SystemTime)> = glob::glob(&full_pattern)
            .map_err(|e| ToolError::ExecutionError(format!("Invalid pattern: {e}")))?
            .filter_map(|r| r.ok())
            .filter_map(|p| p.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (p, t)))
            .collect();

        entries.sort_by(|a, b| b.1.cmp(&a.1));
        if entries.len() > 1000 { entries.truncate(1000); }

        if entries.is_empty() {
            return Ok(ToolResult { data: Value::String("No files found".into()), new_messages: None });
        }
        let list: Vec<String> = entries.iter().map(|(p, _)| p.display().to_string()).collect();
        Ok(ToolResult { data: Value::String(list.join("\n")), new_messages: None })
    }
}

// ── GrepTool ──

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str { "grep" }
    fn description(&self) -> &str { "Search file contents with regex pattern." }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"},"glob":{"type":"string"},"-i":{"type":"boolean"},"context":{"type":"number"}},"required":["pattern"]})
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool { true }
    fn is_read_only(&self, _: &Value) -> bool { true }
    fn prompt(&self) -> &str { "Search file contents with regex. Supports glob filter, case-insensitive, context lines." }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let pattern = input.get("pattern").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ValidationError("missing 'pattern'".into()))?;
        let case_insensitive = input.get("-i").and_then(|v| v.as_bool()).unwrap_or(false);
        let ctx_lines = input.get("context").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        let re = regex::RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|e| ToolError::ValidationError(format!("Invalid regex: {e}")))?;

        let base = input.get("path").and_then(|v| v.as_str())
            .map(|p| ctx.working_dir.join(p))
            .unwrap_or_else(|| ctx.working_dir.clone());

        let file_glob = input.get("glob").and_then(|v| v.as_str()).unwrap_or("**/*");
        let full_pattern = base.join(file_glob).to_string_lossy().to_string();

        let mut results = Vec::new();
        let files = glob::glob(&full_pattern).map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        for entry in files.filter_map(|r| r.ok()).filter(|p| p.is_file()) {
            if let Ok(content) = std::fs::read_to_string(&entry) {
                let lines: Vec<&str> = content.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    if re.is_match(line) {
                        let start = i.saturating_sub(ctx_lines);
                        let end = (i + ctx_lines + 1).min(lines.len());
                        let snippet: Vec<String> = (start..end)
                            .map(|j| format!("{}:{}: {}", entry.display(), j + 1, lines[j]))
                            .collect();
                        results.push(snippet.join("\n"));
                        if results.len() >= 1000 { break; }
                    }
                }
            }
            if results.len() >= 1000 { break; }
        }

        if results.is_empty() {
            return Ok(ToolResult { data: Value::String("No matches found".into()), new_messages: None });
        }
        Ok(ToolResult { data: Value::String(results.join("\n--\n")), new_messages: None })
    }
}

/// Register all built-in tools.
pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(FileReadTool));
    registry.register(Arc::new(FileWriteTool));
    registry.register(Arc::new(FileEditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::watch;

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("hank-{prefix}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn context(working_dir: PathBuf) -> ToolContext {
        let (_tx, rx) = watch::channel(false);
        ToolContext { working_dir, abort: rx }
    }

    #[tokio::test]
    async fn bash_tool_executes_simple_command() {
        let dir = temp_dir("bash-tool");
        let result = BashTool
            .call(json!({"command": "printf hello"}), &context(dir.clone()))
            .await
            .unwrap();

        assert_eq!(result.data, Value::String("hello".into()));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn file_write_and_read_round_trip() {
        let dir = temp_dir("file-roundtrip");
        let ctx = context(dir.clone());

        FileWriteTool
            .call(
                json!({"file_path": "notes.txt", "content": "alpha\nbeta\n"}),
                &ctx,
            )
            .await
            .unwrap();

        let result = FileReadTool
            .call(json!({"file_path": "notes.txt"}), &ctx)
            .await
            .unwrap();
        let text = result.data.as_str().unwrap();

        assert!(text.contains("\talpha"));
        assert!(text.contains("\tbeta"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn file_edit_requires_unique_match_without_replace_all() {
        let dir = temp_dir("file-edit");
        fs::write(dir.join("dup.txt"), "same\nsame\n").unwrap();

        let error = FileEditTool
            .call(
                json!({"file_path": "dup.txt", "old_string": "same", "new_string": "new"}),
                &context(dir.clone()),
            )
            .await
            .unwrap_err();

        match error {
            ToolError::ExecutionError(message) => {
                assert!(message.contains("not unique"));
            }
            other => panic!("expected uniqueness error, got {other:?}"),
        }

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn glob_tool_finds_matching_files() {
        let dir = temp_dir("glob-tool");
        fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src").join("lib.rs"), "pub fn run() {}\n").unwrap();

        let result = GlobTool
            .call(json!({"pattern": "**/*.rs"}), &context(dir.clone()))
            .await
            .unwrap();
        let text = result.data.as_str().unwrap();

        assert!(text.contains("main.rs"));
        assert!(text.contains("lib.rs"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn grep_tool_matches_regex_with_context_lines() {
        let dir = temp_dir("grep-tool");
        fs::write(dir.join("sample.txt"), "alpha\nbeta\ngamma\n").unwrap();

        let result = GrepTool
            .call(
                json!({"pattern": "beta", "glob": "**/*.txt", "context": 1}),
                &context(dir.clone()),
            )
            .await
            .unwrap();
        let text = result.data.as_str().unwrap();

        assert!(text.contains("sample.txt:1: alpha"));
        assert!(text.contains("sample.txt:2: beta"));
        assert!(text.contains("sample.txt:3: gamma"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn register_all_adds_builtin_tools() {
        let mut registry = ToolRegistry::new();
        register_all(&mut registry);

        for tool_name in ["bash", "read", "write", "edit", "glob", "grep"] {
            assert!(registry.get(tool_name).is_some(), "missing {tool_name}");
        }
    }
}
