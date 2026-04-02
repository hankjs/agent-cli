## Why

The current hank-cli is a monolithic ~2000-line main.rs that cannot be maintained or extended. It lacks streaming responses (blocks until full API response), has no real permission system, no file search tools, and no modular tool architecture. To become a fully functional Claude Code alternative in Rust, it needs a complete rewrite with a Core + Plugin (Tools) architecture using a Cargo workspace.

## What Changes

- **BREAKING**: Replace the entire monolithic main.rs with a 4-crate Cargo workspace: `hank-core`, `hank-tools`, `hank-mcp`, `hank-tui`
- **New**: `hank-core` crate providing the Tool trait, ToolRegistry, streaming API client (SSE via eventsource-stream), QueryEngine (conversation loop), permission system, and system prompt assembly
- **New**: `hank-tools` crate implementing built-in tools (Bash, FileRead, FileWrite, FileEdit, Glob, Grep) as `impl Tool` registered at startup
- **New**: `hank-mcp` crate implementing MCP JSON-RPC 2.0 client over stdio, wrapping remote tools as `Box<dyn Tool>` via adapter pattern
- **New**: `hank-tui` crate providing ratatui-based terminal UI with streaming text display, input box, permission confirmation popup, and spinner status bar
- **New**: Real-time streaming output via mpsc channel between QueryEngine and TUI
- **New**: Permission system with Allow/Deny/Ask decisions and user confirmation dialogs
- **New**: System prompt assembly from templates + CLAUDE.md discovery + git context

## Capabilities

### New Capabilities
- `streaming-api`: SSE streaming client for Anthropic Messages API, StreamEvent parsing, partial JSON accumulation for tool_use inputs
- `tool-system`: Tool trait definition, ToolRegistry for registration/discovery/dispatch, ToolExecutor for concurrency-safe parallel/serial tool scheduling
- `query-engine`: Core conversation loop - API call, stream response, detect tool_use, execute tools, append results, loop. Context compression when token limit approached
- `permission-system`: PermissionMode (Default/AcceptEdits/Bypass), rule-based matching with wildcards, interactive Ask flow via oneshot channel to TUI
- `system-prompt`: Layered prompt assembly - base template + tool descriptions + CLAUDE.md (managed/user/project/local) + git status
- `terminal-ui`: ratatui app with scrollable message display, tui-textarea input, throbber spinner, permission popup modal, crossterm async event handling
- `mcp-client`: MCP JSON-RPC 2.0 client spawning child processes, initialize handshake, tools/list discovery, tools/call execution, McpTool adapter to Tool trait
- `builtin-tools`: Bash (command execution + danger check), FileRead (line numbers + size limit), FileWrite (path safety), FileEdit (unique match replace + diff), Glob (pattern matching), Grep (content search)
- `prompt-harness`: Complete system prompt ported from Claude Code - identity, system instructions, doing-tasks guidelines, executing-actions-with-care, using-your-tools rules, tone-and-style, git commit/PR workflow instructions, environment info section. Each tool's description/prompt text faithfully reproduced as Rust string constants
- `message-harness`: Tool result formatting pipeline (mapToolResult, large result persistence with `<persisted-output>` wrappers), `<system-reminder>` tag injection for context/memory/hooks, message normalization for API (filter synthetic errors, merge consecutive user messages, sanitize unavailable tool refs), todo/task nag reminders, error formatting with is_error flag, tool result budget enforcement

### Modified Capabilities
_(None - this is a full rewrite, existing specs describe the old monolithic architecture)_

## Impact

- **Code**: Replaces entire src/main.rs with workspace at crates/
- **Dependencies**: Adds reqwest (streaming), eventsource-stream, ratatui, crossterm, tui-textarea, throbber-widgets-tui, glob, futures-util. Removes anthropic-ai-sdk (replaced by direct API calls)
- **Build**: Changes from single binary to Cargo workspace with 4 crates + thin main.rs entry point
- **Config**: New settings.json format for permissions and MCP server configuration
- **Data**: New session persistence format (JSON conversation history)
