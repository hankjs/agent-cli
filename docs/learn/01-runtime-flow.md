# Claude Code 主流程

这一份只回答一个问题：一次用户输入，是怎样走到模型、工具、再回到下一轮的。

## 1. 入口分两层

### 1.1 轻量 CLI 分流

`claude-code-source-code/src/entrypoints/cli.tsx:33`

这层做的是“快速分流”：

- `--version`
- `--dump-system-prompt`
- remote control / daemon / bg session
- worktree + tmux 之类 fast-path

设计重点是：很多路径不需要加载完整 CLI，就尽量不加载。

### 1.2 重量级运行时初始化

`claude-code-source-code/src/main.tsx:585`

这层才是真正启动主程序，负责：

- 初始化配置、warning handler、信任/权限模式
- 解析 CLI 选项
- 处理 `--system-prompt` / `--append-system-prompt`
- 建立 state/store
- 连接 MCP server
- 启动 REPL 或 headless/print/SDK 路径

## 2. 两条执行面

### 2.1 REPL 交互路径

关键文件：

- `claude-code-source-code/src/utils/handlePromptSubmit.ts`
- `claude-code-source-code/src/utils/processUserInput/processUserInput.ts`
- `claude-code-source-code/src/query.ts`

链路：

```text
PromptInput / handlePromptSubmit
  -> processUserInput(...)
  -> onQuery(...)
  -> query(...)
```

REPL 多出来的东西：

- Ink UI 状态
- 交互式权限弹窗
- spinner / progress / queued command
- `refreshTools()`，允许 MCP 晚连接后下一轮自动可见

### 2.2 SDK / headless 路径

关键文件：

- `claude-code-source-code/src/QueryEngine.ts:184`

这是更适合学习的路径，因为它把会话生命周期收敛进 `QueryEngine.submitMessage()`。

核心步骤都能在一个函数里看到：

1. 解析当前模型和 thinking 配置
2. `fetchSystemPromptParts()` 拉默认 prompt、user context、system context
3. 调 `processUserInput()`
4. 生成 SDK 的 `system/init` 消息
5. 调 `query()` 进入主循环
6. 持续消费流式消息并更新 `mutableMessages`

## 3. 输入分流

关键文件：

- `claude-code-source-code/src/utils/processUserInput/processUserInput.ts:85`
- `claude-code-source-code/src/utils/processUserInput/processTextPrompt.ts:19`
- `claude-code-source-code/src/utils/processUserInput/processSlashCommand.tsx:309`
- `claude-code-source-code/src/utils/processUserInput/processBashCommand.tsx:17`

`processUserInput()` 不是简单“把字符串包成 user message”，它先做：

- 粘贴内容展开
- 图片缩放/降采样
- 附件解析
- bridge-safe slash command 判断
- ultraplan 关键字重写

然后分流为 3 条主路径 + 1 条特殊路径：

1. bash 模式 -> `processBashCommand()`
2. slash command（`/` 开头）-> `processSlashCommand()`
3. 普通文本 -> `processTextPrompt()`
4. ultraplan 关键字 -> 重写为 `/ultraplan` slash command

附件不是独立的分流路径，而是在分流之前通过 `getAttachmentMessages()` 提取，作为横切关注点传入各路径。带图片/附件的内容仍然走普通 prompt，但 message content 更复杂。

一个要点：slash command 和 bash 命令并不一定走模型，它们很多是本地执行后直接返回 `shouldQuery: false`。

## 4. `query()` 是真正的 agent loop

关键文件：

- `claude-code-source-code/src/query.ts:219`（`query()` 入口，thin wrapper）
- `claude-code-source-code/src/query.ts:241`（`queryLoop()` 开始）
- `claude-code-source-code/src/query.ts:307`（真正的 `while (true)` 循环）

`query()` 本身是一个 thin wrapper，委托给 `queryLoop()`，后者包含真正的循环。最重要的骨架：

```text
query() -> queryLoop()
while (true):  // line 307
  1. 预取 memory / skill
  2. 处理 tool result budget / snip / microcompact / collapse / autocompact
  3. 组装 fullSystemPrompt
  4. prependUserContext(messages)
  5. 调模型流式接口
  6. 收集 assistant block / tool_use
  7. 执行工具，生成 tool_result
  8. 注入 attachments / queued commands / memory / skill discovery
  9. 如果还要继续，就把新 messages 递归回下一轮
```

## 5. 流式响应中的两个关键设计

### 5.1 流式工具执行

当模型流里边出现 `tool_use` 时，`query.ts` 会把 block 收集到 `toolUseBlocks`，同时在开启开关时使用：

- `claude-code-source-code/src/services/tools/StreamingToolExecutor.ts:40`

也就是说，不需要等整条 assistant message 结束才执行工具。

### 5.2 “继续下一轮”不是函数递归，而是状态递推

`query.ts` 虽然概念上是多轮递归，但实现上是一个 `while (true)` + `state = next`：

- 工具结果出来后，重新构造 `next: State`
- `messages = [...messagesForQuery, ...assistantMessages, ...toolResults]`
- 再进入下一次循环

这使得它能统一处理：

- tool follow-up
- compact 后重试
- fallback model
- stop hook
- max output tokens recovery

## 6. 为什么 `query.ts` 会这么复杂

因为它不是“最小 agent loop”，而是“生产级 harness”。

它额外处理了：

- prompt-too-long 恢复
- max-output-tokens 恢复
- 流式 fallback
- tool result pairing 修复
- compact 和 collapse
- queued command / background task 注入
- tool use summary
- MCP 工具热更新

如果只想先理解最小闭环，建议只盯住这几个点：

1. `messagesForQuery` 从哪里来
2. `fullSystemPrompt` 从哪里来
3. `deps.callModel(...)` 发了什么
4. `toolUseBlocks` 如何变成 `toolResults`
5. `next: State` 如何把本轮产物推进到下一轮

## 7. 学习时的最短源码跳转路径

```text
QueryEngine.submitMessage
  -> fetchSystemPromptParts
  -> processUserInput
  -> query
      -> prependUserContext / appendSystemContext
      -> deps.callModel (services/api/claude)
      -> StreamingToolExecutor 或 runTools
      -> next State
```

如果你只能花 30 分钟，先把上面这条线走通。

