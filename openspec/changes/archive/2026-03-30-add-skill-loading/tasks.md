## 1. SkillLoader struct

- [x] 1.1 Add `SkillLoader` struct with `skills: HashMap<String, Skill>` where Skill holds meta (HashMap<String, String>) and body (String)
- [x] 1.2 Implement `new(skills_dir: &Path)` that scans `skills/*/SKILL.md` and parses each file
- [x] 1.3 Implement `parse_frontmatter(text: &str)` — split on `---` delimiters, extract key-value pairs via `split_once(':')`
- [x] 1.4 Implement `get_descriptions()` — returns Layer 1 text for system prompt
- [x] 1.5 Implement `get_content(name: &str)` — returns `<skill>` wrapped body or error with available names

## 2. Tool definition and wiring

- [x] 2.1 Add `load_skill_tool()` fn returning Tool definition (name, description, input_schema with "name" param)
- [x] 2.2 Add `load_skill_tool()` to main agent's tools vec in `agent_loop` (not to `child_tools()`)
- [x] 2.3 Add `"load_skill"` match arm in agent_loop's tool dispatch, calling `skill_loader.get_content()`

## 3. System prompt and plumbing

- [x] 3.1 Initialize `SkillLoader` in `main()` with `skills/` directory
- [x] 3.2 Update system prompt to include `skill_loader.get_descriptions()` and instruction to use `load_skill`
- [x] 3.3 Pass `&SkillLoader` to `agent_loop` (add parameter to function signature)
- [x] 3.4 Change REPL prompt from `s03 >>` to `s05 >>`
