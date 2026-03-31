## ADDED Requirements

### Requirement: shutdown_request 工具
系统 SHALL 提供 `shutdown_request` 工具，接受 `teammate`（必填）参数。生成唯一 request_id（uuid 前 8 位），通过 MessageBus 向目标 teammate 发送 shutdown_request 类型消息。

#### Scenario: 发送 shutdown 请求
- **WHEN** lead 调用 shutdown_request，teammate="worker1"
- **THEN** 系统生成 request_id，向 worker1 inbox 发送 shutdown_request 消息，返回 "Shutdown request {id} sent to 'worker1'"

#### Scenario: Teammate 收到 shutdown_request
- **WHEN** teammate 在工作或 idle 阶段检查 inbox 发现 shutdown_request
- **THEN** teammate 将自身 status 设为 "shutdown" 并退出循环

### Requirement: plan_approval 工具
系统 SHALL 提供 `plan_approval` 工具，接受 `request_id`（必填）、`approve`（必填，布尔）、`feedback`（选填）参数。通过 MessageBus 向请求者发送 plan_approval_response 消息。

#### Scenario: 批准 plan
- **WHEN** lead 调用 plan_approval，request_id="abc123"，approve=true
- **THEN** 系统将 plan request 状态设为 "approved"，向请求者发送 plan_approval_response

#### Scenario: 拒绝 plan
- **WHEN** lead 调用 plan_approval，request_id="abc123"，approve=false，feedback="需要更多测试"
- **THEN** 系统将 plan request 状态设为 "rejected"，向请求者发送包含 feedback 的 plan_approval_response

#### Scenario: 未知 request_id
- **WHEN** 调用 plan_approval，request_id 不存在
- **THEN** 系统返回 "Error: Unknown plan request_id '{id}'"

### Requirement: shutdown/plan 状态追踪
系统 SHALL 在内存中维护 shutdown_requests 和 plan_requests 两个 HashMap，用于追踪请求状态。

#### Scenario: 追踪 shutdown 请求
- **WHEN** 发送 shutdown_request
- **THEN** 系统在 shutdown_requests 中记录 {request_id: {target, status: "pending"}}
