## Why

The Rust CLI (`src/main.rs`) currently only exposes a `bash` tool, matching the s01 (basic loop) level. The Python reference (`agents/s02_tool_use.py`) provides `read_file`, `write_file`, and `edit_file` in addition to `bash`. Adding these tools brings the Rust CLI to s02 parity, giving the model direct file operations that are safer and more efficient than piping everything through shell commands.

## What Changes

- Add `safe_path()` helper for workspace-relative path resolution with escape prevention
- Add `run_read(path, limit)` — read file contents with optional line limit and 50k char cap
- Add `run_write(path, content)` — write content to file, creating parent directories as needed
- Add `run_edit(path, old_text, new_text)` — find-and-replace first occurrence of exact text
- Add tool definitions (`Tool` structs) for `read_file`, `write_file`, `edit_file`
- Update `agent_loop` tool dispatch to route by tool name instead of hardcoding bash
- Update system prompt from "Use bash" to "Use tools" and prompt marker from "s01" to "s02"

## Capabilities

### New Capabilities
- `file-tools`: read_file, write_file, edit_file tool implementations with path safety and dispatch routing

### Modified Capabilities

## Impact

- `src/main.rs` — all changes are in this single file
- No new crate dependencies required (`std::fs`, `std::path` from stdlib)
- No API or protocol changes — tools use the same Anthropic tool_use/tool_result flow
