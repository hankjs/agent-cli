## Context

The Rust CLI (`src/main.rs`) implements a basic agent loop with only a `bash` tool. The Python reference (`agents/s02_tool_use.py`) demonstrates the full s02 tool set: bash, read_file, write_file, edit_file. All types needed (`Tool`, `ContentBlock::ToolUse`, `ContentBlock::ToolResult`) are already available in `anthropic-ai-sdk 0.2`.

## Goals / Non-Goals

**Goals:**
- Achieve feature parity with Python s02_tool_use.py in the Rust CLI
- Implement path safety (workspace escape prevention)
- Route tool calls by name in agent_loop instead of hardcoding bash

**Non-Goals:**
- Adding tools beyond what s02 defines (no glob, grep, LSP, etc.)
- Streaming support
- Permission/confirmation system for dangerous file operations
- Refactoring into multiple modules (keep everything in main.rs)

## Decisions

1. **Dispatch via `match` on tool name** — A `match name.as_str()` block in agent_loop is idiomatic Rust and avoids the complexity of a HashMap with heterogeneous handler signatures. Each branch extracts its own fields from the `serde_json::Value` input.

2. **Path safety using `canonicalize` with fallback** — For existing files, use `std::fs::canonicalize()` then check `starts_with(workdir)`. For new files (write_file), canonicalize the parent directory and verify containment, since the file itself doesn't exist yet.

3. **Workspace root from `env::current_dir()` at startup** — Same approach as the Python version using `Path.cwd()`. Stored as a `PathBuf` and passed to tool handlers.

4. **All changes in a single file** — Matches the Python s02 pattern where everything lives in one file. No module extraction needed at this stage.

## Risks / Trade-offs

- **`canonicalize` requires file existence** → Mitigated by canonicalizing parent dir for write operations
- **No timeout on file operations** → Acceptable for local file I/O; bash already has no timeout in Rust version
- **50k char truncation uses `.chars().take()` which is O(n)** → Acceptable for this scale; matches existing bash output truncation pattern
