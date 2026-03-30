use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anthropic_ai_sdk::client::AnthropicClient;
use anthropic_ai_sdk::types::message::{
    ContentBlock, CreateMessageParams, Message, MessageClient, MessageContent, MessageError,
    RequiredMessageParams, Role, StopReason, Tool,
};
use serde_json::json;

const DANGEROUS: &[&str] = &["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];
const THRESHOLD: usize = 50000;
const KEEP_RECENT: usize = 3;

// -- TodoManager: structured state the LLM writes to --
struct TodoItem {
    id: String,
    text: String,
    status: String, // "pending" | "in_progress" | "completed"
}

struct TodoManager {
    items: Vec<TodoItem>,
}

impl TodoManager {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn update(&mut self, items: &[serde_json::Value]) -> Result<String, String> {
        if items.len() > 20 {
            return Err("Max 20 todos allowed".into());
        }
        let mut validated = Vec::new();
        let mut in_progress_count = 0;
        for (i, item) in items.iter().enumerate() {
            let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_lowercase();
            let id = item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
                .unwrap_or_else(|| (i + 1).to_string());
            if text.is_empty() {
                return Err(format!("Item {id}: text required"));
            }
            if !["pending", "in_progress", "completed"].contains(&status.as_str()) {
                return Err(format!("Item {id}: invalid status '{status}'"));
            }
            if status == "in_progress" {
                in_progress_count += 1;
            }
            validated.push(TodoItem { id, text, status });
        }
        if in_progress_count > 1 {
            return Err("Only one task can be in_progress at a time".into());
        }
        self.items = validated;
        Ok(self.render())
    }

    fn render(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".into();
        }
        let mut lines: Vec<String> = self.items.iter().map(|item| {
            let marker = match item.status.as_str() {
                "pending" => "[ ]",
                "in_progress" => "[>]",
                "completed" => "[x]",
                _ => "[ ]",
            };
            format!("{marker} #{}: {}", item.id, item.text)
        }).collect();
        let done = self.items.iter().filter(|t| t.status == "completed").count();
        lines.push(format!("\n({done}/{} completed)", self.items.len()));
        lines.join("\n")
    }
}

// -- SkillLoader: scan skills/<name>/SKILL.md with YAML frontmatter --
struct Skill {
    meta: HashMap<String, String>,
    body: String,
}

struct SkillLoader {
    skills: HashMap<String, Skill>,
}

impl SkillLoader {
    fn new(skills_dir: &Path) -> Self {
        let mut skills = HashMap::new();
        if skills_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(skills_dir) {
                let mut dirs: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                dirs.sort_by_key(|e| e.file_name());
                for entry in dirs {
                    let skill_file = entry.path().join("SKILL.md");
                    if skill_file.is_file() {
                        if let Ok(text) = fs::read_to_string(&skill_file) {
                            let (meta, body) = Self::parse_frontmatter(&text);
                            let name = meta.get("name").cloned()
                                .unwrap_or_else(|| entry.file_name().to_string_lossy().into());
                            skills.insert(name, Skill { meta, body });
                        }
                    }
                }
            }
        }
        Self { skills }
    }

    fn parse_frontmatter(text: &str) -> (HashMap<String, String>, String) {
        // Split on --- delimiters
        if !text.starts_with("---\n") {
            return (HashMap::new(), text.to_string());
        }
        let rest = &text[4..]; // skip first "---\n"
        if let Some(end) = rest.find("\n---\n") {
            let yaml_block = &rest[..end];
            let body = rest[end + 5..].trim().to_string();
            let mut meta = HashMap::new();
            for line in yaml_block.lines() {
                if let Some((key, val)) = line.split_once(':') {
                    meta.insert(key.trim().to_string(), val.trim().to_string());
                }
            }
            (meta, body)
        } else {
            (HashMap::new(), text.to_string())
        }
    }

    fn get_descriptions(&self) -> String {
        if self.skills.is_empty() {
            return "(no skills available)".into();
        }
        let mut names: Vec<&String> = self.skills.keys().collect();
        names.sort();
        names.iter().map(|name| {
            let skill = &self.skills[*name];
            let desc = skill.meta.get("description").map(|s| s.as_str()).unwrap_or("No description");
            let tags = skill.meta.get("tags").map(|s| s.as_str()).unwrap_or("");
            if tags.is_empty() {
                format!("  - {name}: {desc}")
            } else {
                format!("  - {name}: {desc} [{tags}]")
            }
        }).collect::<Vec<_>>().join("\n")
    }

    fn get_content(&self, name: &str) -> String {
        match self.skills.get(name) {
            Some(skill) => format!("<skill name=\"{name}\">\n{}\n</skill>", skill.body),
            None => {
                let mut available: Vec<&String> = self.skills.keys().collect();
                available.sort();
                format!("Error: Unknown skill '{name}'. Available: {}", available.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
            }
        }
    }
}

// -- Context compression: three-layer pipeline --
fn estimate_tokens(history: &[Message]) -> usize {
    serde_json::to_string(history).map(|s| s.len() / 4).unwrap_or(0)
}

fn micro_compact(history: &mut Vec<Message>) {
    // Build tool_use_id → tool_name map from assistant messages
    let mut tool_name_map: HashMap<String, String> = HashMap::new();
    for msg in history.iter() {
        if let MessageContent::Blocks { content } = &msg.content {
            if matches!(msg.role, Role::Assistant) {
                for block in content {
                    if let ContentBlock::ToolUse { id, name, .. } = block {
                        tool_name_map.insert(id.clone(), name.clone());
                    }
                }
            }
        }
    }
    // Collect positions of all ToolResult blocks
    let mut positions: Vec<(usize, usize)> = Vec::new(); // (msg_idx, block_idx)
    for (msg_idx, msg) in history.iter().enumerate() {
        if let MessageContent::Blocks { content } = &msg.content {
            if matches!(msg.role, Role::User) {
                for (block_idx, block) in content.iter().enumerate() {
                    if let ContentBlock::ToolResult { .. } = block {
                        positions.push((msg_idx, block_idx));
                    }
                }
            }
        }
    }
    if positions.len() <= KEEP_RECENT {
        return;
    }
    let to_clear = &positions[..positions.len() - KEEP_RECENT];
    for &(msg_idx, block_idx) in to_clear {
        if let MessageContent::Blocks { content } = &mut history[msg_idx].content {
            if let ContentBlock::ToolResult { tool_use_id, content: c } = &mut content[block_idx] {
                if c.len() > 100 {
                    let name = tool_name_map.get(tool_use_id.as_str()).map(|s| s.as_str()).unwrap_or("unknown");
                    *c = format!("[Previous: used {name}]");
                }
            }
        }
    }
}

async fn auto_compact(client: &AnthropicClient, model: &str, workdir: &Path, history: &[Message]) -> Vec<Message> {
    // Save transcript
    let transcript_dir = workdir.join(".transcripts");
    let _ = fs::create_dir_all(&transcript_dir);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let transcript_path = transcript_dir.join(format!("transcript_{timestamp}.jsonl"));
    if let Ok(mut file) = fs::File::create(&transcript_path) {
        for msg in history {
            if let Ok(line) = serde_json::to_string(msg) {
                let _ = io::Write::write_all(&mut file, line.as_bytes());
                let _ = io::Write::write_all(&mut file, b"\n");
            }
        }
    }
    println!("[transcript saved: {}]", transcript_path.display());

    // Ask LLM to summarize
    let conversation_text: String = serde_json::to_string(history).unwrap_or_default().chars().take(80000).collect();
    let summary_prompt = format!(
        "Summarize this conversation for continuity. Include: \
        1) What was accomplished, 2) Current state, 3) Key decisions made. \
        Be concise but preserve critical details.\n\n{conversation_text}"
    );
    let summary = match client.create_message(Some(&CreateMessageParams::new(RequiredMessageParams {
        model: model.to_string(),
        messages: vec![Message::new_text(Role::User, &summary_prompt)],
        max_tokens: 2000,
    }))).await {
        Ok(resp) => {
            resp.content.iter().find_map(|b| {
                if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
            }).unwrap_or_else(|| "(summary failed)".into())
        }
        Err(e) => format!("(summary error: {e})"),
    };

    vec![
        Message::new_text(Role::User, &format!("[Conversation compressed. Transcript: {}]\n\n{summary}", transcript_path.display())),
        Message::new_text(Role::Assistant, "Understood. I have the context from the summary. Continuing."),
    ]
}

fn safe_path(workdir: &Path, p: &str) -> Result<PathBuf, String> {
    // If the model passes an absolute path, strip the workdir prefix to make it relative
    let p = if let Some(stripped) = p.strip_prefix(&format!("{}/", workdir.display())) {
        stripped
    } else {
        p
    };
    let joined = workdir.join(p);
    // For existing files, canonicalize directly
    if joined.exists() {
        let resolved = joined.canonicalize().map_err(|e| format!("Path error: {e}"))?;
        if !resolved.starts_with(workdir) {
            return Err(format!("Path escapes workspace: {p}"));
        }
        return Ok(resolved);
    }
    // For new files, canonicalize the parent directory
    let parent = joined.parent().ok_or_else(|| format!("Invalid path: {p}"))?;
    let resolved_parent = if parent.exists() {
        parent.canonicalize().map_err(|e| format!("Path error: {e}"))?
    } else {
        // Parent doesn't exist yet — walk up to find an existing ancestor
        let mut ancestor = parent.to_path_buf();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| format!("Invalid path: {p}"))?.to_path_buf();
        }
        let resolved_ancestor = ancestor.canonicalize().map_err(|e| format!("Path error: {e}"))?;
        if !resolved_ancestor.starts_with(workdir) {
            return Err(format!("Path escapes workspace: {p}"));
        }
        resolved_ancestor
    };
    if !resolved_parent.starts_with(workdir) {
        return Err(format!("Path escapes workspace: {p}"));
    }
    // Reconstruct the full path with the resolved parent
    let file_name = joined.file_name().ok_or_else(|| format!("Invalid path: {p}"))?;
    Ok(resolved_parent.join(file_name))
}

fn run_bash(command: &str) -> String {
    if DANGEROUS.iter().any(|d| command.contains(d)) {
        return "Error: Dangerous command blocked".into();
    }
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(o) => {
            let mut out = String::from_utf8_lossy(&o.stdout).to_string();
            out.push_str(&String::from_utf8_lossy(&o.stderr));
            let out = out.trim().to_string();
            if out.is_empty() { "(no output)".into() } else { out.chars().take(50000).collect() }
        }
        Err(e) => format!("Error: {e}"),
    }
}

fn run_read(workdir: &Path, path: &str, limit: Option<i64>) -> String {
    let fp = match safe_path(workdir, path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match fs::read_to_string(&fp) {
        Ok(text) => {
            let lines: Vec<&str> = text.lines().collect();
            if let Some(lim) = limit {
                let lim = lim as usize;
                if lim < lines.len() {
                    let mut out = lines[..lim].join("\n");
                    out.push_str(&format!("\n... ({} more lines)", lines.len() - lim));
                    return out.chars().take(50000).collect();
                }
            }
            text.chars().take(50000).collect()
        }
        Err(e) => format!("Error: {e}"),
    }
}

fn run_write(workdir: &Path, path: &str, content: &str) -> String {
    let fp = match safe_path(workdir, path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if let Some(parent) = fp.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return format!("Error: {e}");
        }
    }
    match fs::write(&fp, content) {
        Ok(()) => format!("Wrote {} bytes to {path}", content.len()),
        Err(e) => format!("Error: {e}"),
    }
}

fn run_edit(workdir: &Path, path: &str, old_text: &str, new_text: &str) -> String {
    let fp = match safe_path(workdir, path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match fs::read_to_string(&fp) {
        Ok(content) => {
            if !content.contains(old_text) {
                return format!("Error: Text not found in {path}");
            }
            let new_content = content.replacen(old_text, new_text, 1);
            match fs::write(&fp, new_content) {
                Ok(()) => format!("Edited {path}"),
                Err(e) => format!("Error: {e}"),
            }
        }
        Err(e) => format!("Error: {e}"),
    }
}

fn bash_tool() -> Tool {
    Tool {
        name: "bash".into(),
        description: Some("Run a shell command.".into()),
        input_schema: json!({
            "type": "object",
            "properties": { "command": { "type": "string" } },
            "required": ["command"]
        }),
    }
}

fn read_file_tool() -> Tool {
    Tool {
        name: "read_file".into(),
        description: Some("Read file contents. Use relative paths.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "limit": { "type": "integer" }
            },
            "required": ["path"]
        }),
    }
}

fn write_file_tool() -> Tool {
    Tool {
        name: "write_file".into(),
        description: Some("Write content to file. Use relative paths.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        }),
    }
}

fn edit_file_tool() -> Tool {
    Tool {
        name: "edit_file".into(),
        description: Some("Replace exact text in file. Use relative paths.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_text": { "type": "string" },
                "new_text": { "type": "string" }
            },
            "required": ["path", "old_text", "new_text"]
        }),
    }
}

fn child_tools() -> Vec<Tool> {
    vec![bash_tool(), read_file_tool(), write_file_tool(), edit_file_tool()]
}

fn task_tool() -> Tool {
    Tool {
        name: "task".into(),
        description: Some("Spawn a subagent with fresh context. It shares the filesystem but not conversation history.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string" },
                "description": { "type": "string", "description": "Short description of the task" }
            },
            "required": ["prompt"]
        }),
    }
}

fn todo_tool() -> Tool {
    Tool {
        name: "todo".into(),
        description: Some("Update task list. Track progress on multi-step tasks.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "text": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                        },
                        "required": ["id", "text", "status"]
                    }
                }
            },
            "required": ["items"]
        }),
    }
}

fn load_skill_tool() -> Tool {
    Tool {
        name: "load_skill".into(),
        description: Some("Load specialized knowledge by name.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Skill name to load" }
            },
            "required": ["name"]
        }),
    }
}

fn compact_tool() -> Tool {
    Tool {
        name: "compact".into(),
        description: Some("Trigger manual conversation compression.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "focus": { "type": "string", "description": "What to preserve in the summary" }
            }
        }),
    }
}

async fn agent_loop(client: &AnthropicClient, model: &str, system: &str, subagent_system: &str, workdir: &Path, history: &mut Vec<Message>, todo: &mut TodoManager, skill_loader: &SkillLoader) {
    let mut rounds_since_todo: u32 = 0;
    loop {
        // Layer 1: micro_compact before each LLM call
        micro_compact(history);
        // Layer 2: auto_compact if token estimate exceeds threshold
        if estimate_tokens(history) > THRESHOLD {
            println!("[auto_compact triggered]");
            *history = auto_compact(client, model, workdir, history).await;
        }

        let params = CreateMessageParams::new(RequiredMessageParams {
            model: model.to_string(),
            messages: history.clone(),
            max_tokens: 8000,
        })
        .with_system(system)
        .with_tools(vec![bash_tool(), read_file_tool(), write_file_tool(), edit_file_tool(), todo_tool(), task_tool(), load_skill_tool(), compact_tool()]);

        let response = match client.create_message(Some(&params)).await {
            Ok(r) => r,
            Err(e) => { eprintln!("API error: {e}"); return; }
        };

        // Append assistant turn
        history.push(Message::new_blocks(Role::Assistant, response.content.clone()));

        // If no tool_use, done
        if !matches!(response.stop_reason, Some(StopReason::ToolUse)) {
            return;
        }

        // Execute tools, collect results
        let mut results = Vec::new();
        let mut used_todo = false;
        let mut manual_compact = false;
        for block in &response.content {
            if let ContentBlock::ToolUse { id, name, input } = block {
                let output = match name.as_str() {
                    "bash" => {
                        let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("");
                        println!("\x1b[33m$ {cmd}\x1b[0m");
                        run_bash(cmd)
                    }
                    "read_file" => {
                        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let limit = input.get("limit").and_then(|v| v.as_i64());
                        println!("\x1b[33m> read_file: {path}\x1b[0m");
                        run_read(workdir, path, limit)
                    }
                    "write_file" => {
                        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        println!("\x1b[33m> write_file: {path}\x1b[0m");
                        run_write(workdir, path, content)
                    }
                    "edit_file" => {
                        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let old_text = input.get("old_text").and_then(|v| v.as_str()).unwrap_or("");
                        let new_text = input.get("new_text").and_then(|v| v.as_str()).unwrap_or("");
                        println!("\x1b[33m> edit_file: {path}\x1b[0m");
                        run_edit(workdir, path, old_text, new_text)
                    }
                    "todo" => {
                        used_todo = true;
                        let items = input.get("items").and_then(|v| v.as_array());
                        println!("\x1b[33m> todo\x1b[0m");
                        match items {
                            Some(arr) => match todo.update(arr) {
                                Ok(rendered) => rendered,
                                Err(e) => format!("Error: {e}"),
                            },
                            None => "Error: items required".into(),
                        }
                    }
                    "task" => {
                        let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                        let desc = input.get("description").and_then(|v| v.as_str()).unwrap_or("subtask");
                        println!("\x1b[33m> task ({desc}): {}\x1b[0m", &prompt.chars().take(80).collect::<String>());
                        run_subagent(client, model, subagent_system, workdir, prompt).await
                    }
                    "load_skill" => {
                        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        println!("\x1b[33m> load_skill: {name}\x1b[0m");
                        skill_loader.get_content(name)
                    }
                    "compact" => {
                        manual_compact = true;
                        println!("\x1b[33m> compact\x1b[0m");
                        "Compressing...".into()
                    }
                    other => format!("Unknown tool: {other}"),
                };
                let preview: String = output.chars().take(200).collect();
                println!("{preview}");
                results.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: output,
                });
            }
        }
        rounds_since_todo = if used_todo { 0 } else { rounds_since_todo + 1 };
        // Nag reminder: nudge the model to update todos if it hasn't recently
        if rounds_since_todo >= 3 {
            results.insert(0, ContentBlock::Text {
                text: "<reminder>Update your todos.</reminder>".into(),
            });
        }
        history.push(Message::new_blocks(Role::User, results));
        // Layer 3: manual compact triggered by the compact tool
        if manual_compact {
            println!("[manual compact]");
            *history = auto_compact(client, model, workdir, history).await;
        }
    }
}

async fn run_subagent(client: &AnthropicClient, model: &str, subagent_system: &str, workdir: &Path, prompt: &str) -> String {
    let mut messages = vec![Message::new_text(Role::User, prompt)];
    for _ in 0..30 {
        let params = CreateMessageParams::new(RequiredMessageParams {
            model: model.to_string(),
            messages: messages.clone(),
            max_tokens: 8000,
        })
        .with_system(subagent_system)
        .with_tools(child_tools());

        let response = match client.create_message(Some(&params)).await {
            Ok(r) => r,
            Err(e) => return format!("Subagent API error: {e}"),
        };
        messages.push(Message::new_blocks(Role::Assistant, response.content.clone()));
        if !matches!(response.stop_reason, Some(StopReason::ToolUse)) {
            break;
        }
        let mut results = Vec::new();
        for block in &response.content {
            if let ContentBlock::ToolUse { id, name, input } = block {
                let output = match name.as_str() {
                    "bash" => {
                        let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("");
                        println!("\x1b[35m  sub$ {cmd}\x1b[0m");
                        run_bash(cmd)
                    }
                    "read_file" => {
                        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let limit = input.get("limit").and_then(|v| v.as_i64());
                        println!("\x1b[35m  sub> read_file: {path}\x1b[0m");
                        run_read(workdir, path, limit)
                    }
                    "write_file" => {
                        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        println!("\x1b[35m  sub> write_file: {path}\x1b[0m");
                        run_write(workdir, path, content)
                    }
                    "edit_file" => {
                        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let old_text = input.get("old_text").and_then(|v| v.as_str()).unwrap_or("");
                        let new_text = input.get("new_text").and_then(|v| v.as_str()).unwrap_or("");
                        println!("\x1b[35m  sub> edit_file: {path}\x1b[0m");
                        run_edit(workdir, path, old_text, new_text)
                    }
                    other => format!("Unknown tool: {other}"),
                };
                results.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: output.chars().take(50000).collect(),
                });
            }
        }
        messages.push(Message::new_blocks(Role::User, results));
    }
    // Extract final text from last assistant message
    if let Some(last) = messages.iter().rev().find(|m| matches!(m.content, MessageContent::Blocks { .. })) {
        if let MessageContent::Blocks { content } = &last.content {
            let text: String = content.iter().filter_map(|b| {
                if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
            }).collect();
            if !text.is_empty() {
                return text;
            }
        }
    }
    "(no summary)".into()
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let api_key = env::var("ANTHROPIC_API_KEY")
        .or_else(|_| env::var("ANTHROPIC_AUTH_TOKEN"))
        .expect("ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN not set (check .env)");
    let base_url = env::var("ANTHROPIC_BASE_URL").ok();
    let api_version = env::var("ANTHROPIC_API_VERSION").unwrap_or_else(|_| "2023-06-01".into());
    let model = env::var("MODEL_ID").unwrap_or_else(|_| "claude-sonnet-4-20250514".into());
    let cwd = env::current_dir().unwrap().display().to_string();
    let skill_loader = SkillLoader::new(&env::current_dir().unwrap().join("skills"));
    let system = format!("You are a coding agent at {cwd}.\nUse the todo tool to plan multi-step tasks. Mark in_progress before starting, completed when done.\nUse the task tool to delegate exploration or subtasks to a subagent.\nUse load_skill to access specialized knowledge before tackling unfamiliar topics.\nAll file paths must be relative to the working directory. Do not use absolute paths.\nPrefer tools over prose.\n\nSkills available:\n{}", skill_loader.get_descriptions());
    let subagent_system = format!("You are a coding subagent at {cwd}. Complete the given task, then summarize your findings.");
    let workdir = env::current_dir().unwrap();

    let client: AnthropicClient = match base_url {
        Some(url) => {
            let url = if url.ends_with("/v1") { url } else { format!("{url}/v1") };
            AnthropicClient::builder(api_key, &api_version)
                .with_api_base_url(url)
                .build::<MessageError>()
                .expect("failed to create client")
        }
        None => AnthropicClient::new::<MessageError>(api_key, &api_version)
            .expect("failed to create client"),
    };

    let mut history: Vec<Message> = Vec::new();
    let mut todo = TodoManager::new();
    let stdin = io::stdin();

    loop {
        print!("\x1b[36ms06 >> \x1b[0m");
        io::stdout().flush().unwrap();

        let mut query = String::new();
        if stdin.lock().read_line(&mut query).unwrap() == 0 { break; }
        let query = query.trim();
        if query.is_empty() || query == "q" || query == "exit" { break; }

        history.push(Message::new_text(Role::User, query));
        agent_loop(&client, &model, &system, &subagent_system, &workdir, &mut history, &mut todo, &skill_loader).await;

        // Print final text response
        if let Some(last) = history.last() {
            if let MessageContent::Blocks { content } = &last.content {
                for block in content {
                    if let ContentBlock::Text { text } = block {
                        println!("{text}");
                    }
                }
            }
        }
        println!();
    }
}
