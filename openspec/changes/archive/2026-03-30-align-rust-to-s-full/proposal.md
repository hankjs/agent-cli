## Why

Rust CLI（main.rs）目前停留在 s06 水平（base tools + todo + subagent + skills + compression）。需要对齐 Python 参考实现 s_full.py，补齐 s07-s11 的全部机制：文件持久化 task system、后台任务、消息总线、多 agent 协作（teammates）、shutdown/plan 协议，以及 REPL 命令。

## What Changes

- 新增 `TaskManager`：基于 `.tasks/` 目录的文件持久化 task CRUD，支持 blockedBy/blocks 依赖图
- 新增 `BackgroundManager`：后台线程执行命令，通知队列，drain 机制
- 新增 `MessageBus`：基于 `.team/inbox/` 的 JSONL 文件消息系统（send/read/broadcast）
- 新增 `TeammateManager`：spawn 自治 teammate agent，idle/auto-claim 循环，identity re-injection
- 新增 shutdown 协议：request_id 握手机制
- 新增 plan approval 协议：approve/reject 流程
- 更新 `TodoManager`：字段从 text/status 改为 content/status/activeForm，工具名改为 `TodoWrite`
- 更新 subagent：`task` 工具增加 `agent_type` 参数（Explore vs general-purpose），Explore 模式只给 bash+read_file
- 更新 `agent_loop`：每轮 LLM 调用前 drain 后台通知 + 检查 lead inbox
- 更新 compression：TOKEN_THRESHOLD 从 50000 改为 100000
- 新增 REPL 命令：`/compact`、`/tasks`、`/team`、`/inbox`
- 更新 system prompt 和 REPL prompt marker（s_full >>）
- 新增 `uuid` crate 依赖

## Capabilities

### New Capabilities
- `file-tasks`: 文件持久化 TaskManager，task_create/task_get/task_update/task_list/claim_task 工具，blockedBy/blocks 依赖图
- `background`: BackgroundManager 后台线程执行，background_run/check_background 工具，通知队列 drain
- `messaging`: MessageBus 文件消息系统，send_message/read_inbox/broadcast 工具
- `teammates`: TeammateManager 多 agent 协作，spawn_teammate/list_teammates 工具，idle/auto-claim 循环
- `shutdown-plan`: shutdown_request 握手协议 + plan_approval approve/reject 流程
- `repl-commands`: REPL 命令 /compact /tasks /team /inbox

### Modified Capabilities
- `subagent`: task 工具增加 agent_type 参数，Explore 模式限制工具集为 bash+read_file
- `context-compact`: TOKEN_THRESHOLD 从 50000 改为 100000

## Impact

- `src/main.rs`：主要改动文件，新增约 500 行代码
- `Cargo.toml`：新增 `uuid` 依赖
- 文件系统：运行时创建 `.tasks/`、`.team/`、`.team/inbox/`、`.transcripts/` 目录
- System prompt：完全重写以引导使用新工具集
