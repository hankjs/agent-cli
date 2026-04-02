# Context / Compaction / Attachments

Claude Code 能跑长会话，不是因为上下文无限，而是因为它一直在做上下文工程。

## 1. 附件系统是主循环的一部分

关键文件：

- `claude-code-source-code/src/utils/attachments.ts`
- `claude-code-source-code/src/query.ts:301`
- `claude-code-source-code/src/query.ts:1580`

不要把 attachment 当成“外围功能”。在 Claude Code 里，attachment 是主循环内建的 context 注入机制。

## 2. 两类 attachment 注入时机

### 2.1 输入阶段

`processUserInput()` 期间就会通过 `getAttachmentMessages(...)` 把输入相关附件提出来，例如：

- 用户提到的文件
- IDE 选择
- 某些 agent/skill 相关附件

### 2.2 工具执行后阶段

`query.ts` 在 tool results 完成后，会再次调用 `getAttachmentMessages(...)`，这次主要处理：

- queued command / task notification
- memory 附件
- skill discovery
- file-change 附件

这意味着附件不是“一次性加好”，而是每一轮都可能新增。

## 3. Relevant memory 不是同步查的

`query.ts` 在每轮开始时会先：

- `startRelevantMemoryPrefetch(...)`

对应代码：

- `claude-code-source-code/src/query.ts:301`
- `claude-code-source-code/src/utils/attachments.ts:2361`

设计意图是：

- memory 搜索比较慢
- 模型流式生成和工具执行期间可以并行预取
- 等到适合注入的时候，再消费预取结果

这是典型的 harness 优化：把“会用到但不一定马上要用”的上下文搬到后台。

## 4. Claude Code 不是只有一种 compact

### 4.1 microcompact

关键文件：

- `claude-code-source-code/src/services/compact/microCompact.ts:253`

作用：

- 清理老旧 tool result
- 控制 message 体积
- 支持 cached microcompact / cache edits 等机制

它更像“细粒度上下文清洁”。

### 4.2 autocompact

关键文件：

- `claude-code-source-code/src/services/compact/autoCompact.ts:241`

作用：

- 当 token 接近阈值时，自动触发摘要压缩
- 生成 compact summary
- 把旧历史折叠成更短的 post-compact messages

关键概念：

- `getEffectiveContextWindowSize()`
- `getAutoCompactThreshold()`
- `calculateTokenWarningState()`

### 4.3 reactive compact / overflow recovery

`query.ts` 里还有更重的恢复逻辑：

- prompt-too-long 后恢复
- max-output-tokens 恢复
- media-size error 恢复
- context collapse drain retry

也就是说：

- microcompact = 日常清理
- autocompact = proactive summary
- reactive compact = API 出错后的补救

## 5. `query()` 里上下文处理顺序很重要

在一轮 query 开始前，大致顺序是：

1. `applyToolResultBudget(...)`
2. snip compact（如果开了）
3. `microcompact(...)`
4. context collapse（如果开了）
5. `autocompact(...)`
6. `appendSystemContext(systemPrompt, systemContext)`
7. `prependUserContext(messagesForQuery, userContext)`
8. 发 API 请求

这个顺序说明两件事：

1. “system prompt/context 拼装”并不是最早发生的事
2. 发送给模型前，消息体已经被多轮预算与压缩处理过

## 6. 为什么 tool result budget 也属于上下文工程

`query.ts` 在很早的阶段就会调用：

- `applyToolResultBudget(...)`

它的目标不是权限，也不是格式化，而是：

- 限制超大工具输出长期滞留在上下文中
- 必要时把大结果持久化到磁盘，再在对话里放摘要/引用

所以 Claude Code 的上下文工程不仅在“消息压缩”，也在“结果外置”。

## 7. compact 后怎么续命

`buildPostCompactMessages(...)` 会把 compact 结果转成新的消息序列：

- compact summary
- 需要保留的 attachment
- 某些 hook 结果

对应代码：

- `claude-code-source-code/src/services/compact/compact.ts:330`

从 `query.ts` 的视角看，compact 不是“单独分支”，而是把当前 `messagesForQuery` 替换为新的、更短的消息数组，然后继续正常 query。

## 8. 这套机制的直观理解

可以把 Claude Code 的上下文管理理解成 4 层：

```text
第 1 层：system prompt / context / tool schemas
第 2 层：attachments（memory, skill, queued commands）
第 3 层：tool result budget / persisted output
第 4 层：microcompact / autocompact / reactive recovery
```

不是某一层单独起作用，而是它们叠在一起，才让长会话保持可用。

## 9. 读源码时先抓住这几个函数

推荐顺序：

1. `claude-code-source-code/src/query.ts:301`
2. `claude-code-source-code/src/query.ts:1580`
3. `claude-code-source-code/src/utils/attachments.ts:2361`
4. `claude-code-source-code/src/utils/attachments.ts:2937`
5. `claude-code-source-code/src/services/compact/microCompact.ts:253`
6. `claude-code-source-code/src/services/compact/autoCompact.ts:241`
7. `claude-code-source-code/src/services/compact/compact.ts:330`

如果这几个函数串起来了，你就能理解 Claude Code 为什么不像普通聊天机器人那样“越聊越忘 / 越聊越炸”。

