use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use hank_core::prompt::{self, EnvironmentConfig};
use hank_core::query::QueryEngine;
use hank_core::settings::Settings;
use hank_core::streaming::ApiClient;
use hank_core::tool::{ToolContext, ToolRegistry};
use hank_tui::app::App;
use ratatui::prelude::*;
use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let working_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Load and merge settings (user → project → local), apply env vars
    let settings = Settings::load_merged(&working_dir);
    settings.apply_env();

    // Resolve model and API config from settings + env
    let model = settings.resolve_model();
    let api_key = settings.resolve_api_key().unwrap_or_default();
    let base_url = settings.resolve_base_url();
    let is_git = working_dir.join(".git").exists();

    if api_key.is_empty() {
        eprintln!("Error: Set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN (via settings.json env or environment)");
        std::process::exit(1);
    }

    // Build tool registry
    let mut registry = ToolRegistry::new();
    hank_tools::tools::register_all(&mut registry);

    // Build system prompt
    let tool_prompts: Vec<_> = registry.all_tools().iter()
        .map(|t| (t.name().to_string(), t.prompt().to_string()))
        .collect();
    let config = EnvironmentConfig {
        working_dir: working_dir.clone(),
        is_git_repo: is_git,
        platform: env::consts::OS.into(),
        shell: env::var("SHELL").unwrap_or_else(|_| "unknown".into()),
        os_version: String::new(),
        model_name: model.clone(),
        model_id: model.clone(),
    };
    let system_prompt = prompt::build_system_prompt(&tool_prompts, &config);

    // Create engine
    let client = ApiClient::new(api_key, base_url)?;
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let tool_ctx = ToolContext { working_dir, abort: abort_rx };
    let mut engine = QueryEngine::new(client, registry, system_prompt, model.clone(), tool_ctx);

    // Channels
    let (query_tx, mut query_rx) = tokio::sync::mpsc::channel(256);
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<String>(16);

    // TUI setup
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(model);
    let mut event_stream = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(33));

    // Main loop
    while app.running {
        terminal.draw(|f| app.draw(f))?;
        app.spinner_tick += 1;

        tokio::select! {
            _ = tick.tick() => {}
            Some(Ok(Event::Key(key))) = event_stream.next() => {
                app.handle_key(key, &input_tx);
            }
            Some(event) = query_rx.recv() => {
                app.handle_query_event(event);
            }
            Some(input) = input_rx.recv() => {
                let tx = query_tx.clone();
                engine.submit(&input, &tx).await;
            }
        }
    }

    // Cleanup
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;

    Ok(())
}
