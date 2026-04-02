## ADDED Requirements

### Requirement: Tool result formatting pipeline
Each tool SHALL implement a `format_result()` method that converts its output into a string for the API. The engine SHALL wrap this into a `tool_result` content block with `tool_use_id`, `content` (string), and `is_error` (bool). When a tool fails, the result SHALL have `is_error: true` with the error message as content.

#### Scenario: Successful tool result
- **WHEN** the bash tool returns stdout "hello\n"
- **THEN** the tool_result block SHALL have `content: "hello\n"` and `is_error: false`

#### Scenario: Failed tool result
- **WHEN** a file read fails with "File not found: foo.rs"
- **THEN** the tool_result block SHALL have `content: "Error: File not found: foo.rs"` and `is_error: true`

### Requirement: Large result persistence with persisted-output wrapper
When a tool result exceeds `max_result_size_chars` (default 50,000 characters), the full output SHALL be persisted to disk at `.claude/tool-results/{tool_use_id}.txt`. The content sent to the API SHALL be replaced with a `<persisted-output>` wrapper containing: the file path, a preview of the first 2000 bytes, and a note that the full output was saved. Tools MAY override this threshold (e.g., FileRead uses infinity to prevent read-file-read loops).

#### Scenario: Large bash output
- **WHEN** a bash command produces 100KB of output
- **THEN** the full output SHALL be saved to disk and the API SHALL receive a persisted-output summary with the first 2KB as preview

#### Scenario: Read tool exemption
- **WHEN** the FileRead tool returns a large file
- **THEN** the result SHALL NOT be persisted to disk (threshold is infinity) and SHALL be sent directly to the API

### Requirement: System-reminder tag injection
The engine SHALL inject `<system-reminder>` XML tags around context that is system-generated rather than user-authored. This includes: CLAUDE.md content (as user context), skill listings, hook outputs, task/todo reminders, and memory content. The model's system prompt SHALL instruct it that these tags contain system information with no direct relation to the tool results they appear in.

#### Scenario: CLAUDE.md wrapped in system-reminder
- **WHEN** CLAUDE.md content is prepended to the first user message
- **THEN** it SHALL be wrapped in `<system-reminder>...</system-reminder>` tags

#### Scenario: Hook output injection
- **WHEN** a pre-tool-use hook returns additional context
- **THEN** the context SHALL be appended to the tool result wrapped in `<system-reminder>` tags

### Requirement: Message normalization for API
Before sending messages to the API, the engine SHALL normalize them: (1) filter out synthetic/meta messages (internal errors, progress updates), (2) merge consecutive user messages into a single message with concatenated content blocks, (3) ensure tool_result blocks are packed into user messages (role: user), (4) replace cleared/compressed tool results with placeholder text `[Old tool result content cleared]`.

#### Scenario: Consecutive user messages
- **WHEN** two user messages appear consecutively in history (e.g., after a tool result injection)
- **THEN** they SHALL be merged into a single user message before sending to API

#### Scenario: Cleared old results
- **WHEN** context compression has cleared an old tool result
- **THEN** the API SHALL receive `[Old tool result content cleared]` as the tool result content

### Requirement: Todo/task nag reminders
When the TodoWrite or TaskCreate/TaskUpdate tools have not been used for several assistant turns and there are open items, the engine SHALL inject a `<system-reminder>` with a gentle reminder to use task tracking. The reminder SHALL include the current task/todo list contents. The reminder SHALL instruct the model to NEVER mention the reminder to the user.

#### Scenario: Task reminder after 3 turns
- **WHEN** 3 assistant turns have passed without task tool usage and open tasks exist
- **THEN** a `<system-reminder>` SHALL be injected containing: reminder text, current task list, and "Make sure that you NEVER mention this reminder to the user"

### Requirement: Tool result budget enforcement
The engine SHALL enforce a per-message budget on aggregate tool result size. When the total size of tool results in a single user message exceeds the budget, excess results SHALL be persisted to disk and replaced with `<persisted-output>` wrappers. The 3 most recent tool results SHALL always be kept in full.

#### Scenario: Multiple large tool results
- **WHEN** one assistant turn triggers 5 tool calls each returning 20KB
- **THEN** older results SHALL be persisted and only the 3 most recent SHALL remain in full in the message
