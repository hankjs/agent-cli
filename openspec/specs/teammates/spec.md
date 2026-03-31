## ADDED Requirements

### Requirement: TeammateManager 状态管理
系统 SHALL 提供 TeammateManager，管理 teammate 的生命周期。Team 配置持久化在 `.team/config.json`，包含 team_name 和 members 数组。每个 member 有 name、role、status（working/idle/shutdown）字段。

#### Scenario: 配置文件自动创建
- **WHEN** TeammateManager 初始化时 `.team/config.json` 不存在
- **THEN** 系统使用默认配置 `{"team_name": "default", "members": []}`

#### Scenario: 配置文件已存在
- **WHEN** TeammateManager 初始化时 `.team/config.json` 已存在
- **THEN** 系统加载已有配置

### Requirement: spawn_teammate 工具
系统 SHALL 提供 `spawn_teammate` 工具，接受 `name`（必填）、`role`（必填）、`prompt`（必填）参数。Spawn 一个在独立 tokio task 中运行的自治 agent。

#### Scenario: Spawn 新 teammate
- **WHEN** 调用 spawn_teammate，name="worker1"，role="tester"，prompt="运行所有测试"
- **THEN** 系统创建新 member 记录（status=working），启动独立 agent 循环，返回 "Spawned 'worker1' (role: tester)"

#### Scenario: 重新 spawn 已 idle/shutdown 的 teammate
- **WHEN** 调用 spawn_teammate，name="worker1"（已存在且 status 为 idle 或 shutdown）
- **THEN** 系统更新 status 为 working，启动新的 agent 循环

#### Scenario: Spawn 正在工作的 teammate
- **WHEN** 调用 spawn_teammate，name="worker1"（status 为 working）
- **THEN** 系统返回错误 "Error: 'worker1' is currently working"

### Requirement: Teammate agent 循环
Teammate agent SHALL 运行独立的 tool loop，拥有 bash、read_file、write_file、edit_file、send_message、idle、claim_task 工具。Teammate 使用专用 system prompt 包含其 name、role、team_name。

#### Scenario: Teammate 工作阶段
- **WHEN** teammate 被 spawn
- **THEN** teammate 进入工作阶段，最多执行 50 轮 tool loop

#### Scenario: Teammate 检查 inbox
- **WHEN** teammate 在工作阶段每轮开始时
- **THEN** 系统检查该 teammate 的 inbox，如有 shutdown_request 则立即退出

### Requirement: Teammate idle 阶段
Teammate 调用 `idle` 工具后 SHALL 进入 idle 阶段。Idle 阶段每 POLL_INTERVAL（5 秒）检查一次 inbox 和未认领 task。IDLE_TIMEOUT（60 秒）后无新工作则自动 shutdown。

#### Scenario: Idle 阶段收到消息
- **WHEN** teammate 在 idle 阶段，inbox 收到新消息
- **THEN** teammate 恢复为 working 状态，将消息注入 messages 继续工作

#### Scenario: Idle 阶段 auto-claim task
- **WHEN** teammate 在 idle 阶段，存在 status=pending、无 owner、无 blockedBy 的 task
- **THEN** teammate 自动 claim 该 task（设 owner 和 status=in_progress），注入 `<auto-claimed>` 消息，恢复工作

#### Scenario: Idle 超时
- **WHEN** teammate 在 idle 阶段超过 IDLE_TIMEOUT 无新工作
- **THEN** teammate 自动 shutdown

### Requirement: Teammate identity re-injection
当 teammate 的 messages 列表很短（≤3 条，可能因压缩）时，auto-claim SHALL 在 messages 开头注入 identity 消息。

#### Scenario: 压缩后 identity 恢复
- **WHEN** teammate messages ≤ 3 条且 auto-claim 触发
- **THEN** 系统在 messages 开头插入 `<identity>` user message 和 assistant ack

### Requirement: list_teammates 工具
系统 SHALL 提供 `list_teammates` 工具（无参数），返回 team 名称和所有 member 的 name、role、status。

#### Scenario: 列出 teammates
- **WHEN** 调用 list_teammates，team 有 2 个 member
- **THEN** 系统返回 "Team: {name}" 和每个 member 的 "{name} ({role}): {status}"
