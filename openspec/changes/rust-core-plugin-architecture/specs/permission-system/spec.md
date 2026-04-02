## ADDED Requirements

### Requirement: Permission modes
The system SHALL support three permission modes: `Default` (always ask for non-read-only tools), `AcceptEdits` (auto-allow file edits in working directory, ask for others), `Bypass` (allow all without prompting). The mode SHALL be configurable via settings.json and switchable at runtime.

#### Scenario: Default mode with write tool
- **WHEN** mode is Default and a FileWrite tool is invoked
- **THEN** the system SHALL send a PermissionRequest to the TUI and await user confirmation

#### Scenario: AcceptEdits mode with file edit
- **WHEN** mode is AcceptEdits and a FileEdit tool targets a file within the working directory
- **THEN** the system SHALL auto-allow without prompting

#### Scenario: Bypass mode
- **WHEN** mode is Bypass
- **THEN** all tools SHALL execute without permission prompts

### Requirement: Permission rules with wildcard matching
The system SHALL support permission rules in the format `ToolName` or `ToolName(pattern)` where pattern supports `*` wildcards. Rules SHALL have behavior `allow`, `deny`, or `ask`. Example: `Bash(npm:*)` matches any bash command starting with `npm`.

#### Scenario: Wildcard allow rule
- **WHEN** an allow rule `Bash(git:*)` exists and the bash command is `git status`
- **THEN** the system SHALL auto-allow without prompting

#### Scenario: Deny rule
- **WHEN** a deny rule `Bash(rm -rf:*)` exists and the bash command is `rm -rf /`
- **THEN** the system SHALL deny the tool call and return an error to the model

### Requirement: Permission check flow
For each tool call, the system SHALL: (1) check deny rules first - if matched, deny immediately, (2) check tool's `check_permissions()` method, (3) if result is Ask, check allow rules - if matched, allow, (4) if still Ask, check permission mode - Bypass allows, Default/AcceptEdits may ask user, (5) send PermissionRequest to TUI if needed.

#### Scenario: Deny rule takes precedence
- **WHEN** both a deny rule and an allow rule match the same tool call
- **THEN** the deny rule SHALL take precedence

#### Scenario: Interactive permission request
- **WHEN** no rules match and mode is Default
- **THEN** a PermissionRequest SHALL be sent to the TUI via the QueryEvent channel with a oneshot responder

### Requirement: Permission response options
When the TUI displays a permission prompt, the user SHALL be able to choose: `Allow` (this invocation only), `Deny` (this invocation only), `Always Allow` (adds an allow rule for this tool+pattern to the session). The response SHALL be sent back via the oneshot channel.

#### Scenario: User selects Always Allow
- **WHEN** the user selects "Always Allow" for `Bash(git:*)`
- **THEN** subsequent `git` commands SHALL be auto-allowed for the rest of the session
