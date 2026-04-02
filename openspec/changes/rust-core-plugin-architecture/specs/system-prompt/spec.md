## ADDED Requirements

### Requirement: Layered system prompt assembly
The system SHALL build the system prompt from these layers in order: (1) base identity and instructions template, (2) tool descriptions from ToolRegistry, (3) CLAUDE.md file contents, (4) git context (branch, status, recent commits), (5) current date. Each layer SHALL be a separate text block concatenated with newline separators.

#### Scenario: Full prompt assembly
- **WHEN** a query is submitted with tools registered and CLAUDE.md present
- **THEN** the system prompt SHALL contain the base template, tool descriptions, CLAUDE.md content, git status, and current date

#### Scenario: No CLAUDE.md file
- **WHEN** no CLAUDE.md file is found in any search location
- **THEN** the system prompt SHALL omit the CLAUDE.md layer and assemble remaining layers normally

### Requirement: CLAUDE.md file discovery
The system SHALL search for CLAUDE.md files in this order: (1) user home `~/.claude/CLAUDE.md`, (2) walk from working directory upward to filesystem root checking each directory for `CLAUDE.md` and `.claude/CLAUDE.md`, (3) project-local `.claude.local/CLAUDE.md`. All found files SHALL be concatenated with clear separators indicating their source path.

#### Scenario: User and project CLAUDE.md
- **WHEN** both `~/.claude/CLAUDE.md` and `./CLAUDE.md` exist
- **THEN** both SHALL be included in the prompt, user-level first, project-level second

#### Scenario: Nested project directories
- **WHEN** working directory is `/a/b/c` and CLAUDE.md exists at `/a/CLAUDE.md`
- **THEN** the discovery SHALL find it by walking upward from `/a/b/c`

### Requirement: Git context collection
The system SHALL collect git information when the working directory is a git repository: current branch name, remote tracking status, `git status` output (truncated to 2KB), and the 5 most recent commit messages. This context SHALL be included in the system prompt as structured text.

#### Scenario: Git repository
- **WHEN** the working directory is a git repository
- **THEN** the system prompt SHALL include branch name, status summary, and recent commits

#### Scenario: Not a git repository
- **WHEN** the working directory is not a git repository
- **THEN** the git context layer SHALL be omitted entirely

### Requirement: Tool descriptions in prompt
The system SHALL generate a tool description section from all registered tools. Each tool SHALL be listed with its name and description text. This section SHALL be placed after the base template and before CLAUDE.md content.

#### Scenario: Tool description format
- **WHEN** 6 tools are registered (bash, read, write, edit, glob, grep)
- **THEN** the tool description section SHALL list all 6 with their names and descriptions
