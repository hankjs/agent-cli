## ADDED Requirements

### Requirement: Tool trait definition
The system SHALL define an `#[async_trait] pub trait Tool: Send + Sync` in `hank-core` with methods: `name() -> &str`, `description() -> &str`, `input_schema() -> Value`, `call(input, ctx) -> Result<ToolResult>`, `format_result(result) -> String`. Default-implemented methods SHALL include `is_concurrency_safe(input) -> bool` (default false), `is_read_only(input) -> bool` (default false), `validate_input(input) -> Result<()>` (default Ok), `check_permissions(input, ctx) -> PermissionDecision` (default Allow).

#### Scenario: Implementing a read-only tool
- **WHEN** a tool implementation overrides `is_read_only()` to return true
- **THEN** the executor SHALL allow it to run concurrently with other read-only tools

#### Scenario: Default permission behavior
- **WHEN** a tool does not override `check_permissions()`
- **THEN** the default SHALL return `PermissionDecision::Allow`

### Requirement: ToolRegistry for registration and lookup
The system SHALL provide a `ToolRegistry` struct that stores `Vec<Arc<dyn Tool>>`. It SHALL support `register(tool)` to add a tool, `get(name) -> Option<Arc<dyn Tool>>` for lookup by name, `api_definitions()` to generate the Anthropic API tool definition array, and `merge(new_tools)` for hot-adding MCP tools at runtime.

#### Scenario: Registering built-in tools at startup
- **WHEN** `hank_tools::register_all(&mut registry)` is called
- **THEN** all built-in tools SHALL be findable via `registry.get("bash")` etc.

#### Scenario: MCP tools merged mid-session
- **WHEN** `registry.merge(mcp_tools)` is called with newly discovered MCP tools
- **THEN** subsequent `get()` and `api_definitions()` calls SHALL include the new tools

#### Scenario: Name collision on merge
- **WHEN** an MCP tool has the same name as a built-in tool
- **THEN** the built-in tool SHALL take precedence (MCP tool is skipped)

### Requirement: ToolExecutor with concurrency control
The system SHALL provide a `ToolExecutor` that dispatches tool calls. When multiple tool calls arrive in one assistant response, the executor SHALL check `is_concurrency_safe()` for each. If ALL pending tools are concurrency-safe, they SHALL execute in parallel (tokio::join). Otherwise, they SHALL execute sequentially in order.

#### Scenario: Two read-only tools in same response
- **WHEN** the assistant calls both `glob` and `grep` (both concurrency-safe)
- **THEN** both tools SHALL execute concurrently

#### Scenario: Mixed read-write tools in same response
- **WHEN** the assistant calls `file_read` (concurrency-safe) and `bash` (not concurrency-safe)
- **THEN** all tools SHALL execute sequentially in order

### Requirement: ToolResult structure
The `call()` method SHALL return `Result<ToolResult, ToolError>` where `ToolResult` contains `data: Value` (the tool output) and optionally `new_messages: Vec<Message>` (side-channel messages to inject into conversation).

#### Scenario: Successful tool execution
- **WHEN** a tool completes successfully
- **THEN** the ToolResult.data SHALL contain the tool-specific output as serde_json::Value

#### Scenario: Tool execution failure
- **WHEN** a tool fails (e.g., file not found)
- **THEN** a ToolError SHALL be returned and the engine SHALL send an `is_error: true` tool_result to the API
