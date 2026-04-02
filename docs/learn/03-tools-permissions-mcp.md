# Tools / Permissions / MCP

这部分回答三个问题：

1. Claude Code 看到的 tool list 是怎么来的？
2. 一个 tool_use 是怎么被执行和授权的？
3. MCP server 为什么可以在运行中“长出新工具”？

## 1. Tool 抽象在哪

核心类型在：

- `claude-code-source-code/src/Tool.ts`

读这个文件时重点看 3 类东西：

1. `ToolPermissionContext`
2. `ToolUseContext`
3. tool 的输入 schema、描述、权限、执行上下文

其中 `ToolUseContext` 很关键，因为它不是纯执行参数，它还携带：

- 当前 model / commands / tools
- app state getter/setter
- abort controller
- read file cache
- message 历史
- progress / notification / hook / attribution 更新接口

也就是说：tool 执行不是“纯函数”，而是会读写整个 agent runtime。

## 2. Tool pool 如何组装

### 2.1 built-in tools

`getAllBaseTools()` 定义了内建工具全集：

- Bash / Read / Edit / Write
- Agent / Skill / Todo / WebFetch / WebSearch
- 以及一堆 feature-gated 工具

对应代码：

- `claude-code-source-code/src/tools.ts`

### 2.2 最终可见工具

真正用于当前会话的工具集不是 `getAllBaseTools()`，而是：

- `getTools(permissionContext)`
- `assembleToolPool(permissionContext, mcpTools)`

`assembleToolPool()` 做三件事：

1. 取 built-in tools
2. 用 deny rule 过滤 MCP tools
3. built-in + MCP 合并、按名字排序、按名字去重

对应代码：

- `claude-code-source-code/src/tools.ts:345`

这个函数很关键，因为它是 REPL 和 subagent 都共享的“工具池单一真相来源”。

### 2.3 运行中刷新工具

REPL 会把 `refreshTools()` 放进 `ToolUseContext.options`：

- `claude-code-source-code/src/screens/REPL.tsx:2396`

`query.ts` 在每轮结束前会调用它；这意味着：

- 某个 MCP server 本轮中途连上
- 下一轮模型就能看到它的新工具

## 3. Tool 执行的两种路径

### 3.1 常规批执行

`runTools()`：

- `claude-code-source-code/src/services/tools/toolOrchestration.ts:19`

它会先把 tool calls 分成两类 batch：

- 并发安全 batch
- 非并发安全 batch

然后：

- 并发安全的并行跑
- 非并发安全的串行跑

### 3.2 流式执行

`StreamingToolExecutor`：

- `claude-code-source-code/src/services/tools/StreamingToolExecutor.ts:40`

它允许模型流里刚吐出 `tool_use` block，就立刻开始执行，而不是等整个 assistant response 结束。

这是 Claude Code 比很多“玩具 agent loop”更工程化的地方。

## 4. 权限判定的真实优先级

真正的权限内核在：

- `claude-code-source-code/src/utils/permissions/permissions.ts:1158`

### 4.1 内层判定 `hasPermissionsToUseToolInner()`

优先级基本是：

1. 整个 tool 被 deny rule 拒绝
2. 整个 tool 被 ask rule 要求确认
3. 调 tool 自己的 `checkPermissions()`
4. 工具内部 deny
5. bypass-immune ask
   - `requiresUserInteraction`
   - content-specific ask rule
   - safetyCheck
6. bypassPermissions / plan+bypass 直接 allow
7. blanket allow rule 直接 allow
8. 剩余 `passthrough` -> 转成 `ask`

这意味着：不是所有“bypassPermissions”都真能绕过，某些 safety check 和 ask rule 仍然会强制拦住。

### 4.2 外层包装 `hasPermissionsToUseTool()`

外层再处理：

- `dontAsk` 模式：把 `ask` 变成 `deny`
- `auto` / `plan+auto`：走 classifier
- denial tracking
- auto-mode safe allowlist / acceptEdits fast-path

对应代码：

- `claude-code-source-code/src/utils/permissions/permissions.ts:473`

## 5. 交互式权限弹窗在哪接进来

真正把“permission decision”连接到 UI 的是：

- `claude-code-source-code/src/hooks/useCanUseTool.tsx:28`

这个 hook 会：

1. 先调用 `hasPermissionsToUseTool(...)`
2. 如果直接 allow，就返回
3. 如果 deny，就记录并返回
4. 如果 ask，就根据场景决定：
   - coordinator handler
   - swarm worker handler
   - interactive permission dialog

也就是说：

- `permissions.ts` 更像策略引擎
- `useCanUseTool.tsx` 更像 UI/runtime 桥接层

## 6. Hook 也能参与 tool 执行

关键文件：

- `claude-code-source-code/src/services/tools/toolHooks.ts`

这里至少有 3 类 hook 入口：

- PreToolUse
- PostToolUse
- PostToolUseFailure

hook 可以做的事情不止“记录日志”，还包括：

- block continuation
- 返回 additional context
- 改写 MCP tool output
- 产出 attachment message

所以 tool execution 不是：

```text
tool_use -> tool.call -> tool_result
```

而更像：

```text
tool_use
  -> permission engine
  -> pre hooks
  -> tool.call
  -> post hooks / failure hooks
  -> tool_result / attachment / progress
```

## 7. MCP server 如何变成工具

核心文件：

- `claude-code-source-code/src/services/mcp/client.ts:2226`
- `claude-code-source-code/src/services/mcp/client.ts:2408`

### 7.1 连接流程

`getMcpToolsCommandsAndResources()` 会：

1. 读取 MCP config
2. 按 server 类型分成本地和远程两批
3. 并发连接 server
4. 拉取 tools / commands / resources
5. 把连接状态回调给调用者

MCP server 可能处于：

- `pending`
- `connected`
- `needs-auth`
- `disabled`

### 7.2 启动期预取

`prefetchAllMcpResources()` 会把所有 server 的：

- `clients`
- `tools`
- `commands`

聚合出来，给启动阶段或 print/headless 阶段使用。

### 7.3 为什么 tool list 会动态变化

因为 MCP 连接是异步的：

- 程序先起来
- MCP server 再逐个连上
- `appState.mcp.tools` 会不断增量更新
- `refreshTools()` 每轮重新组装工具池

这就是 Claude Code 为什么能“晚连接也能生效”。

## 8. 你真正该记住的模型

把 tools / permissions / MCP 抽象成下面这张图最有用：

```text
all base tools
  + current MCP tools
  -> assembleToolPool
  -> model 看到的 tool schemas
  -> assistant 产生 tool_use
  -> hasPermissionsToUseTool / useCanUseTool
  -> runTools / StreamingToolExecutor
  -> tool_result + attachments
  -> 下一轮继续
```

如果这条链理解了，后面看具体某个 Tool 实现就不会迷路。

