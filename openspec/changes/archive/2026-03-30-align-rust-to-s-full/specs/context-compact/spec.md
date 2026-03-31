## MODIFIED Requirements

### Requirement: auto_compact 触发阈值
agent_loop SHALL 在每轮 LLM 调用前检查 token 估算值，当超过 TOKEN_THRESHOLD（100000）时触发 auto_compact。

#### Scenario: 阈值触发
- **WHEN** 对话历史的 token 估算值超过 100000
- **THEN** 系统触发 auto_compact，保存 transcript 并用 LLM 生成摘要替换历史
