## ADDED Requirements

### Requirement: Async event loop with tokio::select
The TUI SHALL run a main loop that uses `tokio::select!` to simultaneously handle: (1) crossterm keyboard events via `EventStream`, (2) QueryEvent messages from the engine via mpsc channel, (3) tick events at 30 FPS for spinner animation. All three sources SHALL be multiplexed without blocking.

#### Scenario: Typing while streaming
- **WHEN** the engine is streaming a response and the user presses keys
- **THEN** keyboard events SHALL be processed immediately without waiting for the stream to complete

### Requirement: Three-zone vertical layout
The TUI SHALL render three zones: (1) scrollable message display area (fills available height), (2) text input box (3 lines height), (3) status bar (1 line height). The layout SHALL use ratatui's `Layout` with `Constraint::Min(1)`, `Constraint::Length(3)`, `Constraint::Length(1)`.

#### Scenario: Terminal resize
- **WHEN** the terminal is resized
- **THEN** the message area SHALL expand/shrink while input and status bar maintain fixed height

### Requirement: Scrollable message display
The message display SHALL render the conversation as a ratatui `Paragraph` with `scroll((offset, 0))`. User messages SHALL be styled with cyan color. Assistant messages SHALL be styled with white. Tool executions SHALL show tool name in yellow. The display SHALL auto-scroll to bottom during streaming and allow manual scroll via PageUp/PageDown.

#### Scenario: Auto-scroll during streaming
- **WHEN** the engine sends TextDelta events
- **THEN** the scroll offset SHALL automatically track the bottom of the content

#### Scenario: Manual scroll during streaming
- **WHEN** the user presses PageUp while streaming
- **THEN** the display SHALL scroll up and stop auto-scrolling until the user scrolls back to bottom

### Requirement: Text input with tui-textarea
The input area SHALL use `tui-textarea::TextArea` for text editing. Enter SHALL submit the message. The input SHALL be cleared after submission. During streaming (engine busy), keyboard input other than Ctrl+C SHALL be buffered in the textarea.

#### Scenario: Submit message
- **WHEN** the user types "hello" and presses Enter
- **THEN** the text SHALL be sent to QueryEngine::submit() and the input box SHALL be cleared

#### Scenario: Ctrl+C during streaming
- **WHEN** the user presses Ctrl+C while the engine is streaming
- **THEN** the current query SHALL be cancelled (abort signal sent to engine)

### Requirement: Permission confirmation popup
When the TUI receives a `QueryEvent::PermissionRequest`, it SHALL display a centered modal overlay using `Clear` widget + `centered_rect`. The popup SHALL show the tool name, a description of the action, and options: `[Y] Allow`, `[N] Deny`, `[A] Always Allow`. The popup SHALL capture all keyboard input until dismissed.

#### Scenario: User allows
- **WHEN** the permission popup is shown and the user presses Y
- **THEN** the TUI SHALL send `true` through the oneshot channel and dismiss the popup

#### Scenario: User denies
- **WHEN** the permission popup is shown and the user presses N
- **THEN** the TUI SHALL send `false` through the oneshot channel and dismiss the popup

### Requirement: Spinner status bar
The status bar SHALL display a throbber spinner animation (via throbber-widgets-tui) with a label reflecting the current engine state. Labels SHALL change based on `QueryEvent::SpinnerMode`: `Requesting` → "Waiting for API...", `Thinking` → "Thinking...", `Responding` → "Responding...", `ToolExecuting` → "Running {tool_name}...". When idle, the status bar SHALL show session info (message count, model name).

#### Scenario: Spinner during API call
- **WHEN** the engine sends `SpinnerMode::Requesting`
- **THEN** the status bar SHALL show an animated spinner with "Waiting for API..."

#### Scenario: Idle status
- **WHEN** no query is active
- **THEN** the status bar SHALL show "hank | {n} messages | {model}"
