## ADDED Requirements

### Requirement: Conversation loop with tool execution
The QueryEngine SHALL implement a loop: (1) call API with messages + tools + system prompt, (2) stream response events to TUI via mpsc channel, (3) when `stop_reason == "tool_use"`, execute detected tool calls, (4) append tool results as user messages, (5) call API again. The loop SHALL terminate when `stop_reason == "end_turn"` or `stop_reason == "max_tokens"`.

#### Scenario: Simple text response
- **WHEN** the user asks "what is 2+2" and the model responds with text only
- **THEN** the engine SHALL stream TextDelta events and send TurnComplete when message_stop arrives

#### Scenario: Tool use followed by text
- **WHEN** the model calls the bash tool and then provides a text summary
- **THEN** the engine SHALL execute the tool, send the result back to the API, and stream the follow-up text response

#### Scenario: Multiple sequential tool calls
- **WHEN** the model makes 3 consecutive tool-use turns
- **THEN** the engine SHALL execute each, append results, and call the API again each time until end_turn

### Requirement: Message history management
The QueryEngine SHALL maintain a `Vec<Message>` containing the full conversation history. User messages, assistant messages (with content blocks), and tool results SHALL all be stored. The history SHALL be passed to each API call.

#### Scenario: History accumulates correctly
- **WHEN** the user sends 3 messages with tool calls in between
- **THEN** the messages vec SHALL contain all user messages, assistant responses, and tool results in chronological order

### Requirement: QueryEvent channel communication
The QueryEngine SHALL send all events through a `mpsc::Sender<QueryEvent>`. Event types SHALL include: `TextDelta(String)`, `ThinkingDelta(String)`, `ToolStart { id, name, input }`, `ToolComplete { id, name, output }`, `PermissionRequest { tool_name, input, respond: oneshot::Sender }`, `SpinnerMode(SpinnerMode)`, `TurnComplete`, `Error(String)`.

#### Scenario: Streaming text to TUI
- **WHEN** a `text_delta` SSE event arrives
- **THEN** the engine SHALL immediately send `QueryEvent::TextDelta(text)` through the channel

#### Scenario: Permission request flow
- **WHEN** a tool's `check_permissions()` returns Ask
- **THEN** the engine SHALL send `QueryEvent::PermissionRequest` with a oneshot sender, then await the user's response before proceeding

### Requirement: Context compression
The QueryEngine SHALL estimate token count of the message history. When the estimated count exceeds a configurable threshold (default 100,000 tokens), the engine SHALL compress older messages by replacing them with a summary. The 3 most recent message pairs SHALL be preserved uncompressed.

#### Scenario: Auto-compact triggers
- **WHEN** message history exceeds 100K estimated tokens
- **THEN** the engine SHALL summarize older messages and replace them with the summary, keeping the 3 most recent exchanges

#### Scenario: Manual compact via command
- **WHEN** the user runs `/compact`
- **THEN** the engine SHALL immediately perform context compression regardless of current token count
