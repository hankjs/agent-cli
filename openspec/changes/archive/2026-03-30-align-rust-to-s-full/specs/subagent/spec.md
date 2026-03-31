## MODIFIED Requirements

### Requirement: task 工具定义
系统 SHALL 提供名为 `task` 的工具，接受 `prompt`（必填）和 `agent_type`（选填，enum: "Explore"/"general-purpose"，默认 "Explore"）参数，用于派发子代理任务。

#### Scenario: Explore 模式子代理
- **WHEN** 调用 task 工具，agent_type="Explore" 或未指定
- **THEN** 子代理只获得 bash 和 read_file 两个工具（不含 write_file 和 edit_file）

#### Scenario: general-purpose 模式子代理
- **WHEN** 调用 task 工具，agent_type="general-purpose"
- **THEN** 子代理获得 bash、read_file、write_file、edit_file 四个工具
