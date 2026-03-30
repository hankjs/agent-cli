## Context

The Rust agent CLI (`src/main.rs`) currently has bash, read_file, write_file, edit_file, todo, and task tools. It follows the Python course progression (s01→s05). The Python s05 introduces a two-layer skill loading system. The `skills/` directory already exists with 4 skills (pdf, code-review, mcp-builder, agent-builder), each containing a `SKILL.md` with YAML frontmatter.

## Goals / Non-Goals

**Goals:**
- Port the Python `SkillLoader` pattern to Rust
- Parse `skills/*/SKILL.md` frontmatter at startup
- Inject skill descriptions into system prompt (Layer 1)
- Provide `load_skill` tool for on-demand full content (Layer 2)
- Keep parity with Python s05 behavior

**Non-Goals:**
- Hot-reloading skills at runtime
- Giving subagent access to load_skill
- Adding new skills (existing 4 are sufficient)
- YAML library dependency (manual parsing is fine)

## Decisions

**1. Manual frontmatter parsing (no serde_yaml)**
Rationale: The Python version does simple `split(":", 1)` parsing. The frontmatter is trivial (name, description, tags). Adding a YAML crate for 3 key-value pairs is overkill. Match the Python approach.

**2. SkillLoader as immutable struct**
Rationale: Skills are loaded once at startup and never mutated. Pass as `&SkillLoader` to agent_loop (not `&mut`). This is simpler than TodoManager which needs `&mut`.

**3. load_skill only on main agent**
Rationale: Matches Python s05. Subagent has `child_tools()` which stays unchanged. The main agent's tool vec adds `load_skill_tool()`.

**4. Skill content wrapped in `<skill>` XML tags**
Rationale: Direct port of Python's `get_content()` format: `<skill name="X">...body...</skill>`. This gives the model clear boundaries for the injected knowledge.

## Risks / Trade-offs

- [Frontmatter with colons in values] → First `split_once(':')` handles this (value can contain colons)
- [Missing SKILL.md files] → Graceful: empty skills map, system prompt says "(no skills available)"
- [Large skill bodies] → Not truncated (matches Python). Skills are author-controlled, not user input.
