## Why

main.rs 当前只有单层 agent loop，无法将复杂任务拆分为独立子任务。Python 版 s04_subagent.py 已验证了子代理模式的有效性——通过上下文隔离让父级保持清晰，子代理独立工作后只返回摘要。需要将此能力移植到 Rust CLI。

## What Changes

- 新增 `run_subagent` async 函数：创建独立 messages 上下文，运行子代理 tool loop（最多 30 轮），只返回最终文本摘要
- 新增 `task` 工具定义：父级可通过 task 工具派发子任务，传入 prompt 和 description
- 拆分工具集：子代理获得 bash/read_file/write_file/edit_file（不含 task 和 todo，防止递归和状态污染）
- 新增子代理专用 system prompt
- agent_loop 中处理 task 工具调用，串行执行子代理

## Capabilities

### New Capabilities
- `subagent`: 子代理派发与上下文隔离机制——task 工具定义、run_subagent 函数、子代理工具集、独立 system prompt

### Modified Capabilities

（无现有 spec 需要修改）

## Impact

- `src/main.rs`：新增 run_subagent 函数、task_tool 函数、修改 agent_loop 增加 task 分支
- API 调用量增加：每个子代理独立消耗 API 调用
- 无破坏性变更：现有工具和 todo 功能不受影响
