use std::env;
use std::io::{self, BufRead, Write};
use std::process::Command;

use anthropic_ai_sdk::client::AnthropicClient;
use anthropic_ai_sdk::types::message::{
    ContentBlock, CreateMessageParams, Message, MessageClient, MessageContent, MessageError,
    RequiredMessageParams, Role, StopReason, Tool,
};
use serde_json::json;

const DANGEROUS: &[&str] = &["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];

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

async fn agent_loop(client: &AnthropicClient, model: &str, system: &str, history: &mut Vec<Message>) {
    loop {
        let params = CreateMessageParams::new(RequiredMessageParams {
            model: model.to_string(),
            messages: history.clone(),
            max_tokens: 8000,
        })
        .with_system(system)
        .with_tools(vec![bash_tool()]);

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
            if let ContentBlock::ToolUse { id, input, .. } = block {
                let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("");
                println!("\x1b[33m$ {cmd}\x1b[0m");
                let output = run_bash(cmd);
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
    let system = format!("You are a coding agent at {cwd}. Use bash to solve tasks. Act, don't explain.");

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
        print!("\x1b[36ms01 >> \x1b[0m");
        io::stdout().flush().unwrap();

        let mut query = String::new();
        if stdin.lock().read_line(&mut query).unwrap() == 0 { break; }
        let query = query.trim();
        if query.is_empty() || query == "q" || query == "exit" { break; }

        history.push(Message::new_text(Role::User, query));
        agent_loop(&client, &model, &system, &mut history).await;

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
