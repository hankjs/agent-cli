## ADDED Requirements

### Requirement: /compact REPL 命令
REPL SHALL 支持 `/compact` 命令，直接触发 auto_compact 压缩当前对话历史，不经过 LLM。

#### Scenario: 执行 /compact
- **WHEN** 用户输入 "/compact" 且 history 非空
- **THEN** 系统执行 auto_compact，打印 "[manual compact via /compact]"，不发送给 LLM

### Requirement: /tasks REPL 命令
REPL SHALL 支持 `/tasks` 命令，直接打印 TaskManager.list_all() 的结果。

#### Scenario: 执行 /tasks
- **WHEN** 用户输入 "/tasks"
- **THEN** 系统打印所有 task 的摘要列表，不发送给 LLM

### Requirement: /team REPL 命令
REPL SHALL 支持 `/team` 命令，直接打印 TeammateManager.list_all() 的结果。

#### Scenario: 执行 /team
- **WHEN** 用户输入 "/team"
- **THEN** 系统打印 team 信息和所有 member 状态，不发送给 LLM

### Requirement: /inbox REPL 命令
REPL SHALL 支持 `/inbox` 命令，直接读取并打印 lead 的 inbox 内容。

#### Scenario: 执行 /inbox
- **WHEN** 用户输入 "/inbox"
- **THEN** 系统读取 lead inbox，打印 JSON 格式的消息列表，不发送给 LLM

### Requirement: REPL prompt 和 system prompt 更新
REPL prompt SHALL 显示 "s_full >>"。System prompt SHALL 引导使用 task_create/task_update/task_list 做多步工作，TodoWrite 做短清单，task 做子代理委派，load_skill 做专业知识加载。

#### Scenario: 启动时 prompt
- **WHEN** CLI 启动
- **THEN** 输入提示显示 "s_full >>"，system prompt 包含 skill 列表和工具使用指引
