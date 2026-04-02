# Hank CLI — TODO

基于 `openspec/changes/rust-core-plugin-architecture/tasks.md` 的 76 个任务，按实施顺序排列。

---

## Phase 1: 基础架构 (必须先完成)

### 1. Workspace 脚手架
- [ ] Cargo workspace + 4 crate 骨架 (hank-core, hank-tools, hank-mcp, hank-tui)
- [ ] workspace dependencies 统一管理
- [ ] 各 crate 空 lib.rs，确保 `cargo build` 通过

### 2. Streaming API Client (hank-core/api)
- [ ] `StreamEvent` enum (MessageStart, ContentBlockStart/Delta/Stop, MessageDelta/Stop, Ping, Error)
- [ ] `ContentBlock` / `Delta` / `Usage` / `StopReason` 类型
- [ ] `ApiClient::stream()` — reqwest + eventsource-stream SSE
- [ ] `StreamAccumulator` — HashMap 累积 partial JSON，content_block_stop 时解析
- [ ] 重试逻辑: 指数退避，仅 429/5xx

### 3. Tool System (hank-core/tool)
- [ ] `Tool` trait (async_trait): name, description, input_schema, call, prompt, format_result, is_concurrency_safe, is_read_only, check_permissions
- [ ] `ToolResult`, `ToolError`, `ToolContext` 类型
- [ ] `ToolRegistry`: register, get, api_definitions, merge, filtered
- [ ] `ToolExecutor`: 并发安全检查 → parallel/sequential dispatch

### 4. Query Engine (hank-core/engine)
- [ ] `QueryEvent` enum: TextDelta, ThinkingDelta, ToolStart, ToolComplete, PermissionRequest(oneshot), SpinnerMode, TurnComplete, Error
- [ ] `QueryEngine::submit(input, tx)` — spawn tokio task 运行 query loop
- [ ] Query loop: API stream → forward events → accumulate tool calls → execute → loop
- [ ] 会话历史管理 + JSON 持久化

### 5. Permission System (hank-core/permission)
- [ ] `PermissionMode` (Default/AcceptEdits/Bypass)
- [ ] `PermissionRule` 通配符匹配 (e.g. `Bash(npm:*)`)
- [ ] `PermissionChecker`: deny → tool.check_permissions → allow → mode → Ask
- [ ] 会话内 "Always Allow" 规则累积

---

## Phase 2: Prompt & Message Harness

### 6. Prompt Harness (hank-core/context)
- [ ] `prompts.rs` — 6 大 base section 作为 const str
- [ ] Git commit/PR workflow prompt
- [ ] 环境信息模板 + `render_environment()`
- [ ] 每个 Tool 的 `prompt()` 方法 (完整指令文本)
- [ ] CLAUDE.md 发现: `~/.claude/CLAUDE.md` + 项目目录向上扫描
- [ ] Git 上下文收集: branch, status(2KB), 5 commits
- [ ] `build_system_prompt()` 组装全部内容
- [ ] User context 注入: `<system-reminder>` 包裹 CLAUDE.md + 日期

### 7. Message Harness (hank-core/engine/messages)
- [ ] `MessageNormalizer::normalize()` — 过滤/合并/配对
- [ ] Tool result 格式化 + error 标记
- [ ] 大结果持久化: `<persisted-output>` + 磁盘存储
- [ ] Tool result budget 强制执行
- [ ] `<system-reminder>` 包裹工具
- [ ] Nag 提醒注入 (todo_reminder, task_reminder)
- [ ] 清理占位符: `[Old tool result content cleared]`

---

## Phase 3: 内置工具

### 8. Built-in Tools (hank-tools)
- [ ] `BashTool` — tokio::process::Command, 50K 截断, 120s 超时, 危险命令拒绝
- [ ] `FileReadTool` — 行号显示, offset/limit, 2000 行默认
- [ ] `FileWriteTool` — 创建父目录, 路径安全校验
- [ ] `FileEditTool` — old_string 唯一性验证, replace_all, diff 返回
- [ ] `GlobTool` — glob crate, mtime 排序, 1000 文件上限
- [ ] `GrepTool` — regex 搜索, 文件过滤, context lines, 1000 匹配上限
- [ ] `register_all(registry, workdir)`

---

## Phase 4: Terminal UI

### 9. Terminal UI (hank-tui)
- [ ] `EventHandler` — crossterm EventStream + tick + mpsc, tokio::select!
- [ ] `App` 状态: messages, scroll, input(TextArea), spinner, permission_dialog
- [ ] 三区布局: 消息区(Min) + 输入区(3行) + 状态栏(1行)
- [ ] 可滚动消息显示 + 流式自动滚动
- [ ] tui-textarea 输入 (Enter 提交, Ctrl+C 取消)
- [ ] 权限弹窗: centered_rect + Clear, [Y]/[N]/[A]
- [ ] Spinner 状态栏: throbber + SpinnerMode 标签
- [ ] `App::run()` 主循环

---

## Phase 5: 集成

### 10. Integration & Entry Point
- [ ] `main.rs` — CLI 参数解析, 加载设置, 构建 registry, 启动 TUI
- [ ] `settings.json` 加载 (权限规则, MCP 配置)
- [ ] QueryEngine ↔ TUI 事件桥接
- [ ] `/compact` 命令
- [ ] `/help` 命令
- [ ] 端到端测试

---

## 后续扩展 (Phase 1 之后)

- [ ] MCP Client (hank-mcp): JSON-RPC 2.0 over stdio, McpTool 适配器, `mcp__{server}__{tool}` 命名
- [ ] Context Compaction: auto/micro/reactive 三模式压缩
- [ ] Agent/Subagent 系统
- [ ] Task/Todo 系统
- [ ] Skill 加载系统
- [ ] Hook 系统
- [ ] Memory 系统 (auto memory, session memory, auto-dream)
- [ ] LSP 集成
- [ ] Notebook 编辑
- [ ] Web Fetch/Search
