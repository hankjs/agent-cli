## Context

The Rust agent CLI (`src/main.rs`) currently has bash, read_file, write_file, edit_file, todo, task, and load_skill tools. Conversations grow unbounded — every tool call adds content to history. The Python s06 introduces a three-layer compression pipeline. The Rust SDK's `ContentBlock::ToolResult { tool_use_id: String, content: String }` has owned String fields, allowing in-place mutation. `Message` derives `Serialize` for transcript serialization.

## Goals / Non-Goals

**Goals:**
- Port the Python s06 three-layer compression to Rust
- micro_compact: silently trim old tool_result content every turn (keep last 3)
- auto_compact: summarize and reset when token estimate > 50000
- compact tool: let the model trigger compression on demand
- Save transcripts to `.transcripts/` before compressing
- Keep all existing tools (cumulative, not standalone like Python s06)

**Non-Goals:**
- Precise token counting (rough heuristic is fine, matching Python)
- Compressing subagent conversations (subagent is short-lived)
- Giving subagent access to compact tool
- Persisting summaries across sessions

## Decisions

**1. In-place mutation of ToolResult content**
Rationale: `ContentBlock::ToolResult.content` is an owned `String`. We can iterate `&mut history` and replace content directly. No need to reconstruct blocks. Matches Python's dict mutation approach.

**2. Token estimation via serialized length / 4**
Rationale: Direct port of Python's `len(str(messages)) // 4`. Use `serde_json::to_string(&history)` for the char count. Good enough heuristic — not worth adding a tokenizer dependency.

**3. Build tool_use_id → tool_name map from assistant messages**
Rationale: micro_compact needs to know which tool produced each result. Scan `ContentBlock::ToolUse { id, name, .. }` blocks in assistant messages to build a `HashMap<String, String>`. Same approach as Python's `tool_name_map`.

**4. auto_compact makes a separate LLM call**
Rationale: Direct port. Use the same `client.create_message()` with a fresh messages vec containing the conversation text and a summarization prompt. Replace history with 2 messages: user (summary) + assistant (ack).

**5. compact tool is main-agent only**
Rationale: Matches Python. Subagent uses `child_tools()` which stays unchanged. The compact tool has an optional `focus` parameter for what to preserve.

**6. Transcript saved as JSONL**
Rationale: Each `Message` serialized as one JSON line. Filename: `transcript_{unix_timestamp}.jsonl`. Directory: `.transcripts/` under workdir, created on demand.

## Risks / Trade-offs

- [Serialization cost of estimate_tokens] → Called every turn, but `serde_json::to_string` on history is fast enough for this use case
- [Summary quality] → Depends on LLM; same trade-off as Python. Critical details may be lost.
- [Large history serialization for auto_compact prompt] → Truncate to 80000 chars like Python
- [ToolResult content mutation requires &mut history] → agent_loop already takes `&mut Vec<Message>`, so this is fine
