use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use anthropic_ai_sdk::client::AnthropicClient;
use anthropic_ai_sdk::types::message::{
    ContentBlock, CreateMessageParams, Message, MessageClient, MessageContent, MessageError,
    RequiredMessageParams, Role, StopReason, Tool,
};
use serde_json::json;

const DANGEROUS: &[&str] = &["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];
const THRESHOLD: usize = 100000;
const KEEP_RECENT: usize = 3;

// -- TodoManager: structured state the LLM writes to --
struct TodoItem {
    content: String,
    status: String, // "pending" | "in_progress" | "completed"
    active_form: Option<String>,
}

struct TodoManager {
    items: Vec<TodoItem>,
}

impl TodoManager {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn has_open_items(&self) -> bool {
        self.items.iter().any(|i| i.status != "completed")
    }

    fn update(&mut self, items: &[serde_json::Value]) -> Result<String, String> {
        if items.len() > 20 {
            return Err("Max 20 todos allowed".into());
        }
        let mut validated = Vec::new();
        let mut in_progress_count = 0;
        for (i, item) in items.iter().enumerate() {
            let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_lowercase();
            let active_form = item.get("activeForm").and_then(|v| v.as_str()).map(|s| s.to_string());
            if content.is_empty() {
                return Err(format!("Item {}: content required", i + 1));
            }
            if !["pending", "in_progress", "completed"].contains(&status.as_str()) {
                return Err(format!("Item {}: invalid status '{status}'", i + 1));
            }
            if status == "in_progress" {
                in_progress_count += 1;
            }
            validated.push(TodoItem { content, status, active_form });
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
        let mut lines: Vec<String> = self.items.iter().enumerate().map(|(i, item)| {
            let marker = match item.status.as_str() {
                "pending" => "[ ]",
                "in_progress" => "[>]",
                "completed" => "[x]",
                _ => "[ ]",
            };
            format!("{marker} #{}: {}", i + 1, item.content)
        }).collect();
        let done = self.items.iter().filter(|t| t.status == "completed").count();
        lines.push(format!("\n({done}/{} completed)", self.items.len()));
        lines.join("\n")
    }
}

// -- TaskManager: file-persisted task system --
#[derive(Clone)]
struct Task {
    id: u32,
    subject: String,
    description: String,
    status: String,
    owner: Option<String>,
    blocked_by: Vec<u32>,
    blocks: Vec<u32>,
}

impl Task {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "id": self.id, "subject": self.subject, "description": self.description,
            "status": self.status, "owner": self.owner,
            "blockedBy": self.blocked_by, "blocks": self.blocks
        })
    }
    fn from_json(v: &serde_json::Value) -> Option<Self> {
        Some(Task {
            id: v.get("id")?.as_u64()? as u32,
            subject: v.get("subject")?.as_str()?.to_string(),
            description: v.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
            status: v.get("status")?.as_str()?.to_string(),
            owner: v.get("owner").and_then(|o| o.as_str()).map(|s| s.to_string()),
            blocked_by: v.get("blockedBy").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default(),
            blocks: v.get("blocks").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default(),
        })
    }
}

struct TaskManager { dir: PathBuf }

impl TaskManager {
    fn new(workdir: &Path) -> Self {
        let dir = workdir.join(".tasks");
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }
    fn _path(&self, id: u32) -> PathBuf { self.dir.join(format!("task_{id}.json")) }
    fn _next_id(&self) -> u32 {
        let mut max = 0u32;
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(n) = name.strip_prefix("task_").and_then(|s| s.strip_suffix(".json")).and_then(|s| s.parse::<u32>().ok()) {
                    if n > max { max = n; }
                }
            }
        }
        max + 1
    }
    fn _load(&self, id: u32) -> Option<Task> {
        let data = fs::read_to_string(self._path(id)).ok()?;
        let v: serde_json::Value = serde_json::from_str(&data).ok()?;
        Task::from_json(&v)
    }
    fn _save(&self, task: &Task) {
        let _ = fs::write(self._path(task.id), serde_json::to_string_pretty(&task.to_json()).unwrap_or_default());
    }
    fn create(&self, subject: &str, description: &str) -> Task {
        let id = self._next_id();
        let task = Task { id, subject: subject.to_string(), description: description.to_string(), status: "pending".into(), owner: None, blocked_by: vec![], blocks: vec![] };
        self._save(&task);
        task
    }
    fn get(&self, id: u32) -> Result<Task, String> {
        self._load(id).ok_or_else(|| format!("Task {id} not found"))
    }
    fn update(&self, id: u32, status: Option<&str>, add_blocked_by: &[u32], add_blocks: &[u32]) -> Result<Task, String> {
        let mut task = self.get(id)?;
        if let Some(s) = status {
            if s == "deleted" {
                let _ = fs::remove_file(self._path(id));
                return Ok(Task { id, subject: task.subject, description: task.description, status: "deleted".into(), owner: task.owner, blocked_by: vec![], blocks: vec![] });
            }
            task.status = s.to_string();
            if s == "completed" {
                // Remove this id from all other tasks' blockedBy
                if let Ok(entries) = fs::read_dir(&self.dir) {
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if let Some(oid) = name.strip_prefix("task_").and_then(|s| s.strip_suffix(".json")).and_then(|s| s.parse::<u32>().ok()) {
                            if oid != id {
                                if let Some(mut other) = self._load(oid) {
                                    if other.blocked_by.contains(&id) {
                                        other.blocked_by.retain(|&x| x != id);
                                        self._save(&other);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for &b in add_blocked_by { if !task.blocked_by.contains(&b) { task.blocked_by.push(b); } }
        for &b in add_blocks { if !task.blocks.contains(&b) { task.blocks.push(b); } }
        self._save(&task);
        Ok(task)
    }
    fn list_all(&self) -> String {
        let mut tasks: Vec<Task> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            let mut files: Vec<_> = entries.flatten().collect();
            files.sort_by_key(|e| e.file_name());
            for e in files {
                if let Some(t) = fs::read_to_string(e.path()).ok().and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok()).and_then(|v| Task::from_json(&v)) {
                    tasks.push(t);
                }
            }
        }
        if tasks.is_empty() { return "No tasks.".into(); }
        tasks.iter().map(|t| {
            let marker = match t.status.as_str() { "pending" => "[ ]", "in_progress" => "[>]", "completed" => "[x]", _ => "[ ]" };
            let owner = t.owner.as_deref().unwrap_or("unassigned");
            let blocked = if t.blocked_by.is_empty() { String::new() } else { format!(" (blocked by: {:?})", t.blocked_by) };
            format!("{marker} #{}: {} @{owner}{blocked}", t.id, t.subject)
        }).collect::<Vec<_>>().join("\n")
    }
    fn claim(&self, id: u32, owner: &str) -> Result<Task, String> {
        let mut task = self.get(id)?;
        task.owner = Some(owner.to_string());
        task.status = "in_progress".into();
        self._save(&task);
        Ok(task)
    }
}

// -- BackgroundManager: run commands in background tokio tasks --
struct BgTask {
    id: String,
    command: String,
    status: String, // "running" | "completed" | "error"
    result: Option<String>,
}

struct BackgroundManager {
    tasks: HashMap<String, BgTask>,
    notify_tx: mpsc::UnboundedSender<String>,
    notify_rx: Option<mpsc::UnboundedReceiver<String>>,
}

impl BackgroundManager {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tasks: HashMap::new(), notify_tx: tx, notify_rx: Some(rx) }
    }
    fn take_rx(&mut self) -> mpsc::UnboundedReceiver<String> {
        self.notify_rx.take().expect("rx already taken")
    }
    fn run(&mut self, command: &str, _timeout: u64) -> String {
        let id = Uuid::new_v4().to_string()[..8].to_string();
        self.tasks.insert(id.clone(), BgTask { id: id.clone(), command: command.to_string(), status: "running".into(), result: None });
        let cmd = command.to_string();
        let task_id = id.clone();
        let tx = self.notify_tx.clone();
        // We need a way to update the task status — use a shared ref
        // For simplicity, we'll capture the result via the notify channel
        tokio::spawn(async move {
            let output = tokio::task::spawn_blocking(move || run_bash(&cmd)).await.unwrap_or_else(|e| format!("Error: {e}"));
            let _ = tx.send(format!("{}|{}", task_id, output));
        });
        format!("Background task {id} started: {command}")
    }
    fn check(&self, task_id: Option<&str>) -> String {
        match task_id {
            Some(tid) => match self.tasks.get(tid) {
                Some(t) => format!("[{}] {}", t.status, t.result.as_deref().unwrap_or("(running)")),
                None => format!("Unknown background task: {tid}"),
            },
            None => {
                if self.tasks.is_empty() { return "No background tasks.".into(); }
                self.tasks.values().map(|t| format!("{}: [{}] {}", t.id, t.status, t.command)).collect::<Vec<_>>().join("\n")
            }
        }
    }
    fn drain(&mut self, rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<String> {
        let mut notifications = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            if let Some((tid, result)) = msg.split_once('|') {
                if let Some(task) = self.tasks.get_mut(tid) {
                    task.status = "completed".into();
                    task.result = Some(result.to_string());
                }
                notifications.push(format!("Background task {tid} completed: {}", result.chars().take(200).collect::<String>()));
            }
        }
        notifications
    }
}

// -- MessageBus: file-based JSONL messaging --
struct MessageBus {
    inbox_dir: PathBuf,
}

impl MessageBus {
    fn new(workdir: &Path) -> Self {
        let inbox_dir = workdir.join(".team").join("inbox");
        let _ = fs::create_dir_all(&inbox_dir);
        Self { inbox_dir }
    }
    fn send(&self, to: &str, from: &str, content: &str, msg_type: &str) -> String {
        let msg = json!({
            "type": msg_type, "from": from, "content": content,
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
        });
        let path = self.inbox_dir.join(format!("{to}.jsonl"));
        let mut file = fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
        let _ = writeln!(file, "{}", serde_json::to_string(&msg).unwrap_or_default());
        format!("Sent message to {to}")
    }
    fn read_inbox(&self, name: &str) -> String {
        let path = self.inbox_dir.join(format!("{name}.jsonl"));
        if !path.exists() { return "[]".into(); }
        let data = fs::read_to_string(&path).unwrap_or_default();
        let _ = fs::write(&path, ""); // clear
        let msgs: Vec<serde_json::Value> = data.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        serde_json::to_string(&msgs).unwrap_or_else(|_| "[]".into())
    }
    fn broadcast(&self, from: &str, content: &str, members: &[String]) -> String {
        let mut count = 0;
        for m in members {
            if m != from {
                self.send(m, from, content, "broadcast");
                count += 1;
            }
        }
        format!("Broadcast to {count} teammates")
    }
}

// -- TeammateManager: multi-agent collaboration --
#[derive(Clone)]
struct TeamMember {
    name: String,
    role: String,
    status: String, // "working" | "idle" | "shutdown"
}

struct TeammateManager {
    team_name: String,
    members: Vec<TeamMember>,
    config_path: PathBuf,
}

impl TeammateManager {
    fn new(workdir: &Path) -> Self {
        let team_dir = workdir.join(".team");
        let _ = fs::create_dir_all(&team_dir);
        let config_path = team_dir.join("config.json");
        let (team_name, members) = if config_path.exists() {
            if let Ok(data) = fs::read_to_string(&config_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                    let name = v.get("team_name").and_then(|n| n.as_str()).unwrap_or("default").to_string();
                    let mems = v.get("members").and_then(|m| m.as_array()).map(|arr| {
                        arr.iter().filter_map(|m| {
                            Some(TeamMember {
                                name: m.get("name")?.as_str()?.to_string(),
                                role: m.get("role")?.as_str()?.to_string(),
                                status: m.get("status").and_then(|s| s.as_str()).unwrap_or("shutdown").to_string(),
                            })
                        }).collect()
                    }).unwrap_or_default();
                    (name, mems)
                } else { ("default".into(), vec![]) }
            } else { ("default".into(), vec![]) }
        } else { ("default".into(), vec![]) };
        let mgr = Self { team_name, members, config_path };
        mgr._save_config();
        mgr
    }
    fn _save_config(&self) {
        let members: Vec<serde_json::Value> = self.members.iter().map(|m| json!({"name": m.name, "role": m.role, "status": m.status})).collect();
        let config = json!({"team_name": self.team_name, "members": members});
        let _ = fs::write(&self.config_path, serde_json::to_string_pretty(&config).unwrap_or_default());
    }
    fn member_names(&self) -> Vec<String> {
        self.members.iter().map(|m| m.name.clone()).collect()
    }
    fn list_all(&self) -> String {
        if self.members.is_empty() { return format!("Team: {} (no members)", self.team_name); }
        let mut lines = vec![format!("Team: {}", self.team_name)];
        for m in &self.members {
            lines.push(format!("  {} ({}): {}", m.name, m.role, m.status));
        }
        lines.join("\n")
    }
    fn set_status(&mut self, name: &str, status: &str) {
        if let Some(m) = self.members.iter_mut().find(|m| m.name == name) {
            m.status = status.to_string();
            self._save_config();
        }
    }
    fn spawn(&mut self, name: &str, role: &str) -> Result<(), String> {
        if let Some(m) = self.members.iter().find(|m| m.name == name) {
            if m.status == "working" {
                return Err(format!("Error: '{}' is currently working", name));
            }
        }
        // Update or add member
        if let Some(m) = self.members.iter_mut().find(|m| m.name == name) {
            m.role = role.to_string();
            m.status = "working".into();
        } else {
            self.members.push(TeamMember { name: name.to_string(), role: role.to_string(), status: "working".into() });
        }
        self._save_config();
        Ok(())
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
                "description": { "type": "string", "description": "Short description of the task" },
                "agent_type": { "type": "string", "enum": ["Explore", "general-purpose"], "default": "Explore" }
            },
            "required": ["prompt"]
        }),
    }
}

fn todo_write_tool() -> Tool {
    Tool {
        name: "TodoWrite".into(),
        description: Some("Update task list. Track progress on multi-step tasks.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] },
                            "activeForm": { "type": "string" }
                        },
                        "required": ["content", "status"]
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

fn task_create_tool() -> Tool {
    Tool {
        name: "task_create".into(),
        description: Some("Create a new persistent task.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string" },
                "description": { "type": "string" }
            },
            "required": ["subject"]
        }),
    }
}

fn task_get_tool() -> Tool {
    Tool {
        name: "task_get".into(),
        description: Some("Get a task by ID.".into()),
        input_schema: json!({
            "type": "object",
            "properties": { "task_id": { "type": "integer" } },
            "required": ["task_id"]
        }),
    }
}

fn task_update_tool() -> Tool {
    Tool {
        name: "task_update".into(),
        description: Some("Update task status or dependencies.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "integer" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"] },
                "add_blocked_by": { "type": "array", "items": { "type": "integer" } },
                "add_blocks": { "type": "array", "items": { "type": "integer" } }
            },
            "required": ["task_id"]
        }),
    }
}

fn task_list_tool() -> Tool {
    Tool {
        name: "task_list".into(),
        description: Some("List all persistent tasks.".into()),
        input_schema: json!({ "type": "object", "properties": {} }),
    }
}

fn claim_task_tool() -> Tool {
    Tool {
        name: "claim_task".into(),
        description: Some("Claim a task by ID, setting owner and status to in_progress.".into()),
        input_schema: json!({
            "type": "object",
            "properties": { "task_id": { "type": "integer" } },
            "required": ["task_id"]
        }),
    }
}

fn background_run_tool() -> Tool {
    Tool {
        name: "background_run".into(),
        description: Some("Run a shell command in the background.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" },
                "timeout": { "type": "integer", "description": "Timeout in seconds (default 120)" }
            },
            "required": ["command"]
        }),
    }
}

fn check_background_tool() -> Tool {
    Tool {
        name: "check_background".into(),
        description: Some("Check status of background tasks.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Optional specific task ID" }
            }
        }),
    }
}

fn send_message_tool() -> Tool {
    Tool {
        name: "send_message".into(),
        description: Some("Send a message to a teammate.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "to": { "type": "string" },
                "content": { "type": "string" },
                "msg_type": { "type": "string", "default": "message" }
            },
            "required": ["to", "content"]
        }),
    }
}

fn read_inbox_tool() -> Tool {
    Tool {
        name: "read_inbox".into(),
        description: Some("Read and clear your inbox.".into()),
        input_schema: json!({ "type": "object", "properties": {} }),
    }
}

fn broadcast_tool() -> Tool {
    Tool {
        name: "broadcast".into(),
        description: Some("Broadcast a message to all teammates.".into()),
        input_schema: json!({
            "type": "object",
            "properties": { "content": { "type": "string" } },
            "required": ["content"]
        }),
    }
}

fn spawn_teammate_tool() -> Tool {
    Tool {
        name: "spawn_teammate".into(),
        description: Some("Spawn an autonomous teammate agent.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "role": { "type": "string" },
                "prompt": { "type": "string" }
            },
            "required": ["name", "role", "prompt"]
        }),
    }
}

fn list_teammates_tool() -> Tool {
    Tool {
        name: "list_teammates".into(),
        description: Some("List all teammates and their status.".into()),
        input_schema: json!({ "type": "object", "properties": {} }),
    }
}

fn idle_tool() -> Tool {
    Tool {
        name: "idle".into(),
        description: Some("Enter idle mode. Will auto-claim tasks or respond to messages.".into()),
        input_schema: json!({ "type": "object", "properties": {} }),
    }
}

fn shutdown_request_tool() -> Tool {
    Tool {
        name: "shutdown_request".into(),
        description: Some("Request a teammate to shut down.".into()),
        input_schema: json!({
            "type": "object",
            "properties": { "teammate": { "type": "string" } },
            "required": ["teammate"]
        }),
    }
}

fn plan_approval_tool() -> Tool {
    Tool {
        name: "plan_approval".into(),
        description: Some("Approve or reject a plan request.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "request_id": { "type": "string" },
                "approve": { "type": "boolean" },
                "feedback": { "type": "string" }
            },
            "required": ["request_id", "approve"]
        }),
    }
}

// -- Teammate agent loop --
const POLL_INTERVAL_SECS: u64 = 5;
const IDLE_TIMEOUT_SECS: u64 = 60;

fn teammate_tools() -> Vec<Tool> {
    vec![bash_tool(), read_file_tool(), write_file_tool(), edit_file_tool(), send_message_tool(), idle_tool(), claim_task_tool()]
}

async fn teammate_loop(
    client: AnthropicClient, model: String, workdir: PathBuf,
    name: String, role: String, prompt: String,
    msg_bus: Arc<Mutex<MessageBus>>, task_mgr: Arc<Mutex<TaskManager>>,
    team_mgr: Arc<Mutex<TeammateManager>>,
) {
    let system = format!("You are '{name}', a teammate agent (role: {role}). Work directory: {}.\nYou have bash, read_file, write_file, edit_file, send_message, idle, claim_task tools.\nWhen done with your current work, call idle to wait for more tasks.\nSend results to 'lead' via send_message.", workdir.display());
    let mut messages = vec![Message::new_text(Role::User, &prompt)];

    for _ in 0..50 {
        // Check inbox for shutdown
        {
            let bus = msg_bus.lock().await;
            let inbox = bus.read_inbox(&name);
            if inbox.contains("shutdown_request") {
                team_mgr.lock().await.set_status(&name, "shutdown");
                return;
            }
            if inbox != "[]" {
                messages.push(Message::new_text(Role::User, &format!("<inbox>{inbox}</inbox>")));
                messages.push(Message::new_text(Role::Assistant, "Noted inbox messages."));
            }
        }

        let params = CreateMessageParams::new(RequiredMessageParams {
            model: model.clone(), messages: messages.clone(), max_tokens: 8000,
        }).with_system(&system).with_tools(teammate_tools());

        let response = match client.create_message(Some(&params)).await {
            Ok(r) => r,
            Err(_) => break,
        };
        messages.push(Message::new_blocks(Role::Assistant, response.content.clone()));
        if !matches!(response.stop_reason, Some(StopReason::ToolUse)) { break; }

        let mut results = Vec::new();
        let mut wants_idle = false;
        for block in &response.content {
            if let ContentBlock::ToolUse { id, name: tool_name, input } = block {
                let output = match tool_name.as_str() {
                    "bash" => { let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or(""); println!("\x1b[35m  [{name}]$ {cmd}\x1b[0m"); run_bash(cmd) }
                    "read_file" => { let p = input.get("path").and_then(|v| v.as_str()).unwrap_or(""); run_read(&workdir, p, input.get("limit").and_then(|v| v.as_i64())) }
                    "write_file" => { let p = input.get("path").and_then(|v| v.as_str()).unwrap_or(""); let c = input.get("content").and_then(|v| v.as_str()).unwrap_or(""); run_write(&workdir, p, c) }
                    "edit_file" => { let p = input.get("path").and_then(|v| v.as_str()).unwrap_or(""); run_edit(&workdir, p, input.get("old_text").and_then(|v| v.as_str()).unwrap_or(""), input.get("new_text").and_then(|v| v.as_str()).unwrap_or("")) }
                    "send_message" => { let to = input.get("to").and_then(|v| v.as_str()).unwrap_or(""); let content = input.get("content").and_then(|v| v.as_str()).unwrap_or(""); let mt = input.get("msg_type").and_then(|v| v.as_str()).unwrap_or("message"); msg_bus.lock().await.send(to, &name, content, mt) }
                    "claim_task" => { let tid = input.get("task_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32; match task_mgr.lock().await.claim(tid, &name) { Ok(t) => serde_json::to_string(&t.to_json()).unwrap_or_default(), Err(e) => e } }
                    "idle" => { wants_idle = true; "Entering idle mode.".into() }
                    other => format!("Unknown tool: {other}"),
                };
                results.push(ContentBlock::ToolResult { tool_use_id: id.clone(), content: output });
            }
        }
        messages.push(Message::new_blocks(Role::User, results));

        if wants_idle {
            // Idle phase
            team_mgr.lock().await.set_status(&name, "idle");
            let start = SystemTime::now();
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
                let elapsed = start.elapsed().map(|d| d.as_secs()).unwrap_or(0);
                if elapsed > IDLE_TIMEOUT_SECS {
                    team_mgr.lock().await.set_status(&name, "shutdown");
                    return;
                }
                // Check inbox
                let inbox = msg_bus.lock().await.read_inbox(&name);
                if inbox.contains("shutdown_request") {
                    team_mgr.lock().await.set_status(&name, "shutdown");
                    return;
                }
                if inbox != "[]" {
                    team_mgr.lock().await.set_status(&name, "working");
                    messages.push(Message::new_text(Role::User, &format!("<inbox>{inbox}</inbox>")));
                    messages.push(Message::new_text(Role::Assistant, "Resuming work from inbox."));
                    break;
                }
                // Auto-claim: find pending task with no owner and no blockers
                let task_mgr_lock = task_mgr.lock().await;
                let mut claimed = false;
                if let Ok(entries) = fs::read_dir(&task_mgr_lock.dir) {
                    for e in entries.flatten() {
                        if let Some(t) = fs::read_to_string(e.path()).ok().and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok()).and_then(|v| Task::from_json(&v)) {
                            if t.status == "pending" && t.owner.is_none() && t.blocked_by.is_empty() {
                                drop(task_mgr_lock);
                                let _ = task_mgr.lock().await.claim(t.id, &name);
                                let claim_msg = format!("<auto-claimed>Task #{}: {}</auto-claimed>", t.id, t.subject);
                                // Identity re-injection if messages are short
                                if messages.len() <= 3 {
                                    messages.insert(0, Message::new_text(Role::User, &format!("<identity>You are '{name}', role: {role}</identity>")));
                                    messages.insert(1, Message::new_text(Role::Assistant, "Understood, continuing as my role."));
                                }
                                messages.push(Message::new_text(Role::User, &claim_msg));
                                messages.push(Message::new_text(Role::Assistant, &format!("Working on task #{}: {}", t.id, t.subject)));
                                team_mgr.lock().await.set_status(&name, "working");
                                claimed = true;
                                break;
                            }
                        }
                    }
                    if claimed { break; }
                } else {
                    drop(task_mgr_lock);
                }
            }
        }
    }
    team_mgr.lock().await.set_status(&name, "shutdown");
}

async fn agent_loop(
    client: &AnthropicClient, model: &str, system: &str, subagent_system: &str, workdir: &Path,
    history: &mut Vec<Message>, todo: &mut TodoManager, skill_loader: &SkillLoader,
    task_mgr: &Arc<Mutex<TaskManager>>, bg_mgr: &Arc<Mutex<BackgroundManager>>,
    bg_rx: &mut mpsc::UnboundedReceiver<String>,
    msg_bus: &Arc<Mutex<MessageBus>>, team_mgr: &Arc<Mutex<TeammateManager>>,
    shutdown_requests: &mut HashMap<String, serde_json::Value>,
    plan_requests: &mut HashMap<String, serde_json::Value>,
) {
    let mut rounds_since_todo: u32 = 0;
    loop {
        // Drain background notifications
        let notifications = bg_mgr.lock().await.drain(bg_rx);
        if !notifications.is_empty() {
            let bg_text = format!("<background-results>{}</background-results>", notifications.join("\n"));
            history.push(Message::new_text(Role::User, &bg_text));
            history.push(Message::new_text(Role::Assistant, "Noted background results."));
        }
        // Check lead inbox
        let inbox = msg_bus.lock().await.read_inbox("lead");
        if inbox != "[]" {
            history.push(Message::new_text(Role::User, &format!("<inbox>{inbox}</inbox>")));
            history.push(Message::new_text(Role::Assistant, "Noted inbox messages."));
        }
        // Layer 1: micro_compact
        micro_compact(history);
        // Layer 2: auto_compact
        if estimate_tokens(history) > THRESHOLD {
            println!("[auto_compact triggered]");
            *history = auto_compact(client, model, workdir, history).await;
        }

        let all_tools = vec![
            bash_tool(), read_file_tool(), write_file_tool(), edit_file_tool(),
            todo_write_tool(), task_tool(), load_skill_tool(), compact_tool(),
            task_create_tool(), task_get_tool(), task_update_tool(), task_list_tool(), claim_task_tool(),
            background_run_tool(), check_background_tool(),
            send_message_tool(), read_inbox_tool(), broadcast_tool(),
            spawn_teammate_tool(), list_teammates_tool(),
            shutdown_request_tool(), plan_approval_tool(),
        ];

        let params = CreateMessageParams::new(RequiredMessageParams {
            model: model.to_string(), messages: history.clone(), max_tokens: 8000,
        }).with_system(system).with_tools(all_tools);

        let response = match client.create_message(Some(&params)).await {
            Ok(r) => r,
            Err(e) => { eprintln!("API error: {e}"); return; }
        };
        history.push(Message::new_blocks(Role::Assistant, response.content.clone()));
        if !matches!(response.stop_reason, Some(StopReason::ToolUse)) { return; }

        let mut results = Vec::new();
        let mut used_todo = false;
        let mut manual_compact = false;
        for block in &response.content {
            if let ContentBlock::ToolUse { id, name, input } = block {
                let output = match name.as_str() {
                    "bash" => { let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or(""); println!("\x1b[33m$ {cmd}\x1b[0m"); run_bash(cmd) }
                    "read_file" => { let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(""); println!("\x1b[33m> read_file: {path}\x1b[0m"); run_read(workdir, path, input.get("limit").and_then(|v| v.as_i64())) }
                    "write_file" => { let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(""); let content = input.get("content").and_then(|v| v.as_str()).unwrap_or(""); println!("\x1b[33m> write_file: {path}\x1b[0m"); run_write(workdir, path, content) }
                    "edit_file" => { let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(""); println!("\x1b[33m> edit_file: {path}\x1b[0m"); run_edit(workdir, path, input.get("old_text").and_then(|v| v.as_str()).unwrap_or(""), input.get("new_text").and_then(|v| v.as_str()).unwrap_or("")) }
                    "TodoWrite" => { used_todo = true; println!("\x1b[33m> TodoWrite\x1b[0m"); match input.get("items").and_then(|v| v.as_array()) { Some(arr) => todo.update(arr).unwrap_or_else(|e| format!("Error: {e}")), None => "Error: items required".into() } }
                    "task" => { let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or(""); let desc = input.get("description").and_then(|v| v.as_str()).unwrap_or("subtask"); let at = input.get("agent_type").and_then(|v| v.as_str()).unwrap_or("Explore"); println!("\x1b[33m> task ({desc}, {at})\x1b[0m"); run_subagent(client, model, subagent_system, workdir, prompt, at).await }
                    "load_skill" => { let n = input.get("name").and_then(|v| v.as_str()).unwrap_or(""); println!("\x1b[33m> load_skill: {n}\x1b[0m"); skill_loader.get_content(n) }
                    "compact" => { manual_compact = true; println!("\x1b[33m> compact\x1b[0m"); "Compressing...".into() }
                    // Task tools
                    "task_create" => { let s = input.get("subject").and_then(|v| v.as_str()).unwrap_or(""); let d = input.get("description").and_then(|v| v.as_str()).unwrap_or(""); println!("\x1b[33m> task_create: {s}\x1b[0m"); let t = task_mgr.lock().await.create(s, d); serde_json::to_string(&t.to_json()).unwrap_or_default() }
                    "task_get" => { let tid = input.get("task_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32; match task_mgr.lock().await.get(tid) { Ok(t) => serde_json::to_string(&t.to_json()).unwrap_or_default(), Err(e) => e } }
                    "task_update" => { let tid = input.get("task_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32; let st = input.get("status").and_then(|v| v.as_str()); let abb: Vec<u32> = input.get("add_blocked_by").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default(); let ab: Vec<u32> = input.get("add_blocks").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default(); match task_mgr.lock().await.update(tid, st, &abb, &ab) { Ok(t) => if t.status == "deleted" { format!("Task {tid} deleted") } else { serde_json::to_string(&t.to_json()).unwrap_or_default() }, Err(e) => e } }
                    "task_list" => { println!("\x1b[33m> task_list\x1b[0m"); task_mgr.lock().await.list_all() }
                    "claim_task" => { let tid = input.get("task_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32; match task_mgr.lock().await.claim(tid, "lead") { Ok(t) => serde_json::to_string(&t.to_json()).unwrap_or_default(), Err(e) => e } }
                    // Background tools
                    "background_run" => { let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or(""); let timeout = input.get("timeout").and_then(|v| v.as_u64()).unwrap_or(120); println!("\x1b[33m> background_run: {cmd}\x1b[0m"); bg_mgr.lock().await.run(cmd, timeout) }
                    "check_background" => { let tid = input.get("task_id").and_then(|v| v.as_str()); bg_mgr.lock().await.check(tid) }
                    // Messaging tools
                    "send_message" => { let to = input.get("to").and_then(|v| v.as_str()).unwrap_or(""); let content = input.get("content").and_then(|v| v.as_str()).unwrap_or(""); let mt = input.get("msg_type").and_then(|v| v.as_str()).unwrap_or("message"); msg_bus.lock().await.send(to, "lead", content, mt) }
                    "read_inbox" => { msg_bus.lock().await.read_inbox("lead") }
                    "broadcast" => { let content = input.get("content").and_then(|v| v.as_str()).unwrap_or(""); let members = team_mgr.lock().await.member_names(); msg_bus.lock().await.broadcast("lead", content, &members) }
                    // Teammate tools
                    "spawn_teammate" => {
                        let tname = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let role = input.get("role").and_then(|v| v.as_str()).unwrap_or("");
                        let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                        println!("\x1b[33m> spawn_teammate: {tname} ({role})\x1b[0m");
                        let mut tm = team_mgr.lock().await;
                        match tm.spawn(tname, role) {
                            Ok(()) => {
                                drop(tm);
                                let c = client.clone();
                                let m = model.to_string();
                                let w = workdir.to_path_buf();
                                let n = tname.to_string();
                                let r = role.to_string();
                                let p = prompt.to_string();
                                let mb = msg_bus.clone();
                                let tmgr = task_mgr.clone();
                                let tmmgr = team_mgr.clone();
                                tokio::spawn(async move {
                                    teammate_loop(c, m, w, n, r, p, mb, tmgr, tmmgr).await;
                                });
                                format!("Spawned '{tname}' (role: {role})")
                            }
                            Err(e) => e,
                        }
                    }
                    "list_teammates" => { println!("\x1b[33m> list_teammates\x1b[0m"); team_mgr.lock().await.list_all() }
                    // Shutdown/plan tools
                    "shutdown_request" => {
                        let teammate = input.get("teammate").and_then(|v| v.as_str()).unwrap_or("");
                        let rid = Uuid::new_v4().to_string()[..8].to_string();
                        shutdown_requests.insert(rid.clone(), json!({"target": teammate, "status": "pending"}));
                        msg_bus.lock().await.send(teammate, "lead", &format!("{{\"request_id\":\"{rid}\"}}"), "shutdown_request");
                        format!("Shutdown request {rid} sent to '{teammate}'")
                    }
                    "plan_approval" => {
                        let rid = input.get("request_id").and_then(|v| v.as_str()).unwrap_or("");
                        let approve = input.get("approve").and_then(|v| v.as_bool()).unwrap_or(false);
                        let feedback = input.get("feedback").and_then(|v| v.as_str()).unwrap_or("");
                        match plan_requests.get_mut(rid) {
                            Some(req) => {
                                let status = if approve { "approved" } else { "rejected" };
                                req["status"] = json!(status);
                                let from = req.get("from").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                msg_bus.lock().await.send(&from, "lead", &format!("{{\"request_id\":\"{rid}\",\"approved\":{approve},\"feedback\":\"{feedback}\"}}"), "plan_approval_response");
                                format!("Plan {rid} {status}")
                            }
                            None => format!("Error: Unknown plan request_id '{rid}'"),
                        }
                    }
                    other => format!("Unknown tool: {other}"),
                };
                let preview: String = output.chars().take(200).collect();
                println!("{preview}");
                results.push(ContentBlock::ToolResult { tool_use_id: id.clone(), content: output });
            }
        }
        rounds_since_todo = if used_todo { 0 } else { rounds_since_todo + 1 };
        if rounds_since_todo >= 3 && todo.has_open_items() {
            results.insert(0, ContentBlock::Text { text: "<reminder>Update your todos.</reminder>".into() });
        }
        history.push(Message::new_blocks(Role::User, results));
        // Layer 3: manual compact triggered by the compact tool
        if manual_compact {
            println!("[manual compact]");
            *history = auto_compact(client, model, workdir, history).await;
        }
    }
}

async fn run_subagent(client: &AnthropicClient, model: &str, subagent_system: &str, workdir: &Path, prompt: &str, agent_type: &str) -> String {
    let is_general = agent_type == "general-purpose";
    let mut messages = vec![Message::new_text(Role::User, prompt)];
    for _ in 0..30 {
        let tools = if is_general { child_tools() } else { vec![bash_tool(), read_file_tool()] };
        let params = CreateMessageParams::new(RequiredMessageParams {
            model: model.to_string(),
            messages: messages.clone(),
            max_tokens: 8000,
        })
        .with_system(subagent_system)
        .with_tools(tools);

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
    let workdir = env::current_dir().unwrap();
    let skill_loader = SkillLoader::new(&workdir.join("skills"));
    let system = format!(
        "You are a coding agent at {cwd}.\n\
        Use task_create/task_update/task_list for multi-step work with persistent tasks.\n\
        Use TodoWrite for short checklists. Mark in_progress before starting, completed when done.\n\
        Use the task tool to delegate exploration or subtasks to a subagent.\n\
        Use spawn_teammate to create autonomous teammate agents for parallel work.\n\
        Use load_skill to access specialized knowledge before tackling unfamiliar topics.\n\
        All file paths must be relative to the working directory. Do not use absolute paths.\n\
        Prefer tools over prose.\n\n\
        Skills available:\n{}", skill_loader.get_descriptions()
    );
    let subagent_system = format!("You are a coding subagent at {cwd}. Complete the given task, then summarize your findings.");

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

    // Initialize all managers
    let task_mgr = Arc::new(Mutex::new(TaskManager::new(&workdir)));
    let mut bg_mgr_inner = BackgroundManager::new();
    let mut bg_rx = bg_mgr_inner.take_rx();
    let bg_mgr = Arc::new(Mutex::new(bg_mgr_inner));
    let msg_bus = Arc::new(Mutex::new(MessageBus::new(&workdir)));
    let team_mgr = Arc::new(Mutex::new(TeammateManager::new(&workdir)));
    let mut shutdown_requests: HashMap<String, serde_json::Value> = HashMap::new();
    let mut plan_requests: HashMap<String, serde_json::Value> = HashMap::new();

    let mut history: Vec<Message> = Vec::new();
    let mut todo = TodoManager::new();
    let stdin = io::stdin();

    loop {
        print!("\x1b[36ms_full >> \x1b[0m");
        io::stdout().flush().unwrap();

        let mut query = String::new();
        if stdin.lock().read_line(&mut query).unwrap() == 0 { break; }
        let query = query.trim();
        if query.is_empty() || query == "q" || query == "exit" { break; }

        // REPL commands
        match query {
            "/compact" => {
                if !history.is_empty() {
                    println!("[manual compact via /compact]");
                    history = auto_compact(&client, &model, &workdir, &history).await;
                } else {
                    println!("Nothing to compact.");
                }
                continue;
            }
            "/tasks" => {
                println!("{}", task_mgr.lock().await.list_all());
                continue;
            }
            "/team" => {
                println!("{}", team_mgr.lock().await.list_all());
                continue;
            }
            "/inbox" => {
                println!("{}", msg_bus.lock().await.read_inbox("lead"));
                continue;
            }
            _ => {}
        }

        history.push(Message::new_text(Role::User, query));
        agent_loop(
            &client, &model, &system, &subagent_system, &workdir,
            &mut history, &mut todo, &skill_loader,
            &task_mgr, &bg_mgr, &mut bg_rx, &msg_bus, &team_mgr,
            &mut shutdown_requests, &mut plan_requests,
        ).await;

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
