# Prompt Harness

这里的 “Prompt harness” 指的不是单个 prompt 字符串，而是 Claude Code 在一次模型请求前，把哪些内容按什么优先级、什么缓存策略、什么 API 结构拼进去。

## 1. 先看 5 个关键函数

- 默认 system prompt：`claude-code-source-code/src/constants/prompts.ts:444`
- effective system prompt 选择：`claude-code-source-code/src/utils/systemPrompt.ts:41`
- 取三件套（default prompt / user context / system context）：`claude-code-source-code/src/utils/queryContext.ts:44`
- tool schema 转 API schema：`claude-code-source-code/src/utils/api.ts:119`
- system prompt block -> API text block：`claude-code-source-code/src/services/api/claude.ts:3213`

## 2. System Prompt 的 3 层结构

### 2.1 默认 prompt

`getSystemPrompt()` 产出的是 `string[]`，不是单个字符串。

静态部分来自这些 section：

- `getSimpleIntroSection()`
- `getSimpleSystemSection()`
- `getSimpleDoingTasksSection()`
- `getActionsSection()`
- `getUsingYourToolsSection()`
- `getSimpleToneAndStyleSection()`
- `getOutputEfficiencySection()`

动态部分通过 section registry 加进去：

- `session_guidance`
- `memory`
- `ant_model_override`
- `env_info_simple`
- `language`
- `output_style`
- `mcp_instructions`
- `scratchpad`
- `frc`（function result clearing）
- `summarize_tool_results`
- 某些 feature gate 相关 section

对应代码：

- `claude-code-source-code/src/constants/prompts.ts:444`
- `claude-code-source-code/src/constants/systemPromptSections.ts:20`

### 2.2 effective prompt 的优先级

`buildEffectiveSystemPrompt()` 的优先级很关键：

1. `overrideSystemPrompt`
2. coordinator prompt
3. agent prompt
4. custom system prompt (`--system-prompt`)
5. default system prompt
6. `appendSystemPrompt` 在除 override 模式外的所有情况下追加到最后（override 直接返回，跳过 append）

注意：agent prompt 不一定替换 default prompt。在 proactive 模式里，它会 append 到 default prompt 后面。

对应代码：

- `claude-code-source-code/src/utils/systemPrompt.ts:41`

### 2.3 CLI 还能注入 custom / append prompt

`main.tsx` 会处理：

- `--system-prompt`
- `--system-prompt-file`
- `--append-system-prompt`
- `--append-system-prompt-file`

对应代码：

- `claude-code-source-code/src/main.tsx:1342`

## 3. Prompt 不是只有 system prompt

一次 query 里，真正送到模型前的“提示上下文”至少包含 4 块：

1. `systemPrompt`
2. `userContext`
3. `systemContext`
4. `tools`（tool schemas）

`fetchSystemPromptParts()` 专门拉前 3 块：

- `defaultSystemPrompt`
- `userContext`
- `systemContext`

对应代码：

- `claude-code-source-code/src/utils/queryContext.ts:44`

## 4. `userContext` 和 `systemContext` 的角色不同

### 4.1 `userContext`

来源：

- `claude-code-source-code/src/context.ts:155`

主要内容：

- `CLAUDE.md` / memory 相关内容
- 当前日期

注入方式：

- `prependUserContext(messages, userContext)`
- 以一个隐藏的 `<system-reminder>` user message 形式插到消息数组最前面

对应代码：

- `claude-code-source-code/src/utils/api.ts:449`

### 4.2 `systemContext`

来源：

- `claude-code-source-code/src/context.ts:116`

主要内容：

- git status snapshot
- branch / main branch / recent commits
- 某些 cache breaker 注入

注入方式：

- `appendSystemContext(systemPrompt, systemContext)`
- 追加到 system prompt 数组尾部

对应代码：

- `claude-code-source-code/src/utils/api.ts:437`

## 5. Tool schema 也是 prompt 的一部分

`toolToAPISchema()` 负责把每个 Tool 变成 API 能看懂的 schema。

它做的不只是 schema 转换，还会处理：

- tool description（`tool.prompt()`）
- `strict`
- `eager_input_streaming`
- `defer_loading`
- `cache_control`
- swarm 字段裁剪
- tool schema 缓存

对应代码：

- `claude-code-source-code/src/utils/api.ts:119`

所以 Claude Code 的 harness 是：

```text
system prompt
+ user/system context
+ tool descriptions
+ tool input schemas
+ API betas / thinking / cache headers
```

## 6. Prompt cache 的关键设计

### 6.1 static / dynamic 边界

`SYSTEM_PROMPT_DYNAMIC_BOUNDARY` 把 system prompt 分成：

- 边界前：尽量全局可缓存
- 边界后：会话级动态内容

对应代码：

- `claude-code-source-code/src/constants/prompts.ts`
- `claude-code-source-code/src/utils/api.ts:321`

### 6.2 section registry 里的缓存

`systemPromptSection()` 是缓存型 section。

`DANGEROUS_uncachedSystemPromptSection()` 是每轮重算型 section。

这一层是在“prompt 内容生成阶段”控制 cache churn。

对应代码：

- `claude-code-source-code/src/constants/systemPromptSections.ts:20`

### 6.3 发请求前还要再切 block

`splitSysPromptPrefix()` 会把 system prompt 拆成若干 block，并给每个 block 一个 cache scope：

- `global`
- `org`
- `null`

然后 `buildSystemPromptBlocks()` 把这些 block 转成 API 的 `TextBlockParam[]`，必要时加上 `cache_control`。

对应代码：

- `claude-code-source-code/src/utils/api.ts:321`
- `claude-code-source-code/src/services/api/claude.ts:3213`

## 7. 最终 API 请求前还会加什么

`services/api/claude.ts` 在真正发请求前还会继续包装：

- attribution header
- CLI sysprompt prefix
- advisor / chrome tool-search instructions
- betas
- thinking config
- context management
- output format
- prompt caching headers

关键位置：

- `claude-code-source-code/src/services/api/claude.ts:1259`
- `claude-code-source-code/src/services/api/claude.ts:1371`

可以把这一层理解为“prompt/runtime transport harness”。

## 8. 为什么这套设计重要

如果把 Claude Code 误读成：

```text
一个 system prompt + 一个 message 数组 + 一个 tool list
```

会漏掉 4 个真正关键的工程点：

1. prompt 不是一段文本，而是多来源、可缓存、可增量变化的 block 集合
2. tool schema 本身是 prompt 的一部分，而且会影响 cache key
3. user context 和 system context 被放在不同通道里
4. 真实的“Claude Code 行为”很大程度上由 harness 决定，而不是只由 prompt 文案决定

## 9. 建议的源码阅读顺序

```text
getSystemPrompt
  -> systemPromptSection / resolveSystemPromptSections
  -> buildEffectiveSystemPrompt
  -> fetchSystemPromptParts
  -> prependUserContext / appendSystemContext
  -> toolToAPISchema
  -> splitSysPromptPrefix
  -> buildSystemPromptBlocks
  -> services/api/claude.ts 的 queryModelWithStreaming
```

