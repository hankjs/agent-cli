## Context

Rust CLI（`src/main.rs`）是 Python 课程 agent 的 Rust 移植版。当前实现到 s06 水平，包含 base tools、TodoManager（内存）、subagent、skill loader、三层 context compression。参考实现 `agents/s_full.py` 整合了 s01-s11 全部机制。本次设计将 Rust 版对齐到 s_full.py 的完整功能集。

当前 Rust 代码约 745 行，预计改动后约 1200-1400 行。

## Goals / Non-Goals

**Goals:**
- 功能完全对齐 s_full.py：TaskManager、BackgroundManager、MessageBus、TeammateManager、shutdown/plan
- 保持 Rust 惯用写法（Arc/Mutex 共享状态、tokio spawn 替代 threading）
- 所有新增 struct 和 tool 定义遵循现有代码风格

**Non-Goals:**
- 不做性能优化或 Rust 特有的架构改进
- 不引入额外的 crate（除 uuid 外）
- 不实现 s12（worktree isolation）

## Decisions

### D1: 并发模型 — tokio::spawn 替代 Python threading
Python 用 `threading.Thread` 做后台任务和 teammate。Rust 用 `tokio::spawn` + `Arc<Mutex<T>>` 共享状态。

理由：项目已依赖 tokio（features=["full"]），自然选择。`Arc<Mutex<>>` 是 Rust 多任务共享可变状态的标准模式。

### D2: BackgroundManager 通知队列 — tokio::sync::mpsc 替代 Python Queue
Python 用 `queue.Queue`。Rust 用 `tokio::sync::mpsc::unbounded_channel`。

理由：tokio mpsc 是异步友好的，与 tokio spawn 配合自然。unbounded 避免背压问题（通知量小）。

### D3: TaskManager 文件 I/O — 同步 std::fs
TaskManager 的文件操作（读写 .tasks/*.json）使用同步 `std::fs`，不用 tokio::fs。

理由：文件小（单个 JSON < 1KB），操作快，同步更简单。Python 版也是同步的。

### D4: TeammateManager 状态持久化 — JSON 文件
与 Python 一致，team config 存储在 `.team/config.json`。teammate 状态（working/idle/shutdown）实时写入文件。

### D5: TodoManager 字段对齐
从 `{id, text, status}` 改为 `{content, status, activeForm}`，工具名从 `todo` 改为 `TodoWrite`。与 s_full.py 完全一致。

### D6: 保留现有功能
SkillLoader、三层 compression、subagent 保留，在此基础上扩展（subagent 加 agent_type，compression 改阈值）。

## Risks / Trade-offs

- [Mutex 死锁] → 每次锁的持有时间极短（读写小 JSON），不做跨锁操作
- [Teammate 线程泄漏] → daemon=true 等价：tokio::spawn 的 task 在 main 退出时自动取消
- [文件竞争] → teammate 和 lead 可能同时写 .tasks/ 文件 → 与 Python 版行为一致，不额外处理（课程级别可接受）
- [MessageBus JSONL 并发写] → 多个 teammate 可能同时 append → 与 Python 版一致，不加锁
