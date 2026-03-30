## ADDED Requirements

### Requirement: Token estimation
The system SHALL estimate token count by serializing the message history to JSON and dividing the character count by 4.

#### Scenario: Estimating tokens
- **WHEN** the agent loop runs with message history
- **THEN** the system computes `serde_json::to_string(&history).len() / 4` as the token estimate

### Requirement: Layer 1 — micro_compact trims old tool results
The system SHALL replace the `content` field of old `ToolResult` blocks with `[Previous: used {tool_name}]` before each LLM call. Only tool results with content longer than 100 characters SHALL be replaced. The most recent 3 tool results SHALL be preserved.

#### Scenario: More than 3 tool results in history
- **WHEN** history contains 5 tool_result blocks with content > 100 chars
- **THEN** the oldest 2 are replaced with `[Previous: used {tool_name}]` and the newest 3 are preserved

#### Scenario: 3 or fewer tool results
- **WHEN** history contains 3 or fewer tool_result blocks
- **THEN** no content is replaced

#### Scenario: Tool name lookup
- **WHEN** a tool_result's `tool_use_id` matches a `ToolUse` block's `id` in a prior assistant message
- **THEN** the placeholder uses that tool's name (e.g., `[Previous: used bash]`)

#### Scenario: Unknown tool name
- **WHEN** a tool_result's `tool_use_id` has no matching ToolUse block
- **THEN** the placeholder uses `unknown` as the tool name

### Requirement: Layer 2 — auto_compact at token threshold
The system SHALL trigger auto_compact when the estimated token count exceeds 50000. Auto_compact SHALL save the full transcript to `.transcripts/transcript_{unix_timestamp}.jsonl`, ask the LLM to summarize the conversation, and replace all messages with the summary.

#### Scenario: Token threshold exceeded
- **WHEN** estimated tokens > 50000 before an LLM call
- **THEN** the system saves the transcript, generates a summary, and replaces history with 2 messages: a user message containing the summary and an assistant acknowledgment

#### Scenario: Token threshold not exceeded
- **WHEN** estimated tokens <= 50000
- **THEN** no auto_compact occurs

#### Scenario: Transcript format
- **WHEN** auto_compact saves a transcript
- **THEN** each message is serialized as one JSON line in the `.transcripts/` directory

### Requirement: Layer 3 — compact tool for manual compression
The system SHALL provide a `compact` tool that the model can call to trigger compression on demand. The tool SHALL accept an optional `focus` parameter describing what to preserve. Calling compact SHALL trigger the same auto_compact logic.

#### Scenario: Model calls compact
- **WHEN** the model calls the `compact` tool
- **THEN** the system runs auto_compact, replacing all messages with a summary

#### Scenario: Compact tool result
- **WHEN** the compact tool is called
- **THEN** the tool_result content is "Compressing..." before auto_compact runs

### Requirement: compact tool is main-agent only
The `compact` tool SHALL be available only to the main agent. The subagent tool set (`child_tools()`) SHALL NOT include `compact`.

#### Scenario: Subagent tool set
- **WHEN** a subagent is spawned
- **THEN** its available tools do not include `compact`

### Requirement: REPL prompt update
The REPL prompt SHALL display `s06 >>` to reflect the current course stage.

#### Scenario: User sees updated prompt
- **WHEN** the agent starts
- **THEN** the input prompt shows `s06 >>` instead of `s05 >>`
