## 1. 工具定义

- [x] 1.1 新增 `task_tool()` 函数，返回 task 工具的 Tool 定义（name/description/input_schema）
- [x] 1.2 新增 `child_tools()` 函数，返回子代理工具集（bash + read_file + write_file + edit_file）
- [x] 1.3 修改 agent_loop 中的 `with_tools` 调用，加入 task_tool

## 2. 子代理核心

- [x] 2.1 新增 `SUBAGENT_SYSTEM` 常量或在 main 中构造子代理 system prompt
- [x] 2.2 新增 `run_subagent` async 函数：独立 messages、子代理工具集、最多 30 轮 tool loop、返回最终文本摘要

## 3. 父级集成

- [x] 3.1 在 agent_loop 的工具执行分支中增加 `"task"` 匹配，调用 `run_subagent` 并打印描述信息
- [ ] 3.2 验证：手动测试父级派发子代理任务，确认上下文隔离和摘要返回
