# Claude Code Source 学习索引

目标：给后续深入读 `claude-code-source-code/` 时留一份“先看什么、怎么看、核心链路在哪”的学习地图。

## 先知道的 4 件事

1. 这份源码来自 npm 包解包/还原，`README.md` 明确说明它不是 Anthropic 内部 monorepo 的完整源码；很多 `feature()` 分支对应的内部模块在发布包里被 DCE 删除了。
2. 真正最值得先读的不是 UI，而是最小执行链：`QueryEngine` -> `processUserInput` -> `query` -> `services/api/claude`。
3. Claude Code 的“Prompt harness”不是单个 prompt 文件，而是 6 层拼装：默认 system prompt、agent/custom prompt、user/system context、tool schema、cache/caching scope、compact/attachments。
4. `query.ts` 是核心大脑，但太大；第一遍不要全读，先带着问题读它的阶段划分。

## 文档目录

- `docs/learn/01-runtime-flow.md`
  - 入口、REPL/SDK 两条主链、一次完整 turn 的执行顺序
- `docs/learn/02-prompt-harness.md`
  - system prompt、context、tool schema、prompt cache 的拼装方式
- `docs/learn/03-tools-permissions-mcp.md`
  - tool pool、权限判定、streaming tool execution、MCP 注入
- `docs/learn/04-context-compaction-and-attachments.md`
  - memory/attachments、microcompact/autocompact/reactive compact

## 推荐阅读顺序

第一轮建议按下面顺序：

1. `claude-code-source-code/src/QueryEngine.ts:184`
   - 这是最干净的 headless/SDK 外壳，能先看清“一个 turn 怎样启动”。
2. `claude-code-source-code/src/utils/processUserInput/processUserInput.ts:85`
   - 先搞清输入如何分流成普通文本、slash command、bash、附件。
3. `claude-code-source-code/src/query.ts:219`
   - 再看真正的 agent loop。
4. `claude-code-source-code/src/constants/prompts.ts:444`
   - 看默认 system prompt 的 section 组成。
5. `claude-code-source-code/src/utils/systemPrompt.ts:41`
   - 看 default/custom/agent/override 的优先级。
6. `claude-code-source-code/src/utils/api.ts:119`
   - 看 tool schema 和 system prompt block 如何变成 API payload。
7. `claude-code-source-code/src/services/api/claude.ts:3213`
   - 看 system prompt block 最终如何带 cache_control 发给模型。
8. `claude-code-source-code/src/tools.ts:345`
   - 看 built-in tool + MCP tool 如何合并。
9. `claude-code-source-code/src/utils/permissions/permissions.ts:1158`
   - 看权限判定优先级。
10. `claude-code-source-code/src/services/mcp/client.ts:2226`
   - 看外部 MCP server 如何变成工具、命令、资源。

第二轮再回头看：

1. `claude-code-source-code/src/main.tsx:585`
2. `claude-code-source-code/src/screens/REPL.tsx`
3. `claude-code-source-code/src/utils/attachments.ts`
4. `claude-code-source-code/src/services/compact/*`

## 第一遍先忽略什么

- `feature()` 包裹但源码缺失的内部功能
- analytics / telemetry 细节
- 大量 UI 组件和 Ink 渲染细节
- 各种 ant-only 分支

第一遍最重要的是建立这 5 个问题的答案：

1. 用户输入如何进入系统？
2. prompt 到底由哪些来源拼起来？
3. 一次 API 调用前后发生了哪些预处理和恢复机制？
4. tool 是怎么被选择、执行、授权、回灌到上下文里的？
5. 长会话为什么不会因为上下文爆炸直接崩掉？

## 一句话总图

```text
CLI/REPL/SDK 输入
  -> processUserInput
  -> system prompt + context + tool schemas 组装
  -> query() 流式请求模型
  -> 发现 tool_use 就执行工具并生成 tool_result
  -> 附件 / memory / compact / MCP 状态继续注入
  -> 如有后续工具或继续推理就递归进入下一轮
```

