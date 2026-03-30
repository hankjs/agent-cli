## ADDED Requirements

### Requirement: task 工具定义
系统 SHALL 提供名为 `task` 的工具，接受 `prompt`（必填）和 `description`（选填）参数，用于派发子代理任务。

#### Scenario: 父级调用 task 工具
- **WHEN** 父级 agent 调用 task 工具并传入 prompt
- **THEN** 系统创建一个子代理执行该 prompt，并将子代理的最终文本摘要作为 tool_result 返回给父级

### Requirement: 子代理上下文隔离
子代理 SHALL 使用全新的 messages 列表（空），不继承父级的对话历史。子代理完成后，其上下文 SHALL 被丢弃，只有最终文本摘要返回给父级。

#### Scenario: 子代理不继承父级历史
- **WHEN** 父级有 10 轮对话历史并派发子代理
- **THEN** 子代理的 messages 列表只包含初始 user message（prompt），不包含父级的任何历史消息

#### Scenario: 子代理上下文不回流
- **WHEN** 子代理执行了 5 轮工具调用后返回摘要
- **THEN** 父级只收到摘要文本，子代理的 5 轮工具调用细节不出现在父级 messages 中

### Requirement: 子代理工具集限制
子代理 SHALL 只获得 bash、read_file、write_file、edit_file 四个工具。子代理 SHALL NOT 获得 task 工具（防止递归派发）。子代理 SHALL NOT 获得 todo 工具。

#### Scenario: 子代理无法递归派发
- **WHEN** 子代理尝试调用 task 工具
- **THEN** 该工具不在子代理的可用工具列表中，API 不会返回 task 类型的 tool_use

### Requirement: 子代理执行上限
子代理 SHALL 最多执行 30 轮 tool loop。达到上限后 SHALL 停止并返回当前已有的文本内容。

#### Scenario: 子代理达到 30 轮上限
- **WHEN** 子代理连续 30 轮都产生 tool_use
- **THEN** 子代理停止循环，返回最后一次响应中的文本内容

### Requirement: 子代理 system prompt
子代理 SHALL 使用独立的 system prompt：`"You are a coding subagent at {cwd}. Complete the given task, then summarize your findings."`

#### Scenario: 子代理使用专用 system prompt
- **WHEN** 子代理被创建
- **THEN** API 调用使用子代理专用 system prompt，而非父级的 system prompt

### Requirement: 子代理串行执行
当父级在一轮响应中派发多个 task 工具调用时，系统 SHALL 串行执行这些子代理（一次一个）。

#### Scenario: 多个 task 串行执行
- **WHEN** 父级一次返回 2 个 task tool_use
- **THEN** 第一个子代理完成后才开始第二个子代理
