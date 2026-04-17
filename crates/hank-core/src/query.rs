use futures_util::StreamExt;
use tokio::sync::{mpsc, oneshot};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
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

/// Default max_tokens per API request.
const DEFAULT_MAX_TOKENS: u32 = 16384;

// ---------------------------------------------------------------------------
// Fuse 1: Tool Loop Detection
// ---------------------------------------------------------------------------

/// Warn after this many identical (call+result) repetitions.
const LOOP_WARN_THRESHOLD: u32 = 5;
/// Block the tool after this many identical repetitions.
const LOOP_BREAK_THRESHOLD: u32 = 10;
/// Global no-progress limit — hard stop regardless of individual tool counts.
const GLOBAL_NO_PROGRESS_LIMIT: u32 = 30;

#[derive(Debug, PartialEq)]
enum LoopStatus {
    Ok,
    /// Same call+result repeated ≥ WARN times — inject a warning but keep going.
    Warn,
    /// Same call+result repeated ≥ BREAK times — stop this tool.
    Break,
    /// Total no-progress calls across all tools hit the global limit.
    GlobalBreak,
}

struct LoopDetector {
    /// call_fingerprint → (same-result streak, last_result_hash)
    history: HashMap<u64, (u32, u64)>,
    /// Cumulative no-progress calls across all tools.
    global_no_progress: u32,
    /// Whether a loop warning has already been injected this run.
    warn_injected: bool,
}

impl LoopDetector {
    fn new() -> Self {
        Self { history: HashMap::new(), global_no_progress: 0, warn_injected: false }
    }

    /// Check a tool invocation for loop behavior.
    /// `result` is the stringified tool output.
    fn check(&mut self, tool_name: &str, input: &serde_json::Value, result: &str) -> LoopStatus {
        let call_fp = Self::hash_call(tool_name, input);
        let result_fp = Self::hash_str(result);

        let entry = self.history.entry(call_fp).or_insert((0, 0));
        if entry.1 == result_fp {
            // Same call, same result — no progress.
            entry.0 += 1;
            self.global_no_progress += 1;
        } else {
            // Result changed — reset streak for this call.
            entry.0 = 1;
            entry.1 = result_fp;
        }

        if self.global_no_progress >= GLOBAL_NO_PROGRESS_LIMIT {
            LoopStatus::GlobalBreak
        } else if entry.0 >= LOOP_BREAK_THRESHOLD {
            LoopStatus::Break
        } else if entry.0 >= LOOP_WARN_THRESHOLD {
            LoopStatus::Warn
        } else {
            LoopStatus::Ok
        }
    }

    fn hash_call(name: &str, input: &serde_json::Value) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut hasher);
        // serde_json::Value uses BTreeMap for objects — keys are already sorted.
        let stable = serde_json::to_string(input).unwrap_or_default();
        stable.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_str(s: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        s.hash(&mut hasher);
        hasher.finish()
    }
}

// ---------------------------------------------------------------------------
// Fuse 2: Token Budget Control
// ---------------------------------------------------------------------------

/// Default output-token budget for a single agent run.
const DEFAULT_TOKEN_BUDGET: u64 = 200_000;
/// Inject a nudge at this fraction of the budget.
const BUDGET_NUDGE_RATIO: f64 = 0.9;
/// Consecutive turns with output below this count are considered "low".
const LOW_OUTPUT_THRESHOLD: u64 = 500;
/// Stop after this many consecutive low-output turns.
const LOW_STREAK_LIMIT: u32 = 2;
/// Only check diminishing returns after this much total output.
const MIN_OUTPUT_FOR_DIMINISH_CHECK: u64 = 5000;

#[derive(Debug, PartialEq)]
enum BudgetStatus {
    Ok,
    /// 90 % of budget consumed — inject a nudge.
    Nudge,
    /// Diminishing returns detected — stop.
    Stop,
}

struct TokenBudget {
    budget: u64,
    total_output: u64,
    low_streak: u32,
    nudge_injected: bool,
}

impl TokenBudget {
    fn new() -> Self {
        let budget = std::env::var("HANK_TOKEN_BUDGET")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_TOKEN_BUDGET);
        Self { budget, total_output: 0, low_streak: 0, nudge_injected: false }
    }

    fn check(&mut self, output_tokens: u64) -> BudgetStatus {
        self.total_output += output_tokens;

        // Diminishing returns: ≥2 consecutive turns with <500 tokens output,
        // but only after we've already produced a meaningful amount.
        if self.total_output > MIN_OUTPUT_FOR_DIMINISH_CHECK {
            if output_tokens < LOW_OUTPUT_THRESHOLD {
                self.low_streak += 1;
            } else {
                self.low_streak = 0;
            }
            if self.low_streak >= LOW_STREAK_LIMIT {
                return BudgetStatus::Stop;
            }
        }

        // 90 % budget nudge (fire once).
        if !self.nudge_injected
            && self.total_output as f64 >= self.budget as f64 * BUDGET_NUDGE_RATIO
        {
            self.nudge_injected = true;
            return BudgetStatus::Nudge;
        }

        BudgetStatus::Ok
    }
}

// ---------------------------------------------------------------------------
// Fuse 3: Output Truncation Recovery
// ---------------------------------------------------------------------------

/// Maximum recovery attempts before giving up.
const MAX_TRUNCATION_RECOVERY: u32 = 3;
/// Elevated max_tokens used on first recovery attempt.
const ELEVATED_MAX_TOKENS: u32 = 65536;

struct TruncationRecovery {
    count: u32,
}

impl TruncationRecovery {
    fn new() -> Self { Self { count: 0 } }

    /// Returns `true` if we should retry (haven't exhausted recovery attempts).
    fn should_retry(&mut self) -> bool {
        self.count += 1;
        self.count <= MAX_TRUNCATION_RECOVERY
    }

    fn recovery_message(&self) -> &str {
        if self.count <= 1 {
            "Your output was truncated due to token limits. Continue directly from where you left off — do not apologize, do not recap what you were doing. Break remaining work into smaller chunks."
        } else {
            "Output truncated again. Significantly reduce your output — only give key conclusions and essential information."
        }
    }
}

// ---------------------------------------------------------------------------
// Max turns limit
// ---------------------------------------------------------------------------

/// Hard cap on the number of agent turns per user message.
const DEFAULT_MAX_TURNS: u32 = 200;

fn max_turns_limit() -> u32 {
    std::env::var("HANK_MAX_TURNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_TURNS)
}

// ---------------------------------------------------------------------------
// QueryEngine types
// ---------------------------------------------------------------------------

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
        self.messages.push(Message::new("user", vec![ContentBlock::Text { text: text.into() }]));
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

    fn build_request_body(&self, max_tokens: u32) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": self.system_prompt,
            "messages": self.messages,
            "tools": self.registry.api_definitions(),
        })
    }

    async fn run_loop(&mut self, tx: &mpsc::Sender<QueryEvent>) {
        // RetryState is shared across the entire turn (streaming + non-streaming
        // fallback) so that failure budgets are continuous.
        let mut retry_state = RetryState::new();

        // Three fuses — freshly created per agent run.
        let mut loop_detector = LoopDetector::new();
        let mut token_budget = TokenBudget::new();
        let mut truncation_recovery = TruncationRecovery::new();

        let max_turns = max_turns_limit();
        let mut turn = 0u32;
        let mut max_tokens = DEFAULT_MAX_TOKENS;

        loop {
            // --- Max turns check ---
            turn += 1;
            if turn > max_turns {
                let _ = tx.send(QueryEvent::Error(
                    format!("Reached maximum turn limit ({max_turns}). Stopping to prevent runaway execution.")
                )).await;
                let _ = tx.send(QueryEvent::TurnComplete).await;
                return;
            }

            let _ = tx.send(QueryEvent::Spinner(SpinnerMode::Requesting)).await;
            let body = self.build_request_body(max_tokens);

            // --- Try streaming first ---
            let stream_result = self.run_stream(body.clone(), &mut retry_state, tx).await;

            let (assistant_content, stop_reason, output_tokens) = match stream_result {
                StreamOutcome::Done(content, stop, tokens) => (content, stop, tokens),
                StreamOutcome::Aborted => return,
                StreamOutcome::StreamFailed => {
                    // Streaming failed — fall back to non-streaming request.
                    let fallback = self.run_non_stream(body, &mut retry_state, tx).await;
                    match fallback {
                        Some((content, stop, tokens)) => (content, stop, tokens),
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
            };

            // --- Fuse 2: Token budget check ---
            let budget_status = token_budget.check(output_tokens);
            match budget_status {
                BudgetStatus::Stop => {
                    // Commit whatever the assistant said, then stop.
                    self.messages.push(Message::new("assistant", assistant_content));
                    let _ = tx.send(QueryEvent::Error(
                        format!(
                            "Token budget: diminishing returns detected (total output: {} tokens). Stopping.",
                            token_budget.total_output
                        )
                    )).await;
                    let _ = tx.send(QueryEvent::TurnComplete).await;
                    return;
                }
                BudgetStatus::Nudge | BudgetStatus::Ok => {
                    // Nudge will be injected into the next tool-results message if applicable.
                }
            }

            // --- Fuse 3: Truncation recovery ---
            if matches!(stop_reason, Some(StopReason::MaxTokens)) {
                // Keep the partial assistant output in history.
                self.messages.push(Message::new("assistant", assistant_content));

                if truncation_recovery.should_retry() {
                    // Step 1: raise max_tokens on first attempt.
                    if truncation_recovery.count == 1 {
                        max_tokens = ELEVATED_MAX_TOKENS;
                    }
                    // Step 2: inject recovery instruction.
                    let recovery_msg = truncation_recovery.recovery_message().to_string();
                    let _ = tx.send(QueryEvent::TextDelta(
                        format!("\n[Truncation recovery {}/{}]\n", truncation_recovery.count, MAX_TRUNCATION_RECOVERY)
                    )).await;
                    self.messages.push(Message::new("user", vec![ContentBlock::Text { text: recovery_msg }]));
                    continue; // retry with recovery message
                } else {
                    let _ = tx.send(QueryEvent::Error(
                        format!(
                            "Output truncated {} times. Partial results preserved.",
                            MAX_TRUNCATION_RECOVERY
                        )
                    )).await;
                    let _ = tx.send(QueryEvent::TurnComplete).await;
                    return;
                }
            }

            // --- Normal processing (tool calls or end_turn) ---
            let inject_nudge = budget_status == BudgetStatus::Nudge;
            if let Some(next) = self.process_response(
                assistant_content, stop_reason, tx,
                &mut loop_detector, inject_nudge, &token_budget,
            ).await {
                match next {
                    TurnAction::Continue => continue,
                    TurnAction::Finish => return,
                }
            }
            return;
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
        let mut output_tokens: u64 = 0;

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
                            StreamEvent::MessageDelta { delta, usage } => {
                                stop_reason = delta.stop_reason;
                                // Capture output token count for budget tracking.
                                if let Some(u) = usage {
                                    if let Some(t) = u.output_tokens {
                                        output_tokens = t;
                                    }
                                }
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
            return StreamOutcome::StreamFailed;
        }

        // If the API didn't report output_tokens, estimate from the content.
        if output_tokens == 0 {
            output_tokens = Self::estimate_content_tokens(&assistant_content);
        }

        StreamOutcome::Done(assistant_content, stop_reason, output_tokens)
    }

    /// Non-streaming fallback. Returns parsed content blocks + stop reason + output tokens, or None on error.
    async fn run_non_stream(
        &self,
        body: serde_json::Value,
        retry_state: &mut RetryState,
        tx: &mpsc::Sender<QueryEvent>,
    ) -> Option<(Vec<ContentBlock>, Option<StopReason>, u64)> {
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

        // Use the actual stop_reason and output_tokens from the API response.
        let output_tokens = msg.usage_output_tokens.unwrap_or_else(
            || Self::estimate_content_tokens(&msg.content)
        );
        let stop_reason = msg.stop_reason.or_else(|| {
            let has_tool_use = msg.content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
            if has_tool_use { Some(StopReason::ToolUse) } else { Some(StopReason::EndTurn) }
        });

        Some((msg.content, stop_reason, output_tokens))
    }

    /// Process a complete response: commit to message history, handle tool calls.
    /// Now also performs loop detection and optionally injects budget nudge.
    async fn process_response(
        &mut self,
        assistant_content: Vec<ContentBlock>,
        stop_reason: Option<StopReason>,
        tx: &mpsc::Sender<QueryEvent>,
        loop_detector: &mut LoopDetector,
        inject_nudge: bool,
        token_budget: &TokenBudget,
    ) -> Option<TurnAction> {
        // Append assistant message
        self.messages.push(Message::new("assistant", assistant_content.clone()));

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

                    // Snapshot tool names+inputs for loop detection (before move).
                    let call_info: Vec<(String, String, serde_json::Value)> = approved_calls.iter()
                        .map(|(id, name, input)| (id.clone(), name.clone(), input.clone()))
                        .collect();

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

                        // --- Fuse 1: Loop detection ---
                        if let Some(info) = call_info.iter().find(|(cid, _, _)| cid == &id) {
                            let status = loop_detector.check(&info.1, &info.2, &content);
                            match status {
                                LoopStatus::GlobalBreak => {
                                    let _ = tx.send(QueryEvent::Error(
                                        format!(
                                            "Global loop fuse triggered: {} no-progress tool calls. Stopping.",
                                            GLOBAL_NO_PROGRESS_LIMIT
                                        )
                                    )).await;
                                    // Still commit the tool results we have so far.
                                    tool_results.push(serde_json::json!({
                                        "type": "tool_result",
                                        "tool_use_id": id,
                                        "content": content,
                                        "is_error": is_error,
                                    }));
                                    self.push_tool_results(tool_results);
                                    let _ = tx.send(QueryEvent::TurnComplete).await;
                                    return Some(TurnAction::Finish);
                                }
                                LoopStatus::Break => {
                                    let _ = tx.send(QueryEvent::Error(
                                        format!(
                                            "Loop detected: tool '{}' called {} times with identical results. Stopping.",
                                            info.1, LOOP_BREAK_THRESHOLD
                                        )
                                    )).await;
                                    tool_results.push(serde_json::json!({
                                        "type": "tool_result",
                                        "tool_use_id": id,
                                        "content": content,
                                        "is_error": is_error,
                                    }));
                                    self.push_tool_results(tool_results);
                                    let _ = tx.send(QueryEvent::TurnComplete).await;
                                    return Some(TurnAction::Finish);
                                }
                                LoopStatus::Warn => {
                                    // Inject warning once.
                                    if !loop_detector.warn_injected {
                                        loop_detector.warn_injected = true;
                                    }
                                }
                                LoopStatus::Ok => {}
                            }
                        }

                        tool_results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": content,
                            "is_error": is_error,
                        }));
                    }
                }

                // Build the tool-results user message, optionally with fuse warnings.
                let mut extra_text = String::new();

                // Fuse 1: loop warning
                if loop_detector.warn_injected {
                    extra_text.push_str(
                        "[LOOP_WARNING] You are repeatedly calling the same tool with the same arguments and getting the same results. \
                         This indicates no progress. Please try a different approach or tool to accomplish the task.\n"
                    );
                    // Reset so we only inject once per detection.
                    loop_detector.warn_injected = false;
                }

                // Fuse 2: budget nudge
                if inject_nudge {
                    let pct = (token_budget.total_output as f64 / token_budget.budget as f64 * 100.0) as u32;
                    extra_text.push_str(&format!(
                        "[BUDGET_NUDGE] Output token budget is {pct}% consumed ({}/{}). \
                         Please be concise and focus on completing the task efficiently. \
                         Continue working — do not summarize what you have done so far.\n",
                        token_budget.total_output, token_budget.budget
                    ));
                }

                let mut content_blocks = vec![ContentBlock::Text {
                    text: serde_json::to_string(&tool_results).unwrap_or_default()
                }];
                if !extra_text.is_empty() {
                    content_blocks.push(ContentBlock::Text { text: extra_text });
                }
                self.messages.push(Message::new("user", content_blocks));
                Some(TurnAction::Continue)
            }
            _ => {
                // end_turn — done
                self.maybe_compress();
                let _ = tx.send(QueryEvent::TurnComplete).await;
                Some(TurnAction::Finish)
            }
        }
    }

    /// Push tool results as a user message.
    fn push_tool_results(&mut self, tool_results: Vec<serde_json::Value>) {
        self.messages.push(Message::new("user", vec![ContentBlock::Text {
            text: serde_json::to_string(&tool_results).unwrap_or_default()
        }]));
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
        self.messages.push(Message::new("assistant", vec![ContentBlock::Text { text }]));
    }

    /// Estimate token count from content blocks (rough: 1 token ≈ 4 chars).
    fn estimate_content_tokens(content: &[ContentBlock]) -> u64 {
        content.iter().map(|b| match b {
            ContentBlock::Text { text } => text.len() as u64 / 4,
            ContentBlock::ToolUse { input, .. } => input.to_string().len() as u64 / 4,
            ContentBlock::Thinking { thinking } => thinking.len() as u64 / 4,
        }).sum()
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
        self.messages = vec![Message::new("user", vec![ContentBlock::Text { text: summary }])];
        self.messages.extend(keep);
    }

    pub fn save_history(&self) -> String {
        serde_json::to_string_pretty(&self.messages).unwrap_or_default()
    }
}

/// Outcome of a streaming attempt.
enum StreamOutcome {
    /// Stream completed successfully with content blocks, stop reason, and output token count.
    Done(Vec<ContentBlock>, Option<StopReason>, u64),
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

    #[test]
    fn loop_detector_warns_and_breaks() {
        let mut det = LoopDetector::new();
        let input = serde_json::json!({"path": "/tmp/foo"});
        let result = "file content here";

        for _ in 0..4 {
            assert_eq!(det.check("read_file", &input, result), LoopStatus::Ok);
        }
        // 5th same call+result → Warn
        assert_eq!(det.check("read_file", &input, result), LoopStatus::Warn);

        for _ in 0..4 {
            det.check("read_file", &input, result);
        }
        // 10th → Break
        assert_eq!(det.check("read_file", &input, result), LoopStatus::Break);
    }

    #[test]
    fn loop_detector_resets_on_different_result() {
        let mut det = LoopDetector::new();
        let input = serde_json::json!({"path": "/tmp/foo"});

        for i in 0..8 {
            det.check("read_file", &input, &format!("content v{i}"));
        }
        // Each call had a different result → all Ok, no streak.
        assert_eq!(det.global_no_progress, 0);
    }

    #[test]
    fn loop_detector_global_break() {
        let mut det = LoopDetector::new();
        // Simulate 30 no-progress calls by directly setting the counter,
        // then verify the next no-progress call triggers GlobalBreak.
        det.global_no_progress = 29;
        let input = serde_json::json!({"x": 1});
        // First call for this tool — sets the baseline (result_hash changes from 0 → hash("same")).
        det.check("tool_x", &input, "same");
        // Second call — same result, increments global_no_progress to 30.
        assert_eq!(det.check("tool_x", &input, "same"), LoopStatus::GlobalBreak);
    }

    #[test]
    fn token_budget_nudge_and_stop() {
        let mut budget = TokenBudget {
            budget: 10000,
            total_output: 0,
            low_streak: 0,
            nudge_injected: false,
        };
        // Below 90%
        assert_eq!(budget.check(5000), BudgetStatus::Ok);
        // Crosses 90%
        assert_eq!(budget.check(4500), BudgetStatus::Nudge);
        // Nudge already injected
        assert_eq!(budget.check(100), BudgetStatus::Ok);
        // Diminishing returns (two consecutive low outputs after 5000 total)
        assert_eq!(budget.check(100), BudgetStatus::Stop);
    }

    #[test]
    fn truncation_recovery_limits() {
        let mut tr = TruncationRecovery::new();
        assert!(tr.should_retry()); // 1
        assert!(tr.should_retry()); // 2
        assert!(tr.should_retry()); // 3
        assert!(!tr.should_retry()); // 4 — exceeded
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
