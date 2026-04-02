## Context

hank-cli is a Rust CLI tool intended to replicate the full Claude Code experience. The current implementation is a monolithic ~2000-line main.rs with no streaming, no modular tool architecture, and no real permission system. We are doing a complete rewrite using a Core + Plugin architecture where `hank-core` defines abstractions and `hank-tools`/`hank-mcp` provide implementations.

The reference implementation is Claude Code (TypeScript), specifically its:
- `query.ts` / `QueryEngine.ts`: async generator query loop
- `Tool.ts` / `tools.ts`: tool trait + registry + assembly
- `services/api/claude.ts`: SSE streaming with eventsource
- `utils/permissions/`: layered permission system
- `constants/prompts.ts` + `context.ts`: system prompt assembly
- `services/mcp/client.ts`: MCP JSON-RPC subprocess tools

## Goals / Non-Goals

**Goals:**
- 4-crate Cargo workspace with clear dependency direction: main → tui/tools/mcp → core
- `hank-core` has zero knowledge of any specific tool - pure abstractions
- Real-time streaming via SSE (eventsource-stream on reqwest bytes_stream)
- QueryEngine communicates with TUI via mpsc channel (QueryEvent enum)
- Permission system with interactive Ask flow via oneshot channel back from TUI
- Phase 1 deliverable: streaming conversation + bash + file read/write/edit + permission popups

**Non-Goals:**
- Web browser tool, voice input, notebook editing (future phases)
- Full Claude Code feature parity in Phase 1 (no agent spawn, no plan mode, no worktrees)
- Plugin hot-reload or dynamic library loading (use MCP subprocess protocol instead)
- Compatibility with the old monolithic main.rs code or data formats

## Decisions

### D1: Cargo workspace with 4 crates

**Choice**: `hank-core`, `hank-tools`, `hank-mcp`, `hank-tui` as separate crates in a workspace.

**Alternatives considered**:
- Single crate with modules: Simpler but no compilation isolation, tool implementations entangled with core
- More granular crates (hank-tools-fs, hank-tools-web): Premature split, Cargo features can add this later

**Rationale**: 4 crates matches the natural architectural boundary. Core defines traits, tools implements them, mcp bridges external tools, tui renders UI. Each can be compiled and tested independently.

### D2: SSE streaming via eventsource-stream + reqwest

**Choice**: Direct `reqwest::Response::bytes_stream().eventsource()` using `eventsource-stream` crate.

**Alternatives considered**:
- `reqwest-eventsource`: Adds unwanted auto-reconnect logic for POST-based one-shot streams
- `anthropic-ai-sdk` crate: Has streaming but limited control over headers, error handling, extended thinking
- Raw HTTP + manual SSE parsing: Unnecessary when eventsource-stream handles it correctly

**Rationale**: eventsource-stream is a thin SSE parser on any byte stream. No reconnect logic (wrong for POST), no SDK lock-in, full control over request construction.

### D3: mpsc channel for engine-to-UI communication

**Choice**: `tokio::sync::mpsc::Sender<QueryEvent>` passed to `QueryEngine::submit()`.

**Alternatives considered**:
- Async generator / Stream trait: Complex lifetimes in Rust, borrow checker friction with mutable engine state
- Shared state with Arc<Mutex>: Polling-based, no event push
- Callback closures: Lifetime issues with async closures

**Rationale**: mpsc is the idiomatic Rust pattern for async producer-consumer. QueryEngine spawns as a tokio task, sends events as they arrive. TUI receives via `tokio::select!` alongside crossterm keyboard events. Permission requests use a nested `oneshot::channel` for the response path.

### D4: ratatui with crossterm event-stream pattern

**Choice**: ratatui 0.30 + crossterm with `event-stream` feature + tui-textarea + throbber-widgets-tui.

**Alternatives considered**:
- Raw crossterm (no framework): Too much manual rendering for scrollable output + popups
- termion backend: Less cross-platform than crossterm
- Full custom TUI: Unnecessary reimplementation

**Rationale**: ratatui's `Paragraph::scroll()` handles scrollable conversation display. `tui-textarea` provides input editing. crossterm's `EventStream` integrates with tokio::select! for non-blocking keyboard input alongside async stream events.

### D5: Tool trait with async_trait + Arc<dyn Tool>

**Choice**: `#[async_trait] pub trait Tool: Send + Sync` stored as `Arc<dyn Tool>` in registry.

**Alternatives considered**:
- Enum dispatch (one variant per tool): No extensibility for MCP tools
- Generic type parameters: Cannot store heterogeneous tools in one collection
- Native async fn in trait (Rust 1.75+): Works for simple cases but doesn't support `dyn Tool` dispatch without boxing

**Rationale**: `async_trait` gives us `Box<dyn Future>` return types that work with `dyn Tool`. `Arc` instead of `Box` because MCP tools share the same connection handle.

### D6: Permission Ask via oneshot channel

**Choice**: `QueryEvent::PermissionRequest` carries a `oneshot::Sender<PermissionResponse>`. Engine awaits the response.

**Alternatives considered**:
- Blocking the engine thread: Would freeze streaming
- Callback function: Lifetime issues in async context
- Shared flag with polling: Wasteful and race-prone

**Rationale**: oneshot is a zero-cost single-use channel. Engine sends the request, awaits response. TUI shows popup, user presses Y/N, sends response. Clean async flow with no shared mutable state.

### D7: MCP via subprocess stdio (self-implemented)

**Choice**: Spawn child process, JSON-RPC 2.0 over stdin/stdout, ~200 lines of Rust.

**Alternatives considered**:
- `rmcp` crate (official Rust MCP SDK): More complete but heavier dependency, less control
- WASM plugins: Massive binary size increase (~10-20MB for wasmtime), overkill for tool calls
- Dynamic library loading: ABI instability across Rust versions, no sandboxing

**Rationale**: MCP over stdio is simple (spawn + readline + JSON), gives process isolation for free, and is the same protocol Claude Code uses. Self-implementation keeps dependency footprint minimal and gives full control.

### D8: Prompt text as faithfully-ported Rust string constants

**Choice**: Port all Claude Code system prompt sections and tool descriptions verbatim into a `hank-core/src/context/prompts.rs` module as `const &str` constants. Sections: INTRO, SYSTEM, DOING_TASKS, ACTIONS, USING_TOOLS, TONE_STYLE, GIT_COMMIT, GIT_PR, ENVIRONMENT (template). Each tool's description/prompt also stored as a const in its implementation file.

**Alternatives considered**:
- External template files (Tera/Handlebars): Adds runtime dependency, harder to test, can fail at runtime
- Simplified/rewritten prompts: Loses the battle-tested wording that shapes model behavior correctly
- Load from TOML/YAML config: Unnecessary indirection for static text

**Rationale**: Claude Code's prompt text is the product of extensive iteration. The exact wording matters for model behavior (e.g., "measure twice, cut once", "Do NOT use the Bash tool to run commands when a relevant dedicated tool is provided"). Storing as Rust string constants is zero-cost, compile-time checked, and easy to diff against the source.

### D9: Message harness with system-reminder injection and result budgeting

**Choice**: Implement a `MessageNormalizer` in hank-core that handles: (1) wrapping injected context in `<system-reminder>` XML tags, (2) merging consecutive user messages, (3) persisting large tool results to disk with `<persisted-output>` wrappers, (4) replacing cleared results with placeholder text, (5) injecting nag reminders for task tracking.

**Alternatives considered**:
- No normalization (send raw): Model would see inconsistent message format, consecutive user messages would fail on some API backends
- Normalization in TUI layer: Wrong separation of concerns, engine should own message format

**Rationale**: Claude Code's message harness is critical infrastructure. The `<system-reminder>` pattern, persisted-output wrappers, and nag reminders directly affect model behavior quality. The normalizer sits between the engine and API client, transforming internal messages to API format.

### D10: StreamAccumulator for partial JSON tool inputs

**Choice**: Accumulate `input_json_delta` partial strings per content block index, parse only at `content_block_stop`.

**Rationale**: Anthropic's streaming sends tool call JSON as fragments. Attempting to parse mid-stream fails. The accumulator pattern (HashMap<block_index, String>) is what Claude Code uses internally. Parse once at block completion, then dispatch to tool executor.

## Risks / Trade-offs

- **[Risk] eventsource-stream crate maintenance** → It's stable (v0.2, nom-based parser), used by reqwest-eventsource. Low risk but we own the integration layer.
- **[Risk] ratatui Paragraph scroll doesn't handle markdown rendering** → Phase 1 uses plain text with ANSI colors. Markdown rendering (code blocks, bold) is Phase 2 work.
- **[Risk] Permission oneshot blocks the engine task** → Timeout after 60s with auto-deny. Engine must not hang if TUI crashes.
- **[Risk] Large tool outputs may exceed terminal buffer** → Truncate to 50K chars (matching current behavior), full output persisted to file.
- **[Trade-off] Self-implemented MCP vs rmcp crate** → Less feature-complete (no SSE transport, no streamable HTTP) but simpler and sufficient for stdio-based MCP servers.
- **[Trade-off] Dropping anthropic-ai-sdk dependency** → Loses pre-built types but gains full control over streaming, headers, and error handling.
