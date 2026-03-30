## Why

The agent's system prompt grows linearly with domain knowledge. Following the s05 course pattern, we need a two-layer skill injection system: cheap metadata in the system prompt (~100 tokens/skill), full skill body loaded on demand via tool_result. This keeps the system prompt lean while giving the model access to specialized knowledge when needed.

## What Changes

- Add `SkillLoader` struct that scans `skills/*/SKILL.md` at startup, parsing YAML frontmatter (name, description, tags) and body
- Inject skill descriptions (Layer 1) into the system prompt
- Add `load_skill` tool to the main agent's tool set (not subagent)
- Handle `load_skill` in agent_loop, returning full skill body wrapped in `<skill>` tags (Layer 2)
- Update REPL prompt from `s03 >>` to `s05 >>`

## Capabilities

### New Capabilities
- `skill-loading`: Two-layer skill injection — metadata in system prompt, full body on demand via load_skill tool

### Modified Capabilities

(none — subagent and file-tools unchanged)

## Impact

- `src/main.rs`: New struct, new tool definition, agent_loop signature change, system prompt change
- No new crate dependencies (frontmatter parsed manually)
- No changes to subagent tools or behavior
