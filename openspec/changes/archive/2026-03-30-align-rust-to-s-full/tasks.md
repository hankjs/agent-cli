## 1. 依赖和基础设施

- [x] 1.1 Cargo.toml 添加 uuid 依赖（features=["v4"]）
- [x] 1.2 更新 TodoManager：字段改为 content/status/activeForm，工具名改为 TodoWrite，新增 has_open_items()

## 2. TaskManager（s07 文件持久化）

- [x] 2.1 实现 TaskManager struct：_next_id、_load、_save、create、get、update（含 completed 清除依赖和 deleted 删除文件）、list_all、claim
- [x] 2.2 添加 task_create、task_get、task_update、task_list、claim_task 五个 tool 定义

## 3. BackgroundManager（s08 后台任务）

- [x] 3.1 实现 BackgroundManager struct：run（tokio::spawn）、check、drain（mpsc channel）
- [x] 3.2 添加 background_run、check_background 两个 tool 定义

## 4. MessageBus（s09 消息系统）

- [x] 4.1 实现 MessageBus struct：send（JSONL append）、read_inbox（读取并清空）、broadcast
- [x] 4.2 添加 send_message、read_inbox、broadcast 三个 tool 定义

## 5. TeammateManager（s09/s11 多 agent 协作）

- [x] 5.1 实现 TeammateManager struct：spawn、_loop（工作阶段 + idle 阶段 + auto-claim）、list_all、member_names
- [x] 5.2 添加 spawn_teammate、list_teammates 两个 tool 定义
- [x] 5.3 Teammate agent 循环：独立 tool set（bash/read/write/edit/send_message/idle/claim_task）、inbox 检查、identity re-injection

## 6. Shutdown 和 Plan 协议（s10）

- [x] 6.1 实现 shutdown_requests/plan_requests HashMap，handle_shutdown_request 和 handle_plan_review 函数
- [x] 6.2 添加 shutdown_request、plan_approval、idle 三个 tool 定义

## 7. Subagent 更新

- [x] 7.1 task 工具增加 agent_type 参数（Explore vs general-purpose），Explore 模式只给 bash+read_file

## 8. Context Compression 更新

- [x] 8.1 TOKEN_THRESHOLD 从 50000 改为 100000

## 9. Agent Loop 整合

- [x] 9.1 agent_loop 每轮 LLM 调用前：drain 后台通知 + 检查 lead inbox
- [x] 9.2 tool dispatch 整合所有新工具（约 22 个工具）
- [x] 9.3 TodoWrite nag reminder 改为检查 has_open_items()

## 10. REPL 和 System Prompt

- [x] 10.1 REPL 添加 /compact、/tasks、/team、/inbox 命令
- [x] 10.2 更新 system prompt 和 REPL prompt marker（s_full >>）
- [x] 10.3 更新 main() 初始化所有 Manager 实例（Arc<Mutex<>> 共享）

## 11. 编译验证

- [x] 11.1 cargo build 编译通过
- [x] 11.2 修复所有编译错误
