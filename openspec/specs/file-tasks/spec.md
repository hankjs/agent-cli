## ADDED Requirements

### Requirement: TaskManager 文件持久化
系统 SHALL 在 `.tasks/` 目录下以 `task_{id}.json` 格式持久化每个 task。每个 task JSON 包含字段：`id`（整数）、`subject`（字符串）、`description`（字符串）、`status`（字符串）、`owner`（字符串或 null）、`blockedBy`（整数数组）、`blocks`（整数数组）。

#### Scenario: 目录自动创建
- **WHEN** TaskManager 初始化时 `.tasks/` 目录不存在
- **THEN** 系统自动创建 `.tasks/` 目录

#### Scenario: ID 自增
- **WHEN** `.tasks/` 目录中已有 task_1.json 和 task_3.json
- **THEN** 下一个创建的 task ID 为 4（max existing + 1）

### Requirement: task_create 工具
系统 SHALL 提供 `task_create` 工具，接受 `subject`（必填）和 `description`（选填）参数，创建 status 为 "pending" 的新 task 并写入文件。

#### Scenario: 创建新 task
- **WHEN** 调用 task_create，subject="实现登录功能"
- **THEN** 系统创建 `.tasks/task_{id}.json`，返回 task JSON（status=pending, blockedBy=[], blocks=[]）

### Requirement: task_get 工具
系统 SHALL 提供 `task_get` 工具，接受 `task_id`（必填，整数）参数，返回 task 的完整 JSON。

#### Scenario: 获取存在的 task
- **WHEN** 调用 task_get，task_id=1，且 task_1.json 存在
- **THEN** 系统返回该 task 的完整 JSON

#### Scenario: 获取不存在的 task
- **WHEN** 调用 task_get，task_id=99，且 task_99.json 不存在
- **THEN** 系统返回错误 "Task 99 not found"

### Requirement: task_update 工具
系统 SHALL 提供 `task_update` 工具，接受 `task_id`（必填）、`status`（选填，enum: pending/in_progress/completed/deleted）、`add_blocked_by`（选填，整数数组）、`add_blocks`（选填，整数数组）参数。

#### Scenario: 更新状态为 completed 时清除依赖
- **WHEN** 调用 task_update，task_id=1，status="completed"
- **THEN** 系统将 task 1 的 status 设为 completed，并从所有其他 task 的 blockedBy 列表中移除 1

#### Scenario: 删除 task
- **WHEN** 调用 task_update，task_id=2，status="deleted"
- **THEN** 系统删除 `.tasks/task_2.json` 文件，返回 "Task 2 deleted"

#### Scenario: 添加依赖
- **WHEN** 调用 task_update，task_id=3，add_blocked_by=[1]，add_blocks=[5]
- **THEN** task 3 的 blockedBy 追加 1，blocks 追加 5（去重）

### Requirement: task_list 工具
系统 SHALL 提供 `task_list` 工具（无参数），返回所有 task 的摘要列表，格式为 `{marker} #{id}: {subject} @{owner} (blocked by: [...])`.

#### Scenario: 列出所有 task
- **WHEN** 调用 task_list，存在 3 个 task
- **THEN** 系统返回按文件名排序的 task 列表，pending 显示 "[ ]"，in_progress 显示 "[>]"，completed 显示 "[x]"

#### Scenario: 无 task
- **WHEN** 调用 task_list，`.tasks/` 目录为空
- **THEN** 系统返回 "No tasks."

### Requirement: claim_task 工具
系统 SHALL 提供 `claim_task` 工具，接受 `task_id`（必填，整数）参数，将 task 的 owner 设为调用者名称，status 设为 "in_progress"。

#### Scenario: Lead 认领 task
- **WHEN** lead 调用 claim_task，task_id=1
- **THEN** task 1 的 owner 设为 "lead"，status 设为 "in_progress"
