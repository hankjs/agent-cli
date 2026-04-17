use futures_util::StreamExt;
use tokio::sync::{mpsc, oneshot};
use std::time::{Duration, Instant};

use crate::permission::{PermissionChecker, PermissionDecision, PermissionResponse, PermissionRule};
use crate::streaming::*;
use crate::tool::{ToolContext, ToolExecutor, ToolRegistry};

/// Watchdog timeout: if no data received for this long, consider the stream stalled.
const STREAM_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the watchdog checks for stalled streams.
const WATCHDOG_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Fallback model used when the primary model is persistently overloaded (529).
const FALLBACK_MODEL: &str = "claude-sonnet-4-20250514";

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
    /// Streaming was interrupted by user; engine has rolled back its message state.
    Interrupted,
    Error(String),
    /// Informational: model was degraded due to persistent overload.
    ModelDegraded { from: String, to: String },
}

/// Commands that can be sent to the engine besides user messages.
pub enum EngineCommand {
    /// User message to submit
    UserMessage(String),
    /// Trigger manual context compression
    Compact,
}

pub struct QueryEngine {
    client: ApiClient,
    registry: ToolRegistry,
    messages: Vec<Message>,
    system_prompt: String,
    model: String,
    tool_ctx: ToolContext,
    permission_checker: PermissionChecker,
}

impl QueryEngine {
    pub fn new(
        client: ApiClient,
        registry: ToolRegistry,
        system_prompt: String,
        model: String,
        tool_ctx: ToolContext,
        permission_checker: PermissionChecker,
    ) -> Self {
        Self { client, registry, messages: Vec::new(), system_prompt, model, tool_ctx, permission_checker }
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

    /// Handle an engine command (message or slash command).
    pub async fn handle_command(&mut self, cmd: EngineCommand, tx: &mpsc::Sender<QueryEvent>) {
        match cmd {
            EngineCommand::UserMessage(input) => {
                self.submit(&input, tx).await;
            }
            EngineCommand::Compact => {
                self.force_compress();
                let _ = tx.send(QueryEvent::TextDelta(
                    "[Context compressed]\n".into()
                )).await;
                let _ = tx.send(QueryEvent::TurnComplete).await;
            }
        }
    }

    fn build_request_body(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": 16384,
            "system": self.system_prompt,
            "messages": self.messages,
            "tools": self.registry.api_definitions(),
        })
    }

    async fn run_loop(&mut self, tx: &mpsc::Sender<QueryEvent>) {
        // RetryState is shared across the entire turn (streaming + non-streaming
        // fallback) so that failure budgets are continuous.
        let mut retry_state = RetryState::new();

        loop {
            let _ = tx.send(QueryEvent::Spinner(SpinnerMode::Requesting)).await;
            let body = self.build_request_body();

            // --- Try streaming first ---
            let stream_result = self.run_stream(body.clone(), &mut retry_state, tx).await;

            match stream_result {
                StreamOutcome::Done(assistant_content, stop_reason) => {
                    // Normal completion — process tool calls or finish turn.
                    if let Some(next) = self.process_response(assistant_content, stop_reason, tx).await {
                        match next {
                            TurnAction::Continue => continue,
                            TurnAction::Finish => return,
                        }
                    }
                    return;
                }
                StreamOutcome::Aborted => return,
                StreamOutcome::StreamFailed => {
                    // Streaming failed — fall back to non-streaming request.
                    let fallback = self.run_non_stream(body, &mut retry_state, tx).await;
                    match fallback {
                        Some((assistant_content, stop_reason)) => {
                            if let Some(next) = self.process_response(assistant_content, stop_reason, tx).await {
                                match next {
                                    TurnAction::Continue => continue,
                                    TurnAction::Finish => return,
                                }
                            }
                            return;
                        }
                        None => return, // error already sent
                    }
                }
                StreamOutcome::NeedsModelDegradation => {
                    // Persistent 529 — degrade model and retry.
                    if self.try_degrade_model(tx).await {
                        retry_state = RetryState::new(); // fresh budget for new model
                        continue;
                    }
                    let _ = tx.send(QueryEvent::Error(
                        "All models overloaded. Please try again later.".into()
                    )).await;
                    return;
                }
            }
        }
    }

    /// Run a streaming API call. Returns the outcome for the caller to decide next steps.
    async fn run_stream(
        &mut self,
        body: serde_json::Value,
        retry_state: &mut RetryState,
        tx: &mpsc::Sender<QueryEvent>,
    ) -> StreamOutcome {
        let mut retry_state_clone = retry_state.clone();
        let stream = match self.client.stream(body, &mut retry_state_clone).await {
            Ok(s) => {
                *retry_state = retry_state_clone;
                s
            }
            Err(ApiClientError::Overloaded { .. }) => {
                *retry_state = retry_state_clone;
                return StreamOutcome::NeedsModelDegradation;
            }
            Err(e) => {
                *retry_state = retry_state_clone;
                let _ = tx.send(QueryEvent::Error(e.to_string())).await;
                return StreamOutcome::StreamFailed;
            }
        };

        let mut accumulator = StreamAccumulator::new();
        let mut stop_reason = None;
        let mut assistant_content: Vec<ContentBlock> = Vec::new();
        let mut last_data_at = Instant::now();
        let mut watchdog_interval = tokio::time::interval(WATCHDOG_CHECK_INTERVAL);

        let mut aborted = false;
        let mut stream_error = false;
        tokio::pin!(stream);

        loop {
            tokio::select! {
                biased;
                // User abort always has highest priority
                _ = self.tool_ctx.abort.changed() => {
                    if *self.tool_ctx.abort.borrow() {
                        aborted = true;
                        break;
                    }
                }
                // Watchdog: detect stalled connections
                _ = watchdog_interval.tick() => {
                    if last_data_at.elapsed() > STREAM_WATCHDOG_TIMEOUT {
                        let _ = tx.send(QueryEvent::Error(
                            "Stream stalled — no data received for 30s, retrying...".into()
                        )).await;
                        stream_error = true;
                        break;
                    }
                }
                result = stream.next() => {
                    last_data_at = Instant::now();
                    match result {
                        Some(Ok(event)) => match event {
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
                                stream_error = true;
                                break;
                            }
                            _ => {}
                        },
                        Some(Err(e)) => {
                            let _ = tx.send(QueryEvent::Error(e.to_string())).await;
                            stream_error = true;
                            break;
                        }
                        None => break, // stream ended normally
                    }
                }
            }
        }

        if aborted {
            // Keep partial assistant text in history so conversation context is preserved
            let partial_text: String = accumulator.text_blocks.values().cloned().collect::<Vec<_>>().join("");
            if !partial_text.is_empty() {
                self.messages_push_partial(partial_text);
            }
            let _ = tx.send(QueryEvent::Interrupted).await;
            return StreamOutcome::Aborted;
        }

        if stream_error {
            // Stream broke mid-flight. Keep only complete content blocks
            // (discard incomplete tool_use whose JSON hasn't closed).
            // Complete text and tool_use blocks already in assistant_content are safe.
            return StreamOutcome::StreamFailed;
        }

        StreamOutcome::Done(assistant_content, stop_reason)
    }

    /// Non-streaming fallback. Returns parsed content blocks + stop reason, or None on error.
    async fn run_non_stream(
        &self,
        body: serde_json::Value,
        retry_state: &mut RetryState,
        tx: &mpsc::Sender<QueryEvent>,
    ) -> Option<(Vec<ContentBlock>, Option<StopReason>)> {
        let mut retry_state_clone = retry_state.clone();
        let msg = match self.client.send(body, &mut retry_state_clone).await {
            Ok(m) => {
                *retry_state = retry_state_clone;
                m
            }
            Err(ApiClientError::Overloaded { .. }) => {
                *retry_state = retry_state_clone;
                // Bubble up for model degradation
                let _ = tx.send(QueryEvent::Error(
                    "Model overloaded (529) — will attempt model degradation.".into()
                )).await;
                return None;
            }
            Err(e) => {
                *retry_state = retry_state_clone;
                let _ = tx.send(QueryEvent::Error(e.to_string())).await;
                return None;
            }
        };

        // Replay the content to the TUI as if it were streamed.
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    let _ = tx.send(QueryEvent::TextDelta(text.clone())).await;
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let _ = tx.send(QueryEvent::ToolStart {
                        id: id.clone(), name: name.clone(), input: input.clone(),
                    }).await;
                }
                ContentBlock::Thinking { thinking } => {
                    let _ = tx.send(QueryEvent::ThinkingDelta(thinking.clone())).await;
                }
            }
        }

        // Determine stop reason from the content.
        let has_tool_use = msg.content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
        let stop = if has_tool_use { Some(StopReason::ToolUse) } else { Some(StopReason::EndTurn) };

        Some((msg.content, stop))
    }

    /// Process a complete response: commit to message history, handle tool calls.
    async fn process_response(
        &mut self,
        assistant_content: Vec<ContentBlock>,
        stop_reason: Option<StopReason>,
        tx: &mpsc::Sender<QueryEvent>,
    ) -> Option<TurnAction> {
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
                // Permission check + execution for each tool call
                let mut approved_calls = Vec::new();
                let mut tool_results = Vec::new();

                for (id, name, input) in tool_calls {
                    let tool_decision = self.registry.get(&name)
                        .map(|t| t.check_permissions(&input))
                        .unwrap_or(PermissionDecision::Ask);

                    let decision = self.permission_checker.check(&name, tool_decision);

                    match decision {
                        PermissionDecision::Allow => {
                            approved_calls.push((id, name, input));
                        }
                        PermissionDecision::Deny(reason) => {
                            let msg = format!("Permission denied: {reason}");
                            let _ = tx.send(QueryEvent::ToolComplete {
                                id: id.clone(), name: name.clone(), output: msg.clone(),
                            }).await;
                            tool_results.push(serde_json::json!({
                                "type": "tool_result",
                                "tool_use_id": id,
                                "content": msg,
                                "is_error": true,
                            }));
                        }
                        PermissionDecision::Ask => {
                            let (resp_tx, resp_rx) = oneshot::channel();
                            let _ = tx.send(QueryEvent::PermissionRequest {
                                tool_name: name.clone(),
                                input: input.clone(),
                                respond: resp_tx,
                            }).await;

                            match resp_rx.await {
                                Ok(PermissionResponse::Allow) => {
                                    approved_calls.push((id, name, input));
                                }
                                Ok(PermissionResponse::AlwaysAllow(pattern)) => {
                                    self.permission_checker.add_session_rule(PermissionRule {
                                        tool_pattern: pattern,
                                        behavior: PermissionDecision::Allow,
                                    });
                                    approved_calls.push((id, name, input));
                                }
                                Ok(PermissionResponse::Deny) | Err(_) => {
                                    let msg = "Permission denied by user".to_string();
                                    let _ = tx.send(QueryEvent::ToolComplete {
                                        id: id.clone(), name: name.clone(), output: msg.clone(),
                                    }).await;
                                    tool_results.push(serde_json::json!({
                                        "type": "tool_result",
                                        "tool_use_id": id,
                                        "content": msg,
                                        "is_error": true,
                                    }));
                                }
                            }
                        }
                    }
                }

                // Execute approved tool calls
                if !approved_calls.is_empty() {
                    let _ = tx.send(QueryEvent::Spinner(SpinnerMode::ToolExecuting)).await;
                    let results = ToolExecutor::execute(&self.registry, approved_calls, &self.tool_ctx).await;

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
                }

                self.messages.push(Message {
                    role: "user".into(),
                    content: vec![ContentBlock::Text {
                        text: serde_json::to_string(&tool_results).unwrap_or_default()
                    }],
                    id: None,
                });
                Some(TurnAction::Continue)
            }
            _ => {
                // end_turn or max_tokens — done
                self.maybe_compress();
                let _ = tx.send(QueryEvent::TurnComplete).await;
                Some(TurnAction::Finish)
            }
        }
    }

    /// Try to degrade the model. Returns true if degradation happened.
    async fn try_degrade_model(&mut self, tx: &mpsc::Sender<QueryEvent>) -> bool {
        if self.model == FALLBACK_MODEL {
            return false; // already on the fallback model
        }
        let old = self.model.clone();
        self.model = FALLBACK_MODEL.to_string();
        let _ = tx.send(QueryEvent::ModelDegraded {
            from: old,
            to: self.model.clone(),
        }).await;
        true
    }

    /// Push partial assistant text to messages (used on abort).
    fn messages_push_partial(&mut self, text: String) {
        self.messages.push(Message {
            role: "assistant".into(),
            content: vec![ContentBlock::Text { text }],
            id: None,
        });
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
            self.do_compress();
        }
    }

    /// Force context compression regardless of token count.
    pub fn force_compress(&mut self) {
        if self.messages.len() > 6 {
            self.do_compress();
        }
    }

    fn do_compress(&mut self) {
        let keep = self.messages.split_off(self.messages.len() - 6);
        let summary = "[Earlier conversation compressed]".to_string();
        self.messages = vec![Message {
            role: "user".into(),
            content: vec![ContentBlock::Text { text: summary }],
            id: None,
        }];
        self.messages.extend(keep);
    }

    pub fn save_history(&self) -> String {
        serde_json::to_string_pretty(&self.messages).unwrap_or_default()
    }
}

/// Outcome of a streaming attempt.
enum StreamOutcome {
    /// Stream completed successfully with content blocks and stop reason.
    Done(Vec<ContentBlock>, Option<StopReason>),
    /// User aborted the stream.
    Aborted,
    /// Stream failed (network error, SSE error, watchdog timeout).
    StreamFailed,
    /// Persistent 529 — needs model degradation.
    NeedsModelDegradation,
}

/// What to do after processing a response.
enum TurnAction {
    /// Continue the API loop (tool results need to be sent back).
    Continue,
    /// Turn is complete, return to caller.
    Finish,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::PermissionMode;
    use crate::tool::{Tool, ToolResult, ToolError};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "echoes input" }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            })
        }
        async fn call(&self, input: serde_json::Value, _ctx: &ToolContext) -> Result<ToolResult, ToolError> {
            let text = input.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ToolResult { data: serde_json::Value::String(text), new_messages: None })
        }
    }

    /// End-to-end integration test: submit a message to the real API, verify
    /// streaming output + tool call execution + permission popup flow.
    /// Requires ANTHROPIC_API_KEY. Run with: cargo test --lib -- --ignored
    #[tokio::test]
    #[ignore]
    async fn end_to_end_streaming_with_tool_call() {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be set");
        let client = crate::streaming::ApiClient::new(api_key, None).unwrap();
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool));

        let system = "You are a test assistant. When asked to echo, use the echo tool.".to_string();
        let checker = PermissionChecker::new(PermissionMode::Bypass, vec![]);
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let ctx = ToolContext { working_dir: std::path::PathBuf::from("."), abort: rx };
        let mut engine = QueryEngine::new(
            client, registry, system, "claude-sonnet-4-20250514".into(), ctx, checker,
        );

        let (query_tx, mut query_rx) = mpsc::channel(256);
        engine.submit("Please echo the text 'hello world' using the echo tool.", &query_tx).await;

        let mut _saw_text = false;
        let mut saw_tool_start = false;
        let mut saw_tool_complete = false;
        let mut saw_turn_complete = false;

        while let Ok(event) = query_rx.try_recv() {
            match event {
                QueryEvent::TextDelta(_) => _saw_text = true,
                QueryEvent::ToolStart { .. } => saw_tool_start = true,
                QueryEvent::ToolComplete { .. } => saw_tool_complete = true,
                QueryEvent::TurnComplete => { saw_turn_complete = true; break; }
                _ => {}
            }
        }

        assert!(saw_turn_complete, "expected TurnComplete");
        assert!(saw_tool_start, "expected ToolStart for echo tool");
        assert!(saw_tool_complete, "expected ToolComplete with echo result");
    }
}
