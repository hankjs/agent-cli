## ADDED Requirements

### Requirement: MessageBus 文件消息系统
系统 SHALL 提供 MessageBus，基于 `.team/inbox/` 目录的 JSONL 文件实现消息传递。每个 agent 有独立的 inbox 文件 `{name}.jsonl`。

#### Scenario: 目录自动创建
- **WHEN** MessageBus 初始化时 `.team/inbox/` 不存在
- **THEN** 系统自动创建 `.team/inbox/` 目录（含父目录）

### Requirement: send_message 工具
系统 SHALL 提供 `send_message` 工具，接受 `to`（必填）、`content`（必填）、`msg_type`（选填，默认 "message"）参数。消息 JSON 包含 type、from、content、timestamp 字段。

#### Scenario: 发送消息
- **WHEN** lead 调用 send_message，to="worker1"，content="请检查测试"
- **THEN** 系统向 `.team/inbox/worker1.jsonl` 追加一行 JSON，返回 "Sent message to worker1"

### Requirement: read_inbox 工具
系统 SHALL 提供 `read_inbox` 工具（无参数），读取并清空 lead 的 inbox，返回消息 JSON 数组。

#### Scenario: 读取并清空 inbox
- **WHEN** lead 调用 read_inbox，inbox 中有 3 条消息
- **THEN** 系统返回 3 条消息的 JSON 数组，并清空 inbox 文件

#### Scenario: 空 inbox
- **WHEN** lead 调用 read_inbox，inbox 为空或不存在
- **THEN** 系统返回空数组 "[]"

### Requirement: broadcast 工具
系统 SHALL 提供 `broadcast` 工具，接受 `content`（必填）参数，向所有 teammate（排除发送者自身）发送 broadcast 类型消息。

#### Scenario: 广播消息
- **WHEN** lead 调用 broadcast，content="全体暂停"，team 有 worker1 和 worker2
- **THEN** 系统向 worker1 和 worker2 的 inbox 各追加一条 broadcast 消息，返回 "Broadcast to 2 teammates"

### Requirement: agent_loop inbox 检查
agent_loop SHALL 在每轮 LLM 调用前检查 lead 的 inbox。如有消息，SHALL 以 `<inbox>` 标签注入到 messages 中。

#### Scenario: Lead inbox 有消息
- **WHEN** agent_loop 开始新一轮，lead inbox 有 1 条消息
- **THEN** 系统将消息格式化为 `<inbox>` JSON，追加 user message 和 assistant ack

### Requirement: 合法消息类型
系统 SHALL 支持以下消息类型：message、broadcast、shutdown_request、shutdown_response、plan_approval_response。

#### Scenario: 消息类型验证
- **WHEN** send_message 指定 msg_type
- **THEN** 系统使用指定的 msg_type 写入消息 JSON
