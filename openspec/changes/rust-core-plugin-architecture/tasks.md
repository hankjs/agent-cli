## 1. Workspace Scaffolding

- [x] 1.1 Create Cargo workspace with root Cargo.toml defining members `["crates/*"]` and workspace dependencies (tokio, serde, serde_json, async-trait, thiserror)
- [x] 1.2 Create `crates/hank-core/Cargo.toml` with dependencies: tokio, serde, serde_json, async-trait, thiserror, reqwest (stream+json), eventsource-stream, futures-util, uuid
- [x] 1.3 Create `crates/hank-tools/Cargo.toml` with dependencies: hank-core, tokio, serde_json, async-trait, glob
- [x] 1.4 Create `crates/hank-mcp/Cargo.toml` with dependencies: hank-core, tokio, serde, serde_json, async-trait
- [x] 1.5 Create `crates/hank-tui/Cargo.toml` with dependencies: hank-core, tokio, ratatui, crossterm (event-stream), tui-textarea (crossterm), throbber-widgets-tui, futures
- [x] 1.6 Create thin `src/main.rs` entry point that imports all 4 crates and has a skeleton `#[tokio::main] async fn main()`
- [x] 1.7 Verify `cargo build` succeeds with empty lib.rs in each crate

## 2. Streaming API Client (hank-core/api)

- [x] 2.1 Define `StreamEvent` enum with serde tag deserialization: MessageStart, ContentBlockStart, ContentBlockDelta, ContentBlockStop, MessageDelta, MessageStop, Ping, Error
- [x] 2.2 Define `ContentBlock` enum (Text, ToolUse, Thinking), `Delta` enum (TextDelta, InputJsonDelta, ThinkingDelta), `Usage`, `StopReason` types
- [x] 2.3 Define `Message` struct with role, content blocks, and metadata (id, timestamp)
- [x] 2.4 Implement `ApiClient` struct with `stream()` method: POST to `/v1/messages` with `stream: true`, return `impl Stream<Item=Result<StreamEvent>>` via eventsource-stream on bytes_stream
- [x] 2.5 Implement `StreamAccumulator`: HashMap<usize, String> for partial JSON, HashMap<usize, (id, name)> for tool calls, parse at content_block_stop
- [x] 2.6 Implement retry logic: exponential backoff (1s, 2s, 4s), max 3 retries, only for 429/500/502/503/529
- [x] 2.7 Write integration test: send a simple message to API, verify StreamEvent sequence received

## 3. Tool System (hank-core/tool)

- [x] 3.1 Define `Tool` trait with async_trait: name, description, input_schema, call, format_result, is_concurrency_safe, is_read_only, validate_input, check_permissions
- [x] 3.2 Define `ToolResult` struct (data: Value, new_messages: Option<Vec<Message>>)
- [x] 3.3 Define `ToolError` enum (ValidationError, ExecutionError, PermissionDenied, Timeout)
- [x] 3.4 Define `ToolContext` struct (working_dir, abort_signal, permission_context)
- [x] 3.5 Implement `ToolRegistry`: register, get, api_definitions, merge, filtered
- [x] 3.6 Implement `ToolExecutor`: accept Vec of tool calls, check concurrency_safe, dispatch parallel or sequential via tokio::join/sequential loop
- [x] 3.7 Write unit test: register mock tools, dispatch by name, verify results

## 4. Query Engine (hank-core/engine)

- [x] 4.1 Define `QueryEvent` enum: TextDelta, ThinkingDelta, ToolStart, ToolComplete, PermissionRequest (with oneshot sender), SpinnerMode, TurnComplete, Error
- [x] 4.2 Define `SpinnerMode` enum: Requesting, Thinking, Responding, ToolInput, ToolExecuting
- [x] 4.3 Implement `QueryEngine` struct holding ApiClient, ToolRegistry, messages Vec, system prompt, permission context
- [x] 4.4 Implement `QueryEngine::submit(input, tx)`: add user message, spawn tokio task running the query loop
- [x] 4.5 Implement query loop: call API stream → forward TextDelta/ThinkingDelta to tx → accumulate tool calls → on end_turn send TurnComplete → on tool_use execute tools and loop
- [x] 4.6 Implement tool execution within loop: for each tool call, check permissions (send PermissionRequest if Ask, await oneshot), execute via ToolExecutor, send ToolStart/ToolComplete events, append tool_result messages
- [x] 4.7 Implement session history management: message append, serialization to JSON for persistence
- [x] 4.8 Implement basic context compression: estimate tokens (chars/4), when >100K compress older messages keeping 3 most recent pairs

## 5. Permission System (hank-core/permission)

- [x] 5.1 Define `PermissionMode` enum: Default, AcceptEdits, Bypass
- [x] 5.2 Define `PermissionRule` struct: tool_name pattern, behavior (Allow/Deny/Ask), with wildcard matching
- [x] 5.3 Define `PermissionDecision` enum: Allow, Deny(reason), Ask
- [x] 5.4 Define `PermissionResponse` enum: Allow, Deny, AlwaysAllow(pattern)
- [x] 5.5 Implement `PermissionChecker`: load rules, check(tool_name, input) → PermissionDecision, following the flow: deny rules → tool.check_permissions → allow rules → mode-based decision
- [x] 5.6 Implement session rule accumulation: when user selects "Always Allow", add an allow rule for the session
- [x] 5.7 Write unit test: verify deny rules take precedence, wildcard matching works, mode-based decisions correct

## 6. Prompt Harness (hank-core/context)

- [x] 6.1 Create `prompts.rs` with all base system prompt sections as const str: INTRO (identity + cyber risk), SYSTEM (tools, permissions, system-reminder tags, hooks, compression), DOING_TASKS (read before modify, avoid over-engineering, no backwards-compat hacks, security awareness, no time estimates), ACTIONS (reversibility, blast radius, risky action examples, measure-twice-cut-once), USING_TOOLS (dedicated tool preference: Read not cat, Edit not sed, Write not echo, Glob not find, Grep not grep; parallel tool calls guidance), TONE_STYLE (no emojis, concise, file_path:line_number format)
- [x] 6.2 Create git commit workflow prompt as const str: safety protocol (never force-push, never skip hooks, always new commit, HEREDOC format), step-by-step with parallel tool calls for status/diff/log, commit message guidelines, Co-Authored-By attribution
- [x] 6.3 Create PR workflow prompt as const str: parallel status/diff/log checks, PR title/body format with HEREDOC, gh pr create example, Summary + Test plan template
- [x] 6.4 Create environment info template: working directory, git repo status, platform, shell, OS version, model name/ID, current date. Implement `render_environment(config) -> String`
- [x] 6.5 Add `prompt()` method to Tool trait returning full tool-specific instruction text. Implement for each tool: BashTool (basic desc + instructions + sandbox + git commit/PR), FileReadTool (absolute path, 2000 line default, line numbers, image/PDF notes), FileEditTool (read-before-edit, indentation, unique match, replace_all), FileWriteTool (overwrite warning, read-first, prefer-Edit), GlobTool (pattern syntax, mtime sort), GrepTool (ripgrep syntax, output modes, multiline)
- [x] 6.6 Implement CLAUDE.md discovery: scan ~/.claude/CLAUDE.md, walk up from working dir checking CLAUDE.md and .claude/CLAUDE.md, check .claude.local/CLAUDE.md
- [x] 6.7 Implement git context collection: branch name, main branch detection, status (truncated 2KB), 5 recent commits via `git` CLI
- [x] 6.8 Implement `build_system_prompt(registry, config)`: concatenate base sections (intro→system→doing_tasks→actions→using_tools→tone_style) + tool prompts from registry + CLAUDE.md content + git context + environment info + current date
- [x] 6.9 Implement user context injection: prepend CLAUDE.md + currentDate as first user message wrapped in `<system-reminder>` tags with "As you answer the user's questions, you can use the following context:" header
- [x] 6.10 Write unit test: verify all 6 base sections present in order, tool prompts included, CLAUDE.md wrapped in system-reminder, git context appended as plain text

## 7. Message Harness (hank-core/engine/messages)

- [x] 7.1 Implement `MessageNormalizer::normalize(messages) -> Vec<ApiMessage>`: filter synthetic/meta messages, merge consecutive user messages, pack tool_results into user messages
- [x] 7.2 Implement tool result formatting: `format_tool_result(tool_use_id, content, is_error) -> ToolResultBlock`. Error results get `is_error: true` with "Error: {message}" prefix
- [x] 7.3 Implement large result persistence: when result exceeds `max_result_size_chars`, write full content to `.claude/tool-results/{tool_use_id}.txt`, replace with `<persisted-output>` wrapper containing file path + first 2KB preview + "Output too large ({size}). Full output saved to: {path}"
- [x] 7.4 Implement tool result budget enforcement: per-message aggregate budget, persist excess results, keep 3 most recent in full
- [x] 7.5 Implement `<system-reminder>` wrapper utility: `wrap_system_reminder(content) -> String` that wraps text in `<system-reminder>` XML tags. Used for: CLAUDE.md injection, hook outputs, skill listings, nag reminders
- [x] 7.6 Implement todo/task nag reminder injection: after N assistant turns without task tool usage (and open items exist), inject `<system-reminder>` with reminder text + current task list + "Make sure that you NEVER mention this reminder to the user"
- [x] 7.7 Implement cleared result placeholder: when context compression clears old tool results, replace content with `[Old tool result content cleared]`
- [x] 7.8 Write unit tests: verify consecutive user merge, is_error formatting, persisted-output wrapper format, system-reminder wrapping, nag reminder injection timing

## 8. Built-in Tools (hank-tools)

- [x] 8.1 Implement `BashTool`: tokio::process::Command, stdout/stderr capture, 50K char truncation, 120s default timeout, dangerous command deny list
- [x] 8.2 Implement `FileReadTool`: read file with line numbers, offset/limit support, 2000 line default limit, path safety validation
- [x] 8.3 Implement `FileWriteTool`: write content to file, create parent dirs, path traversal prevention (no `..` escaping working dir)
- [x] 8.4 Implement `FileEditTool`: read file, validate old_string uniqueness (or replace_all), replace, write back, return diff
- [x] 8.5 Implement `GlobTool`: glob crate pattern matching, sort by mtime, 1000 file limit
- [x] 8.6 Implement `GrepTool`: regex content search with optional file glob filter and context lines, 1000 match limit
- [x] 8.7 Implement `register_all(registry, workdir)`: register all 6 tools with the working directory context
- [x] 8.8 Write unit tests for each tool: bash echo, file read/write round-trip, edit uniqueness check, glob matching, grep regex

## 9. Terminal UI (hank-tui)

- [x] 9.1 Implement `EventHandler`: crossterm EventStream + tick interval + mpsc channel, tokio::select! multiplexing
- [x] 9.2 Implement `App` struct with state: running, messages, scroll_offset, input (TextArea), spinner_state, show_permission_dialog, is_streaming
- [x] 9.3 Implement three-zone layout: Layout::default() with Min(1) + Length(3) + Length(1) constraints
- [x] 9.4 Implement scrollable message display: Paragraph with scroll, Scrollbar, auto-scroll on streaming, PageUp/PageDown manual scroll
- [x] 9.5 Implement text input: tui-textarea, Enter submits, Ctrl+C cancels streaming
- [x] 9.6 Implement permission popup: centered_rect + Clear overlay, [Y]/[N]/[A] key handling, oneshot response
- [x] 9.7 Implement spinner status bar: throbber widget with SpinnerMode-based labels, idle state showing session info
- [x] 9.8 Implement `App::run()` main loop: draw → next_event → match Tick/Crossterm/App → handle

## 10. Integration & Entry Point

- [x] 10.1 Implement `main.rs`: parse CLI args (model, working dir, initial prompt), load settings, build ToolRegistry with hank_tools::register_all, build system prompt, create QueryEngine, launch TUI
- [x] 10.2 Implement settings.json loading: permission rules, mode, MCP server configs from `~/.config/hank/settings.json`
- [x] 10.3 Wire QueryEngine events to TUI App event handler: spawn engine task, clone TUI event sender, bridge QueryEvent → AppEvent
- [x] 10.4 Implement `/compact` slash command: trigger manual context compression
- [x] 10.5 Implement `/help` slash command: display available commands
- [x] 10.6 End-to-end test: launch app, send a message, verify streaming output appears, tool call executes, permission popup works
