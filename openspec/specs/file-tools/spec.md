## ADDED Requirements

### Requirement: Path safety
The system SHALL resolve all file paths relative to the working directory and MUST reject any path that escapes the workspace root after resolution.

#### Scenario: Relative path within workspace
- **WHEN** a tool receives path "src/lib.rs"
- **THEN** the system resolves it to `{workdir}/src/lib.rs` and allows the operation

#### Scenario: Path traversal attempt
- **WHEN** a tool receives path "../../etc/passwd"
- **THEN** the system returns an error "Path escapes workspace" and does not perform the operation

### Requirement: read_file tool
The system SHALL provide a `read_file` tool that reads file contents given a `path` (required) and optional `limit` (integer, max lines to return).

#### Scenario: Read entire file
- **WHEN** read_file is called with path "src/main.rs" and no limit
- **THEN** the system returns the file contents, truncated to 50000 characters

#### Scenario: Read with line limit
- **WHEN** read_file is called with path "src/main.rs" and limit 10
- **THEN** the system returns the first 10 lines followed by "... (N more lines)" where N is the remaining line count

#### Scenario: File not found
- **WHEN** read_file is called with a path that does not exist
- **THEN** the system returns an error message

### Requirement: write_file tool
The system SHALL provide a `write_file` tool that writes `content` (required string) to `path` (required string), creating parent directories as needed.

#### Scenario: Write new file
- **WHEN** write_file is called with path "new_dir/file.txt" and content "hello"
- **THEN** the system creates parent directories, writes the file, and returns a confirmation with byte count

#### Scenario: Overwrite existing file
- **WHEN** write_file is called with path to an existing file and new content
- **THEN** the system overwrites the file and returns a confirmation

### Requirement: edit_file tool
The system SHALL provide an `edit_file` tool that replaces the first occurrence of `old_text` (required) with `new_text` (required) in the file at `path` (required).

#### Scenario: Successful edit
- **WHEN** edit_file is called and old_text exists in the file
- **THEN** the system replaces the first occurrence and returns "Edited {path}"

#### Scenario: Text not found
- **WHEN** edit_file is called and old_text does not exist in the file
- **THEN** the system returns "Error: Text not found in {path}"

### Requirement: Tool dispatch routing
The agent_loop SHALL dispatch tool calls by matching on the tool name, routing to the appropriate handler for bash, read_file, write_file, and edit_file. Unknown tool names SHALL return "Unknown tool: {name}".

#### Scenario: Known tool dispatch
- **WHEN** the model returns a tool_use block with name "read_file"
- **THEN** the system routes to the read_file handler

#### Scenario: Unknown tool
- **WHEN** the model returns a tool_use block with an unrecognized name
- **THEN** the system returns "Unknown tool: {name}" as the tool result

### Requirement: Updated system prompt
The system prompt SHALL reference "tools" (not just "bash") and the REPL prompt marker SHALL display "s02" instead of "s01".

#### Scenario: Prompt text
- **WHEN** the CLI starts
- **THEN** the system prompt says "Use tools to solve tasks" and the input prompt shows "s02 >>"
