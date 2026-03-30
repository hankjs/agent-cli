## Why

Agent conversations hit context limits during long sessions. Following the s06 course pattern, we need a three-layer compression pipeline: micro_compact silently trims old tool results every turn, auto_compact summarizes the full conversation when token count exceeds a threshold, and a compact tool lets the model trigger compression on demand. This keeps the agent working indefinitely without losing critical context.

## What Changes

- Add `estimate_tokens()` function (~4 chars per token heuristic on serialized messages)
- Add `micro_compact()` that replaces old tool_result content (keeping last 3) with `[Previous: used {tool_name}]` placeholders
- Add `auto_compact()` that saves full transcript to `.transcripts/` and asks the LLM to summarize, replacing all messages with the summary
- Add `compact` tool definition (main agent only) that triggers manual compression
- Wire compression into `agent_loop`: micro_compact before each LLM call, auto_compact when tokens > 50000, compact tool triggers auto_compact on demand
- Update REPL prompt from `s05 >>` to `s06 >>`

## Capabilities

### New Capabilities
- `context-compact`: Three-layer context compression — micro_compact trims old tool results, auto_compact summarizes at threshold, compact tool for on-demand compression

### Modified Capabilities

(none)

## Impact

- `src/main.rs`: New functions (estimate_tokens, micro_compact, auto_compact), new tool definition, agent_loop changes, prompt update
- No new crate dependencies (serde_json already available for serialization)
- `.transcripts/` directory created at runtime for transcript storage
- No changes to subagent tools or behavior
