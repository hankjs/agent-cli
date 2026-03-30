## 1. Path Safety

- [x] 1.1 Add `safe_path(workdir, p)` function that resolves a path relative to workdir and rejects escapes (canonicalize existing, canonicalize parent for new files)

## 2. Tool Handlers

- [x] 2.1 Add `run_read(workdir, path, limit)` — read file contents with optional line limit and 50k char truncation
- [x] 2.2 Add `run_write(workdir, path, content)` — write file with parent dir creation
- [x] 2.3 Add `run_edit(workdir, path, old_text, new_text)` — first-occurrence replacement

## 3. Tool Definitions

- [x] 3.1 Add `read_file_tool()`, `write_file_tool()`, `edit_file_tool()` functions returning `Tool` structs with input schemas

## 4. Dispatch & Wiring

- [x] 4.1 Update `agent_loop` to match on tool name and dispatch to appropriate handler (bash, read_file, write_file, edit_file, or "Unknown tool")
- [x] 4.2 Pass all four tools to `.with_tools()` in agent_loop

## 5. Prompt Updates

- [x] 5.1 Update system prompt to "Use tools to solve tasks" and REPL prompt to "s02 >>"

## 6. Verify

- [x] 6.1 `cargo build` succeeds with no errors
