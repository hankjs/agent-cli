use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anthropic_ai_sdk::client::AnthropicClient;
use anthropic_ai_sdk::types::message::{
    ContentBlock, CreateMessageParams, Message, MessageClient, MessageContent, MessageError,
    RequiredMessageParams, Role, StopReason, Tool,
};
use serde_json::json;

const DANGEROUS: &[&str] = &["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];

fn safe_path(workdir: &Path, p: &str) -> Result<PathBuf, String> {
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
        description: Some("Read file contents.".into()),
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
        description: Some("Write content to file.".into()),
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
        description: Some("Replace exact text in file.".into()),
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

async fn agent_loop(client: &AnthropicClient, model: &str, system: &str, workdir: &Path, history: &mut Vec<Message>) {
    loop {
        let params = CreateMessageParams::new(RequiredMessageParams {
            model: model.to_string(),
            messages: history.clone(),
            max_tokens: 8000,
        })
        .with_system(system)
        .with_tools(vec![bash_tool(), read_file_tool(), write_file_tool(), edit_file_tool()]);

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
        history.push(Message::new_blocks(Role::User, results));
    }
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
    let system = format!("You are a coding agent at {cwd}. Use tools to solve tasks. Act, don't explain.");
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
    let stdin = io::stdin();

    loop {
        print!("\x1b[36ms02 >> \x1b[0m");
        io::stdout().flush().unwrap();

        let mut query = String::new();
        if stdin.lock().read_line(&mut query).unwrap() == 0 { break; }
        let query = query.trim();
        if query.is_empty() || query == "q" || query == "exit" { break; }

        history.push(Message::new_text(Role::User, query));
        agent_loop(&client, &model, &system, &workdir, &mut history).await;

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
