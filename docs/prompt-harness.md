# Claude Code Prompt & Harness 设计全览

源码路径: `claude-code-source-code/`

---

## 一、System Prompt 组装

### 1.1 模块化 Section 系统

**文件:** `src/constants/prompts.ts` (~59KB)

System prompt 由多个 section 拼接而成，通过两种 helper 注册:

- `systemPromptSection(name, compute)` — 缓存型，直到 `/clear` 或 `/compact` 才重算
- `DANGEROUS_uncachedSystemPromptSection(name, compute, reason)` — 每轮重算，会打破 prompt cache

**6 大基础 Section:**

| Section | 内容 |
|---------|------|
| **Intro** | 身份声明 ("You are Claude Code, Anthropic's official CLI") + `CYBER_RISK_INSTRUCTION` 安全指令 |
| **System** | 工具使用规则、权限模式、system-reminder 标签说明、hook 系统、上下文压缩说明 |
| **DoingTasks** | 编码准则: 先读再改、避免过度工程、不加不必要的注释/类型/docstring、安全意识、不给时间估计 |
| **Actions** | 可逆性/影响范围评估、危险操作列表 (force-push, rm -rf, reset --hard 等)、"measure twice, cut once" |
| **UsingTools** | 专用工具优先: Read 不用 cat, Edit 不用 sed, Glob 不用 find, Grep 不用 grep; 并行调用指导 |
| **ToneStyle** | 不用 emoji、简洁、用 `file_path:line_number` 格式引用代码 |

### 1.2 Cache 分割

**文件:** `src/utils/api.ts`

- `SYSTEM_PROMPT_DYNAMIC_BOUNDARY` 标记静态/动态内容边界
- `splitSysPromptPrefix()` 将 prompt 切分为 `SystemPromptBlock[]`，每块带 `cacheScope: 'global' | 'org' | null`
- `buildSystemPromptBlocks()` 转换为 API 的 `TextBlockParam[]` 并附加 `cache_control`

### 1.3 动态内容注入

- **Git 上下文:** branch、status (截断 2KB)、5 条 recent commits、main branch 名
- **环境信息:** OS、shell、working dir、model name/ID、当前日期
- **CLAUDE.md:** 从 `~/.claude/CLAUDE.md` + 项目目录向上扫描 + `.claude/CLAUDE.md`
- **Memory 文件:** 从 `.claude/memory/` 目录加载
- **Skill 列表:** 动态发现，budget 为上下文窗口的 1%

### 1.4 Prompt 变体

**文件:** `src/utils/systemPrompt.ts` — `buildEffectiveSystemPrompt()` 优先级:

1. Override system prompt (loop mode，完全替换)
2. Coordinator system prompt (coordinator 模式)
3. Agent system prompt (proactive 模式追加，否则替换)
4. Custom system prompt (`--system-prompt` 参数)
5. Default system prompt
6. `appendSystemPrompt` (始终追加)

---

## 二、Tool Prompt 清单

每个 tool 在 `src/tools/{ToolName}/prompt.ts` 中定义 `prompt()` 方法，返回完整的工具使用指令。

### 核心工具 (Phase 1 需要)

| Tool | 文件 | 关键指令 |
|------|------|----------|
| **BashTool** | `src/tools/BashTool/prompt.ts` | 命令执行规则、sandbox 说明、git commit/PR 完整工作流、HEREDOC 格式 |
| **FileReadTool** | `src/tools/FileReadTool/prompt.ts` | 绝对路径、2000行默认、行号格式、图片/PDF 支持 |
| **FileEditTool** | `src/tools/FileEditTool/prompt.ts` | 先读后改、保持缩进、old_string 唯一性、replace_all、50行分块 |
| **FileWriteTool** | `src/tools/FileWriteTool/prompt.ts` | 覆盖警告、先读、优先用 Edit、150行分块 |
| **GlobTool** | `src/tools/GlobTool/prompt.ts` | glob 模式语法、mtime 排序 |
| **GrepTool** | `src/tools/GrepTool/prompt.ts` | ripgrep 语法、output_mode、multiline |

### 其他工具 (共 36+)

AgentTool, AskUserQuestionTool, BriefTool, ConfigTool, EnterPlanModeTool, EnterWorktreeTool, ExitPlanModeTool, ExitWorktreeTool, LSPTool, ListMcpResourcesTool, MCPTool, NotebookEditTool, PowerShellTool, ReadMcpResourceTool, RemoteTriggerTool, ScheduleCronTool (Create/Delete/List), SendMessageTool, SkillTool, SleepTool, TaskCreateTool, TaskGetTool, TaskListTool, TaskStopTool, TaskUpdateTool, TeamCreateTool, TeamDeleteTool, TodoWriteTool, ToolSearchTool, WebFetchTool, WebSearchTool

---

## 三、Message Harness (消息管线)

### 3.1 完整 Query Pipeline

**文件:** `src/query.ts` (~1729 行)

```
1. getAttachmentMessages()        → 预取 memory/skill/nag 附件
2. prependUserContext()            → 注入 <system-reminder> 包裹的 CLAUDE.md + 日期
3. startRelevantMemoryPrefetch()   → 后台预取相关 memory
4. autoCompactIfNeeded()           → 超 token 阈值则压缩
5. normalizeMessagesForAPI()       → 合并/过滤消息
6. ensureToolResultPairing()       → 修复孤立的 tool_use/tool_result
7. stripAdvisorBlocks()            → 移除 advisor 内容
8. buildSystemPromptBlocks()       → 构建带 cache_control 的 system prompt
9. API 调用 (streaming)
10. normalizeContentFromAPI()      → 过滤空 block
11. applyToolResultBudget()        → 截断/持久化大结果
12. microcompactMessages()         → 时间维度清理旧 tool result
13. runTools()                     → 执行 tool 调用
14. executePostSamplingHooks()     → 后置 hook
```

### 3.2 消息规范化

**文件:** `src/utils/messages.ts` (~5500 行)

- `normalizeMessagesForAPI()` — 合并连续同角色消息、过滤空/synthetic 消息、规范化 tool input
- `ensureToolResultPairing()` — 严格配对 tool_use ↔ tool_result，补充缺失的 placeholder
- `normalizeContentFromAPI()` — 清理 API 返回的空 text block
- `normalizeToolInput()` — 工具特定的 input 变换 (ExitPlanMode 注入 plan 内容、Bash 路径规范化等)

### 3.3 Tool Result 处理

**文件:** `src/utils/toolResultStorage.ts`

- 阈值: `getPersistenceThreshold(toolName)` — 通过 GrowthBook flag 可按工具覆盖
- 超阈值 → 写入 `{sessionDir}/tool-results/{uuid}.txt`
- 替换为 `<persisted-output>` 包裹: 文件路径 + 前 2KB 预览
- 清理标记: `[Old tool result content cleared]`
- 预算: 每消息聚合上限，保留最近 3 个完整结果

### 3.4 `<system-reminder>` 注入

**文件:** `src/utils/api.ts` — `prependUserContext()`

格式:
```xml
<system-reminder>
As you answer the user's questions, you can use the following context:
# currentDate
Today's date is 2025-05-15.
# CLAUDE.md
...
IMPORTANT: this context may or may not be relevant to your tasks...
</system-reminder>
```

用途: CLAUDE.md 注入、hook 输出、skill 列表、nag 提醒

### 3.5 Nag 提醒系统

**文件:** `src/utils/attachments.ts`

- `todo_reminder` — TodoList 存在时注入，包含待办项列表
- `task_reminder` — Task 列表存在时注入，包含任务状态
- 均以 `<system-reminder>` 包裹，末尾附 "Make sure that you NEVER mention this reminder to the user"

---

## 四、Compaction (上下文压缩)

### 4.1 三种压缩模式

**文件:** `src/services/compact/`

| 模式 | 文件 | 触发条件 |
|------|------|----------|
| **Auto Compact** | `autoCompact.ts` | token 数超过 `getAutoCompactThreshold(model)` |
| **Micro Compact** | `microCompact.ts` | 时间维度清理旧 tool result (不做全量压缩) |
| **Reactive Compact** | `compact.ts` | API 返回 413 prompt-too-long 时的恢复策略 |

### 4.2 压缩 Prompt 模板

**文件:** `src/services/compact/prompt.ts`

三个变体: `BASE_COMPACT_PROMPT`, `PARTIAL_COMPACT_PROMPT`, `PARTIAL_COMPACT_UP_TO_PROMPT`

输出 9 个 section:
1. Primary Request and Intent
2. Key Technical Concepts
3. Files and Code Sections
4. Errors and fixes
5. Problem Solving
6. All user messages
7. Pending Tasks
8. Current Work
9. Optional Next Step

包裹: `NO_TOOLS_PREAMBLE` ("CRITICAL: Respond with TEXT ONLY...") + `NO_TOOLS_TRAILER`

### 4.3 压缩后恢复

- `createPostCompactFileAttachments()` — 重新附加相关文件 (最多 5 个)
- `createSkillAttachmentIfNeeded()` — 重新注入 skill (budget: 25K tokens)
- `createPlanAttachmentIfNeeded()` — 重新注入 plan mode 上下文

---

## 五、Attachment 系统

**文件:** `src/utils/attachments.ts` (~3000 行)

40+ 种附件类型，关键的:

| 类型 | 用途 |
|------|------|
| `todo_reminder` | 待办提醒 |
| `task_reminder` | 任务提醒 |
| `skill_listing` | 可用 skill 列表 |
| `skill_discovery` | 自动发现的相关 skill |
| `relevant_memories` | 匹配的 memory 文件 |
| `nested_memory` | 子目录 memory |
| `memory_saved` | memory 保存确认 |
| `plan_mode` | Plan mode 提醒 |
| `agent_listing_delta` | 动态 agent 列表 |
| `mcp_instructions_delta` | MCP 指令更新 |
| `deferred_tools_delta` | 延迟加载的工具列表 |

Budget: `MAX_SESSION_BYTES = 60KB`

---

## 六、Hook 系统

**文件:** `src/utils/hooks/hookEvents.ts`, `src/types/hooks.ts`

### Hook 事件类型 (16 种):

SessionStart, Setup, SubagentStart, UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest, PermissionDenied, Notification, Elicitation, ElicitationResult, CwdChanged, FileChanged, WorktreeCreate, WorktreeRemove

### 事件流:

- `registerHookEventHandler(handler)` — 注册监听
- `emitHookStarted/Progress/Response()` — 发射事件
- `startHookProgressInterval()` — 每 1000ms 轮询 hook 输出
- Always-emitted: `SessionStart`, `Setup`

---

## 七、Memory 系统

### 7.1 Auto Memory 提取

**文件:** `src/services/extractMemories/prompts.ts`

四类 memory: Feedback & Preferences, Discoveries, Implementation Details, Session Context

### 7.2 Session Memory

**文件:** `src/services/SessionMemory/prompts.ts`

9 section 模板: Session Title, Current State, Task Specification, Files and Functions, Workflow, Errors & Corrections, Codebase Documentation, Learnings, Key Results

### 7.3 Memory 整合 (Auto-Dream)

**文件:** `src/services/autoDream/consolidationPrompt.ts`

四阶段: Orient → Gather → Consolidate → Prune and Index

---

## 八、错误恢复

**文件:** `src/services/api/errors.ts`

- **Prompt-too-long (413):** `getPromptTooLongTokenGap()` 解析 token 差值 → 先尝试 context-collapse drain → 再 reactive-compact → 都失败则报错
- **Max output tokens:** `getMaxOutputTokensForModel()` 动态调整
- **Tool result budget:** 超限自动持久化到磁盘

---

## 九、其他 Prompt 来源

| 文件 | 内容 |
|------|------|
| `src/utils/cyberRiskInstruction.ts` | 安全风险指令常量 |
| `src/coordinator/coordinatorMode.ts` | Coordinator 模式 system prompt |
| `src/utils/swarm/teammatePromptAddendum.ts` | Teammate 模式追加指令 |
| `src/buddy/prompt.ts` | Companion UI system prompt |
| `src/tools/AgentTool/built-in/verificationAgent.ts` | 验证 agent system prompt |
| `src/tools/AgentTool/built-in/statuslineSetup.ts` | Status line agent system prompt |
| `src/services/MagicDocs/prompts.ts` | Magic Docs 更新指令 |
| `src/memdir/findRelevantMemories.ts` | Memory 选择 agent prompt |
| `src/constants/outputStyles.ts` | 输出风格模板 |
| `src/constants/betas.ts` | Beta feature headers |
