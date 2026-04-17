use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use hank_core::permission::PermissionChecker;
use hank_core::prompt::{self, EnvironmentConfig};
use hank_core::query::{EngineCommand, QueryEngine};
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

    // Create engine with permission checker
    let client = ApiClient::new(api_key, base_url)?;
    let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let tool_ctx = ToolContext { working_dir, abort: abort_rx };
    let (perm_mode, perm_rules) = settings.to_permission_config();
    let permission_checker = PermissionChecker::new(perm_mode, perm_rules);
    let mut engine = QueryEngine::new(client, registry, system_prompt, model.clone(), tool_ctx, permission_checker);

    // Channels
    let (query_tx, mut query_rx) = tokio::sync::mpsc::channel(256);
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<EngineCommand>(16);

    // Spawn engine task — runs independently so it doesn't block the UI loop
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            engine.handle_command(cmd, &query_tx).await;
        }
    });

    // TUI setup
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(model);
    app.abort_tx = Some(abort_tx);
    let mut event_stream = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(33));

    // Main loop — draw only on tick (30fps), process events without redrawing
    while app.running {
        tokio::select! {
            biased;
            // User input always has priority
            Some(Ok(event)) = event_stream.next() => {
                match event {
                    Event::Key(key) => app.handle_key(key, &cmd_tx),
                    Event::Mouse(mouse) => app.handle_mouse(mouse),
                    _ => {}
                }
            }
            // Engine events — drain all pending at once
            Some(event) = query_rx.recv() => {
                app.handle_query_event(event);
                while let Ok(event) = query_rx.try_recv() {
                    app.handle_query_event(event);
                }
            }
            // Tick — redraw every frame
            _ = tick.tick() => {
                app.spinner_tick += 1;
                if app.needs_clear {
                    terminal.clear()?;
                    app.needs_clear = false;
                }
                terminal.draw(|frame| {
                    app.render(frame.area(), frame.buffer_mut());
                })?;
            }
        }
    }

    // Cleanup
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    )?;

    Ok(())
}
