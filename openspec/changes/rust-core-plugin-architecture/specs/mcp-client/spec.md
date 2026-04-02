## ADDED Requirements

### Requirement: MCP server subprocess management
The MCP client SHALL spawn MCP servers as child processes using `tokio::process::Command` with stdin/stdout piped. Each server process SHALL be kept alive for the duration of the session. The client SHALL send JSON-RPC 2.0 messages over stdin and read responses from stdout (newline-delimited JSON).

#### Scenario: Spawning an MCP server
- **WHEN** the configuration specifies `{ "command": "node", "args": ["server.js"] }`
- **THEN** the client SHALL spawn `node server.js` with piped stdio and maintain the process handle

#### Scenario: Server process crash
- **WHEN** an MCP server process exits unexpectedly
- **THEN** the client SHALL log a warning and remove the server's tools from the registry

### Requirement: MCP initialize handshake
After spawning, the client SHALL send an `initialize` request with `protocolVersion: "2024-11-05"`, `capabilities: {}`, and `clientInfo: { name: "hank-cli", version: "0.1.0" }`. After receiving the response, the client SHALL send an `initialized` notification. Tool discovery SHALL only proceed after successful initialization.

#### Scenario: Successful initialization
- **WHEN** the server responds to initialize with its capabilities
- **THEN** the client SHALL send `notifications/initialized` and proceed to tool discovery

#### Scenario: Initialization timeout
- **WHEN** the server does not respond to initialize within 10 seconds
- **THEN** the client SHALL kill the process and log an error

### Requirement: Tool discovery via tools/list
The client SHALL send a `tools/list` JSON-RPC request to discover available tools. Each tool in the response SHALL have `name`, `description`, and `inputSchema` fields. The client SHALL convert each to an `McpTool` struct that implements `trait Tool`.

#### Scenario: Server provides 3 tools
- **WHEN** the server responds to `tools/list` with 3 tool definitions
- **THEN** the client SHALL create 3 `McpTool` instances and register them in the ToolRegistry

### Requirement: Tool execution via tools/call
The `McpTool` adapter SHALL implement `Tool::call()` by sending a `tools/call` JSON-RPC request with `name` and `arguments` fields. The response content array SHALL be concatenated (text blocks joined with newlines) and returned as the tool result.

#### Scenario: Successful tool call
- **WHEN** the engine dispatches a call to an MCP tool
- **THEN** the adapter SHALL send `tools/call` to the server and return the response content as a string

### Requirement: MCP tool name prefixing
MCP tools SHALL be registered with names prefixed as `mcp__{server_name}__{tool_name}` to avoid collisions with built-in tools. The adapter SHALL strip the prefix when sending `tools/call` to the server (sending only the original tool name).

#### Scenario: Name prefixing
- **WHEN** server "filesystem" provides tool "read_file"
- **THEN** it SHALL be registered as `mcp__filesystem__read_file` in the ToolRegistry

### Requirement: MCP configuration loading
The client SHALL read MCP server configurations from `~/.config/hank/mcp.json` with format `{ "mcpServers": { "name": { "command": "...", "args": [...], "env": {...} } } }`. Each entry SHALL be connected asynchronously at startup. Missing config file SHALL be treated as no MCP servers (not an error).

#### Scenario: Config with two servers
- **WHEN** mcp.json defines servers "filesystem" and "github"
- **THEN** both SHALL be spawned and initialized concurrently at startup

#### Scenario: No config file
- **WHEN** `~/.config/hank/mcp.json` does not exist
- **THEN** startup SHALL continue with no MCP tools (zero tools from MCP)
