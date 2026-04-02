use futures_util::StreamExt;
use tokio::sync::{mpsc, oneshot};

use crate::permission::{PermissionChecker, PermissionDecision, PermissionResponse};
use crate::streaming::*;
use crate::tool::{ToolContext, ToolExecutor, ToolRegistry, ToolResult};

#[derive(Debug, Clone)]
pub enum SpinnerMode {
    Requesting,
    Thinking,
    Responding,
    ToolInput,
    ToolExecuting,
}

pub enum QueryEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart { id: String, name: String, input: serde_json::Value },
    ToolComplete { id: String, name: String, output: String },
    PermissionRequest {
        tool_name: String,
        input: serde_json::Value,
        respond: oneshot::Sender<PermissionResponse>,
    },
    Spinner(SpinnerMode),
    TurnComplete,
    Error(String),
}

pub struct QueryEngine {
    client: ApiClient,
    registry: ToolRegistry,
    messages: Vec<Message>,
    system_prompt: String,
    model: String,
    tool_ctx: ToolContext,
}

impl QueryEngine {
    pub fn new(
        client: ApiClient,
        registry: ToolRegistry,
        system_prompt: String,
        model: String,
        tool_ctx: ToolContext,
    ) -> Self {
        Self { client, registry, messages: Vec::new(), system_prompt, model, tool_ctx }
    }

    pub fn messages(&self) -> &[Message] { &self.messages }

    pub fn add_user_message(&mut self, text: &str) {
        self.messages.push(Message {
            role: "user".into(),
            content: vec![ContentBlock::Text { text: text.into() }],
            id: None,
        });
    }

    /// Submit a user message and run the query loop, sending events to tx.
    pub async fn submit(&mut self, input: &str, tx: &mpsc::Sender<QueryEvent>) {
        self.add_user_message(input);
        self.run_loop(tx).await;
    }

    async fn run_loop(&mut self, tx: &mpsc::Sender<QueryEvent>) {
        loop {
            let _ = tx.send(QueryEvent::Spinner(SpinnerMode::Requesting)).await;

            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": 16384,
                "system": self.system_prompt,
                "messages": self.messages,
                "tools": self.registry.api_definitions(),
            });

            let stream = match self.client.stream(body).await {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(QueryEvent::Error(e.to_string())).await;
                    return;
                }
            };

            let mut accumulator = StreamAccumulator::new();
            let mut stop_reason = None;
            let mut assistant_content: Vec<ContentBlock> = Vec::new();

            tokio::pin!(stream);
            while let Some(result) = stream.next().await {
                match result {
                    Ok(event) => match event {
                        StreamEvent::ContentBlockStart { index, content_block } => {
                            accumulator.on_content_block_start(index, &content_block);
                            if let ContentBlock::ToolUse { ref id, ref name, .. } = content_block {
                                let _ = tx.send(QueryEvent::Spinner(SpinnerMode::ToolInput)).await;
                                let _ = tx.send(QueryEvent::ToolStart {
                                    id: id.clone(), name: name.clone(),
                                    input: serde_json::Value::Null,
                                }).await;
                            }
                        }
                        StreamEvent::ContentBlockDelta { index, delta } => {
                            match &delta {
                                Delta::TextDelta { text } => {
                                    let _ = tx.send(QueryEvent::Spinner(SpinnerMode::Responding)).await;
                                    let _ = tx.send(QueryEvent::TextDelta(text.clone())).await;
                                }
                                Delta::ThinkingDelta { thinking } => {
                                    let _ = tx.send(QueryEvent::Spinner(SpinnerMode::Thinking)).await;
                                    let _ = tx.send(QueryEvent::ThinkingDelta(thinking.clone())).await;
                                }
                                _ => {}
                            }
                            accumulator.on_delta(index, &delta);
                        }
                        StreamEvent::ContentBlockStop { index } => {
                            if let Some((id, name, input)) = accumulator.on_content_block_stop(index) {
                                assistant_content.push(ContentBlock::ToolUse { id, name, input });
                            } else if let Some(text) = accumulator.text_blocks.remove(&index) {
                                assistant_content.push(ContentBlock::Text { text });
                            }
                        }
                        StreamEvent::MessageDelta { delta, .. } => {
                            stop_reason = delta.stop_reason;
                        }
                        StreamEvent::Error { error } => {
                            let _ = tx.send(QueryEvent::Error(error.message)).await;
                            return;
                        }
                        _ => {}
                    },
                    Err(e) => {
                        let _ = tx.send(QueryEvent::Error(e.to_string())).await;
                        return;
                    }
                }
            }

            // Append assistant message
            self.messages.push(Message {
                role: "assistant".into(),
                content: assistant_content.clone(),
                id: None,
            });

            // Check if we need to execute tools
            let tool_calls: Vec<_> = assistant_content.iter().filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some((id.clone(), name.clone(), input.clone()))
                } else { None }
            }).collect();

            match stop_reason {
                Some(StopReason::ToolUse) if !tool_calls.is_empty() => {
                    let _ = tx.send(QueryEvent::Spinner(SpinnerMode::ToolExecuting)).await;
                    let results = ToolExecutor::execute(&self.registry, tool_calls, &self.tool_ctx).await;

                    let mut tool_results = Vec::new();
                    for (id, result) in results {
                        let (content, is_error) = match result {
                            Ok(r) => {
                                let text = r.data.as_str().map(|s| s.to_string())
                                    .unwrap_or_else(|| r.data.to_string());
                                let _ = tx.send(QueryEvent::ToolComplete {
                                    id: id.clone(), name: String::new(), output: text.clone(),
                                }).await;
                                (text, false)
                            }
                            Err(e) => {
                                let msg = format!("Error: {e}");
                                let _ = tx.send(QueryEvent::ToolComplete {
                                    id: id.clone(), name: String::new(), output: msg.clone(),
                                }).await;
                                (msg, true)
                            }
                        };
                        tool_results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": content,
                            "is_error": is_error,
                        }));
                    }

                    self.messages.push(Message {
                        role: "user".into(),
                        content: vec![ContentBlock::Text {
                            text: serde_json::to_string(&tool_results).unwrap_or_default()
                        }],
                        id: None,
                    });
                    continue; // loop back to API
                }
                _ => {
                    // end_turn or max_tokens — done
                    self.maybe_compress();
                    let _ = tx.send(QueryEvent::TurnComplete).await;
                    return;
                }
            }
        }
    }

    fn estimate_tokens(&self) -> usize {
        self.messages.iter().map(|m| {
            m.content.iter().map(|b| match b {
                ContentBlock::Text { text } => text.len() / 4,
                ContentBlock::ToolUse { input, .. } => input.to_string().len() / 4,
                ContentBlock::Thinking { thinking } => thinking.len() / 4,
            }).sum::<usize>()
        }).sum()
    }

    fn maybe_compress(&mut self) {
        if self.estimate_tokens() > 100_000 && self.messages.len() > 6 {
            let keep = self.messages.split_off(self.messages.len() - 6);
            let summary = "[Earlier conversation compressed]".to_string();
            self.messages = vec![Message {
                role: "user".into(),
                content: vec![ContentBlock::Text { text: summary }],
                id: None,
            }];
            self.messages.extend(keep);
        }
    }

    pub fn save_history(&self) -> String {
        serde_json::to_string_pretty(&self.messages).unwrap_or_default()
    }
}
