## 1. Compression functions

- [x] 1.1 Add `estimate_tokens(history: &[Message]) -> usize` — serialize to JSON, len / 4
- [x] 1.2 Add `micro_compact(history: &mut Vec<Message>)` — build tool_use_id→name map from assistant ToolUse blocks, find all ToolResult blocks, replace content of all but last 3 (if content > 100 chars) with `[Previous: used {name}]`
- [x] 1.3 Add `auto_compact(client, model, history) -> Vec<Message>` — save transcript to `.transcripts/transcript_{timestamp}.jsonl`, call LLM to summarize (truncate input to 80000 chars), return 2-message replacement (user summary + assistant ack)

## 2. Tool definition and wiring

- [x] 2.1 Add `compact_tool()` fn returning Tool definition (name, description, input_schema with optional "focus" param)
- [x] 2.2 Add `compact_tool()` to main agent's tools vec in `agent_loop` (not to `child_tools()`)
- [x] 2.3 Add `"compact"` match arm in agent_loop's tool dispatch, returning "Compressing..." and setting a `manual_compact` flag

## 3. Agent loop integration

- [x] 3.1 Call `micro_compact(&mut history)` at the top of each loop iteration (before LLM call)
- [x] 3.2 After micro_compact, check `estimate_tokens` > 50000 and call `auto_compact` if exceeded, replacing history in-place
- [x] 3.3 After processing tool results, if `manual_compact` flag is set, call `auto_compact` and replace history in-place
- [x] 3.4 Make `agent_loop` signature changes as needed (auto_compact needs `&AnthropicClient` and model, already available)

## 4. Plumbing and prompt

- [x] 4.1 Add `THRESHOLD` and `KEEP_RECENT` constants (50000 and 3)
- [x] 4.2 Change REPL prompt from `s05 >>` to `s06 >>`
