## ADDED Requirements

### Requirement: Bash tool with command execution
The Bash tool SHALL execute shell commands via `tokio::process::Command::new("bash").arg("-c").arg(command)`. It SHALL capture stdout and stderr separately. Output SHALL be truncated to 50,000 characters. The tool SHALL support a configurable timeout (default 120 seconds).

#### Scenario: Simple command
- **WHEN** the model calls bash with `{"command": "echo hello"}`
- **THEN** the tool SHALL return `"hello\n"` as stdout

#### Scenario: Command timeout
- **WHEN** the command runs longer than the timeout
- **THEN** the process SHALL be killed and the tool SHALL return an error indicating timeout

#### Scenario: Non-zero exit code
- **WHEN** the command exits with code 1
- **THEN** the tool SHALL include both stdout and stderr in the result (not treat it as a tool error)

### Requirement: Bash dangerous command blocking
The Bash tool SHALL maintain a deny list of dangerous command patterns: `rm -rf /`, `sudo`, `shutdown`, `reboot`, `> /dev/`. If the command matches any pattern, `check_permissions()` SHALL return `PermissionDecision::Deny` with a descriptive message.

#### Scenario: Dangerous command blocked
- **WHEN** the model calls bash with `{"command": "rm -rf /"}`
- **THEN** the tool SHALL deny execution and return "Blocked: dangerous command"

### Requirement: FileRead tool
The FileRead tool SHALL read file contents and return them with line numbers (format: `{line_number}\t{content}`). It SHALL support optional `offset` (starting line, 0-based) and `limit` (max lines to read, default 2000). The `file_path` parameter SHALL be resolved relative to the working directory. Non-existent files SHALL return a tool error.

#### Scenario: Read entire file
- **WHEN** the model calls read with `{"file_path": "src/main.rs"}`
- **THEN** the tool SHALL return the file contents with line numbers, up to 2000 lines

#### Scenario: Read with offset and limit
- **WHEN** the model calls read with `{"file_path": "big.txt", "offset": 100, "limit": 50}`
- **THEN** the tool SHALL return lines 100-149 with line numbers

#### Scenario: File not found
- **WHEN** the model calls read with a non-existent file path
- **THEN** the tool SHALL return a ToolError with "File not found: {path}"

### Requirement: FileWrite tool
The FileWrite tool SHALL write content to a file at the specified path, creating parent directories if needed. The `file_path` SHALL be resolved relative to the working directory. The tool SHALL validate that the path is within the working directory (no directory traversal via `..`).

#### Scenario: Write new file
- **WHEN** the model calls write with `{"file_path": "new.txt", "content": "hello"}`
- **THEN** the file SHALL be created with the given content

#### Scenario: Path traversal blocked
- **WHEN** the model calls write with `{"file_path": "../../etc/passwd", "content": "x"}`
- **THEN** the tool SHALL return a ToolError indicating path safety violation

### Requirement: FileEdit tool with unique match validation
The FileEdit tool SHALL accept `file_path`, `old_string`, and `new_string`. It SHALL read the file, verify that `old_string` appears exactly once (unless `replace_all: true`), replace it with `new_string`, and write the file back. If `old_string` is not found or appears multiple times (without `replace_all`), the tool SHALL return a descriptive error.

#### Scenario: Successful edit
- **WHEN** `old_string` appears exactly once in the file
- **THEN** the tool SHALL replace it with `new_string` and return the diff

#### Scenario: String not found
- **WHEN** `old_string` does not appear in the file
- **THEN** the tool SHALL return a ToolError "old_string not found in file"

#### Scenario: Multiple matches without replace_all
- **WHEN** `old_string` appears 3 times and `replace_all` is not set
- **THEN** the tool SHALL return a ToolError indicating the string is not unique (found 3 times)

#### Scenario: Replace all matches
- **WHEN** `old_string` appears 3 times and `replace_all: true`
- **THEN** all 3 occurrences SHALL be replaced

### Requirement: Glob tool for file search
The Glob tool SHALL accept a `pattern` (glob syntax like `**/*.rs`) and optional `path` (search root, defaults to working directory). It SHALL return a list of matching file paths sorted by modification time. Results SHALL be limited to 1000 files.

#### Scenario: Search for Rust files
- **WHEN** the model calls glob with `{"pattern": "**/*.rs"}`
- **THEN** the tool SHALL return all `.rs` files under the working directory

#### Scenario: No matches
- **WHEN** the pattern matches no files
- **THEN** the tool SHALL return "No files found"

### Requirement: Grep tool for content search
The Grep tool SHALL accept a `pattern` (regex) and optional `path` (search root), `glob` (file filter), and `context` (lines before/after). It SHALL search file contents and return matching lines with file paths and line numbers. Results SHALL be limited to 1000 matches.

#### Scenario: Search for function definition
- **WHEN** the model calls grep with `{"pattern": "fn main", "glob": "*.rs"}`
- **THEN** the tool SHALL return matching lines with file paths and line numbers

#### Scenario: Case insensitive search
- **WHEN** the model calls grep with `{"pattern": "error", "-i": true}`
- **THEN** the search SHALL match "Error", "ERROR", "error" etc.
