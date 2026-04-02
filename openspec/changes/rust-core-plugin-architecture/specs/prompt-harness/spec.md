## ADDED Requirements

### Requirement: Base system prompt with all Claude Code sections
The system SHALL include a complete system prompt assembled from these sections, faithfully ported from Claude Code: (1) Identity/Intro with cyber risk instruction, (2) System section covering tool execution, permission modes, system-reminder tags, hooks, context compression, (3) Doing-tasks section with all coding guidelines (read before modify, avoid over-engineering, no backwards-compat hacks, security awareness), (4) Executing-actions-with-care section with reversibility/blast-radius guidance, (5) Using-your-tools section with dedicated tool preference rules, (6) Tone-and-style section (no emojis, concise, file_path:line_number references). All text SHALL be stored as Rust string constants in a `prompts` module.

#### Scenario: Full prompt sections present
- **WHEN** the system prompt is assembled for an API call
- **THEN** all 6 base sections SHALL be present in order: identity, system, doing-tasks, actions, tools, tone

#### Scenario: Tool preference rules
- **WHEN** the model receives the system prompt
- **THEN** the "Using your tools" section SHALL instruct it to prefer Read over cat, Edit over sed, Write over echo, Glob over find, Grep over grep

### Requirement: Tool-specific prompt text
Each built-in tool SHALL have a `prompt()` method returning the full Claude Code description text. The Bash tool prompt SHALL include: basic description, instructions section (directory verification, path quoting, timeout, parallel commands, git commands safety protocol, sleep avoidance), git commit workflow instructions, and pull request creation instructions. The Read tool prompt SHALL include: absolute path requirement, 2000 line default, line number format, image/PDF support notes. The Edit tool prompt SHALL include: read-before-edit requirement, indentation preservation, unique match requirement, replace_all usage. The Write tool prompt SHALL include: overwrite warning, read-first requirement, prefer-Edit note. The Glob and Grep prompts SHALL include their respective usage patterns.

#### Scenario: Bash tool git commit instructions
- **WHEN** the model is asked to commit code
- **THEN** the system prompt SHALL contain the full git safety protocol (never force-push, never skip hooks, always new commit not amend, HEREDOC format for messages)

#### Scenario: Bash tool PR instructions
- **WHEN** the model is asked to create a pull request
- **THEN** the system prompt SHALL contain the full gh-based PR workflow with parallel status checks and HEREDOC body format

### Requirement: Environment info section
The system prompt SHALL include an environment section with: working directory path, git repository status (yes/no), platform (darwin/linux/windows), shell info, OS version, model name and ID, current date. This section SHALL be dynamically generated at session start.

#### Scenario: Git repository environment
- **WHEN** the working directory is a git repository
- **THEN** the environment section SHALL show "Is a git repository: true"

### Requirement: CLAUDE.md content injection
CLAUDE.md file contents SHALL be injected as user context, prepended to the first user message wrapped in `<system-reminder>` tags with the format: "As you answer the user's questions, you can use the following context:" followed by key-value sections for claudeMd and currentDate.

#### Scenario: CLAUDE.md present
- **WHEN** a CLAUDE.md file is found at project root
- **THEN** its content SHALL appear in a `<system-reminder>` block before the first user message

### Requirement: Git context in system prompt
Git status information SHALL be appended to the system prompt as structured text, not wrapped in XML tags. It SHALL include: current branch, main/default branch, git status output (truncated to 2KB), and 5 most recent commit summaries.

#### Scenario: Git status appended
- **WHEN** the working directory is a git repository
- **THEN** git context SHALL be appended after the base prompt sections as plain key:value format
