## Context

main.rs 是一个 Rust 异步 CLI agent，使用 Anthropic API，当前有 bash/read_file/write_file/edit_file/todo 五个工具。Python 版 s04_subagent.py 已实现子代理模式，作为参考蓝本。

## Goals / Non-Goals

**Goals:**
- 忠实移植 s04_subagent.py 的子代理模式到 main.rs
- 子代理拥有独立上下文（fresh messages），不污染父级
- 子代理只返回最终文本摘要给父级
- 串行执行子代理（一次一个）

**Non-Goals:**
- 并发子代理执行（后续增强）
- 递归子代理（子代理不能再派发子代理）
- 子代理使用 todo 工具

## Decisions

**1. 子代理工具集 = 父级工具集 - task - todo**

子代理获得 bash/read_file/write_file/edit_file。不给 task（防递归），不给 todo（子代理是短期任务，不需要进度跟踪）。与 Python 版一致。

替代方案：给子代理 todo → 增加复杂度，子代理生命周期短，不值得。

**2. run_subagent 作为独立 async 函数**

签名：`async fn run_subagent(client, model, workdir, prompt) -> String`

内部创建独立 messages vec，运行最多 30 轮 tool loop，提取最终文本返回。与 agent_loop 结构类似但更简单（无 todo，无 nag reminder）。

替代方案：复用 agent_loop 加参数控制 → 增加 agent_loop 复杂度，不如独立函数清晰。

**3. task 工具定义**

```json
{
  "name": "task",
  "description": "Spawn a subagent with fresh context. It shares the filesystem but not conversation history.",
  "input_schema": {
    "properties": {
      "prompt": { "type": "string" },
      "description": { "type": "string" }
    },
    "required": ["prompt"]
  }
}
```

与 Python 版完全一致。

**4. 子代理 system prompt**

`"You are a coding subagent at {cwd}. Complete the given task, then summarize your findings."`

与 Python 版一致。

## Risks / Trade-offs

- [API 成本增加] → 每个子代理独立消耗 token。缓解：30 轮上限 + 子代理 max_tokens=8000
- [子代理写文件冲突] → 串行执行避免此问题。并发版本需要额外设计
- [子代理卡死] → 30 轮上限兜底，与 Python 版一致
