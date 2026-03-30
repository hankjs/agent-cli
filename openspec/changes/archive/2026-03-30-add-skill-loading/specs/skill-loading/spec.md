## ADDED Requirements

### Requirement: Skill discovery at startup
The system SHALL scan the `skills/` directory for subdirectories containing `SKILL.md` files at startup. Each file SHALL be parsed for YAML frontmatter (between `---` delimiters) extracting `name`, `description`, and `tags` fields, with the remaining content stored as the skill body.

#### Scenario: Skills directory with valid skills
- **WHEN** the agent starts and `skills/` contains `pdf/SKILL.md` and `code-review/SKILL.md`
- **THEN** both skills are loaded with their metadata and body content

#### Scenario: Missing skills directory
- **WHEN** the agent starts and `skills/` directory does not exist
- **THEN** the system loads zero skills and continues normally

#### Scenario: SKILL.md without frontmatter
- **WHEN** a `SKILL.md` has no `---` delimiters
- **THEN** the entire file content is treated as body, metadata is empty, and the directory name is used as the skill name

### Requirement: Layer 1 — skill descriptions in system prompt
The system SHALL inject a summary of all available skills into the system prompt. Each skill entry SHALL include the skill name and description. If tags are present, they SHALL be appended in brackets.

#### Scenario: System prompt with skills
- **WHEN** skills are loaded
- **THEN** the system prompt includes a "Skills available:" section with one line per skill formatted as `  - <name>: <description> [<tags>]`

#### Scenario: No skills loaded
- **WHEN** no skills are found
- **THEN** the system prompt includes "(no skills available)"

### Requirement: Layer 2 — load_skill tool
The system SHALL provide a `load_skill` tool that accepts a `name` parameter and returns the full skill body wrapped in `<skill name="...">` XML tags.

#### Scenario: Loading an existing skill
- **WHEN** the model calls `load_skill` with name "pdf"
- **THEN** the tool returns `<skill name="pdf">\n{full body}\n</skill>`

#### Scenario: Loading an unknown skill
- **WHEN** the model calls `load_skill` with a name that doesn't exist
- **THEN** the tool returns an error message listing available skill names

### Requirement: load_skill is main-agent only
The `load_skill` tool SHALL be available only to the main agent. The subagent tool set (`child_tools()`) SHALL NOT include `load_skill`.

#### Scenario: Subagent tool set
- **WHEN** a subagent is spawned
- **THEN** its available tools are bash, read_file, write_file, edit_file (no load_skill)

### Requirement: REPL prompt update
The REPL prompt SHALL display `s05 >>` to reflect the current course stage.

#### Scenario: User sees updated prompt
- **WHEN** the agent starts
- **THEN** the input prompt shows `s05 >>` instead of `s03 >>`
