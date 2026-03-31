## ADDED Requirements

### Requirement: BackgroundManager 后台执行
系统 SHALL 提供 BackgroundManager，在独立线程（tokio::spawn）中执行 shell 命令。每个后台任务有唯一 ID（uuid 前 8 位）、status（running/completed/error）和 result。

#### Scenario: 启动后台任务
- **WHEN** 调用 background_run，command="cargo test"
- **THEN** 系统返回 "Background task {id} started: cargo test"，命令在后台线程执行

### Requirement: background_run 工具
系统 SHALL 提供 `background_run` 工具，接受 `command`（必填）和 `timeout`（选填，默认 120 秒）参数。

#### Scenario: 后台命令完成
- **WHEN** 后台命令执行完毕
- **THEN** 系统将任务 status 设为 "completed"，result 存储 stdout+stderr（截断 50000 字符），并向通知队列推送通知

#### Scenario: 后台命令超时
- **WHEN** 后台命令超过 timeout 时间
- **THEN** 系统将任务 status 设为 "error"，result 为超时错误信息

### Requirement: check_background 工具
系统 SHALL 提供 `check_background` 工具，接受可选 `task_id` 参数。有 task_id 时返回该任务详情，无 task_id 时返回所有后台任务摘要。

#### Scenario: 检查特定任务
- **WHEN** 调用 check_background，task_id="abc12345"
- **THEN** 系统返回 "[{status}] {result}" 格式的任务详情

#### Scenario: 列出所有后台任务
- **WHEN** 调用 check_background，无 task_id
- **THEN** 系统返回所有后台任务的 "{id}: [{status}] {command}" 列表

### Requirement: 通知队列 drain
agent_loop SHALL 在每轮 LLM 调用前 drain BackgroundManager 的通知队列。如有完成的通知，SHALL 以 `<background-results>` 标签注入到 messages 中。

#### Scenario: Drain 后台通知
- **WHEN** agent_loop 开始新一轮，通知队列中有 2 条完成通知
- **THEN** 系统将通知格式化为 `<background-results>` 文本，追加 user message 和 assistant ack
