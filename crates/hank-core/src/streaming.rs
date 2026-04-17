use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

/// SSE stream event types from the Anthropic Messages API.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageMeta },
    #[serde(rename = "content_block_start")]
    ContentBlockStart { index: usize, content_block: ContentBlock },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: Delta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta { delta: MessageDeltaBody, usage: Option<Usage> },
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "error")]
    Error { error: ApiError },
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageMeta {
    pub id: String,
    pub model: String,
    pub role: String,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageDeltaBody {
    pub stop_reason: Option<StopReason>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: serde_json::Value },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
}

/// A complete message with role and content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Stop reason from the API response (non-streaming only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stop_reason: Option<StopReason>,
    /// Output token count from the API response (non-streaming only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage_output_tokens: Option<u64>,
}

impl Message {
    /// Create a new message with only role and content (no metadata).
    pub fn new(role: impl Into<String>, content: Vec<ContentBlock>) -> Self {
        Self {
            role: role.into(),
            content,
            id: None,
            stop_reason: None,
            usage_output_tokens: None,
        }
    }
}

/// Accumulates partial JSON fragments for tool_use inputs during streaming.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    partial_json: HashMap<usize, String>,
    tool_info: HashMap<usize, (String, String)>, // index -> (id, name)
    pub text_blocks: HashMap<usize, String>,
}

impl StreamAccumulator {
    pub fn new() -> Self { Self::default() }

    pub fn on_content_block_start(&mut self, index: usize, block: &ContentBlock) {
        match block {
            ContentBlock::ToolUse { id, name, .. } => {
                self.tool_info.insert(index, (id.clone(), name.clone()));
                self.partial_json.insert(index, String::new());
            }
            ContentBlock::Text { .. } => {
                self.text_blocks.insert(index, String::new());
            }
            _ => {}
        }
    }

    pub fn on_delta(&mut self, index: usize, delta: &Delta) {
        match delta {
            Delta::InputJsonDelta { partial_json } => {
                self.partial_json.entry(index).or_default().push_str(partial_json);
            }
            Delta::TextDelta { text } => {
                self.text_blocks.entry(index).or_default().push_str(text);
            }
            _ => {}
        }
    }

    /// Parse accumulated JSON at content_block_stop. Returns (id, name, parsed_input).
    pub fn on_content_block_stop(&mut self, index: usize) -> Option<(String, String, serde_json::Value)> {
        let (id, name) = self.tool_info.remove(&index)?;
        let json_str = self.partial_json.remove(&index)?;
        let input = serde_json::from_str(&json_str).unwrap_or(serde_json::Value::Object(Default::default()));
        Some((id, name, input))
    }

    pub fn reset(&mut self) {
        self.partial_json.clear();
        self.tool_info.clear();
        self.text_blocks.clear();
    }
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// How an HTTP error should be handled.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    /// Transient — worth retrying with backoff (429, 500, 502, 503, 408, connection reset).
    Retryable,
    /// Client-side problem — retrying won't help (400, 401, 402, 403, 404).
    NonRetryable,
    /// Persistent overload — retry a few times then escalate to degradation (529).
    NeedsDegradation,
}

fn classify_status(status: u16) -> ErrorCategory {
    match status {
        429 | 500 | 502 | 503 | 408 => ErrorCategory::Retryable,
        529 => ErrorCategory::NeedsDegradation,
        _ => ErrorCategory::NonRetryable,
    }
}

/// Retry state passed between streaming and non-streaming attempts so failure
/// budgets are shared across the two layers.
#[derive(Debug, Clone, Default)]
pub struct RetryState {
    /// Total attempts so far (across stream + non-stream).
    pub attempts: u32,
    /// Consecutive 529 errors.
    pub consecutive_529: u32,
}

impl RetryState {
    pub fn new() -> Self { Self::default() }

    /// Returns true when consecutive 529s reach the degradation threshold.
    pub fn should_degrade(&self) -> bool {
        self.consecutive_529 >= MAX_529_BEFORE_DEGRADE
    }
}

/// Maximum consecutive 529 errors before triggering model degradation.
const MAX_529_BEFORE_DEGRADE: u32 = 3;
/// Base delay for exponential backoff (milliseconds).
const BACKOFF_BASE_MS: u64 = 500;
/// Maximum delay cap (milliseconds).
const BACKOFF_MAX_MS: u64 = 60_000;

fn max_retries() -> u32 {
    std::env::var("HANK_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}

/// Calculate backoff delay with exponential growth + random jitter.
/// If the server sent a `Retry-After` header, that value takes precedence.
fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(ra) = retry_after {
        return ra;
    }
    // Exponential: 500ms, 1s, 2s, 4s, ...
    let base = BACKOFF_BASE_MS.saturating_mul(1u64 << attempt.min(12));
    let capped = base.min(BACKOFF_MAX_MS);
    // Jitter: random value in [0, capped)
    let jitter = fastrand::u64(0..capped.max(1));
    Duration::from_millis(capped + jitter)
}

/// Parse the `Retry-After` header value (seconds) into a Duration.
fn parse_retry_after(resp: &reqwest::Response) -> Option<Duration> {
    let val = resp.headers().get("retry-after")?.to_str().ok()?;
    let secs: f64 = val.parse().ok()?;
    Some(Duration::from_secs_f64(secs))
}

// ---------------------------------------------------------------------------
// ApiClient
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ApiClientError {
    #[error("API key not set")]
    MissingApiKey,
    #[error("HTTP error: {status} {body}")]
    Http { status: u16, body: String },
    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),
    #[error("SSE parse error: {0}")]
    SseParse(String),
    #[error("Max retries exceeded after {attempts} attempts: {last_error}")]
    MaxRetries { attempts: u32, last_error: String },
    #[error("Overloaded: model unavailable after {attempts} attempts (529)")]
    Overloaded { attempts: u32 },
}

/// Streaming API client for the Anthropic Messages API.
pub struct ApiClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(api_key: String, base_url: Option<String>) -> Result<Self, ApiClientError> {
        if api_key.is_empty() {
            return Err(ApiClientError::MissingApiKey);
        }
        Ok(Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".into()),
            http: reqwest::Client::new(),
        })
    }

    /// Send a streaming request, returning a stream of StreamEvents.
    /// Retries on transient errors with exponential backoff + jitter.
    /// Tracks 529 errors in `retry_state` for cross-layer degradation.
    pub async fn stream(
        &self,
        body: serde_json::Value,
        retry_state: &mut RetryState,
    ) -> Result<Pin<Box<dyn futures_util::Stream<Item = Result<StreamEvent, ApiClientError>> + Send>>, ApiClientError> {
        use eventsource_stream::Eventsource;

        let mut body = body;
        body.as_object_mut().unwrap().insert("stream".into(), serde_json::Value::Bool(true));

        let max = max_retries();

        loop {
            retry_state.attempts += 1;
            let resp = self.http
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await?;

            let status = resp.status().as_u16();
            if !resp.status().is_success() {
                let category = classify_status(status);
                let retry_after = parse_retry_after(&resp);

                match category {
                    ErrorCategory::NonRetryable => {
                        let body_text = resp.text().await.unwrap_or_default();
                        return Err(ApiClientError::Http { status, body: body_text });
                    }
                    ErrorCategory::NeedsDegradation => {
                        retry_state.consecutive_529 += 1;
                        if retry_state.should_degrade() {
                            return Err(ApiClientError::Overloaded { attempts: retry_state.attempts });
                        }
                        if retry_state.attempts > max {
                            let body_text = resp.text().await.unwrap_or_default();
                            return Err(ApiClientError::MaxRetries { attempts: retry_state.attempts, last_error: body_text });
                        }
                        let delay = backoff_delay(retry_state.attempts - 1, retry_after);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    ErrorCategory::Retryable => {
                        // Non-529 retryable errors reset the 529 counter.
                        retry_state.consecutive_529 = 0;
                        if retry_state.attempts > max {
                            let body_text = resp.text().await.unwrap_or_default();
                            return Err(ApiClientError::MaxRetries { attempts: retry_state.attempts, last_error: body_text });
                        }
                        let delay = backoff_delay(retry_state.attempts - 1, retry_after);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                }
            }

            // Success — reset 529 counter.
            retry_state.consecutive_529 = 0;

            let stream = resp.bytes_stream().eventsource().filter_map(|result| async move {
                match result {
                    Ok(event) => {
                        if event.data.is_empty() || event.event == "ping" {
                            return None;
                        }
                        match serde_json::from_str::<StreamEvent>(&event.data) {
                            Ok(ev) => Some(Ok(ev)),
                            Err(e) => Some(Err(ApiClientError::SseParse(format!("{e}: {}", event.data)))),
                        }
                    }
                    Err(e) => Some(Err(ApiClientError::SseParse(e.to_string()))),
                }
            });

            return Ok(Box::pin(stream));
        }
    }

    /// Non-streaming request. Used as a fallback when streaming repeatedly fails.
    /// Shares the same `RetryState` so failure budgets are continuous.
    pub async fn send(
        &self,
        body: serde_json::Value,
        retry_state: &mut RetryState,
    ) -> Result<Message, ApiClientError> {
        let mut body = body;
        // Ensure stream is false for non-streaming.
        body.as_object_mut().unwrap().remove("stream");

        let max = max_retries();

        loop {
            retry_state.attempts += 1;
            let resp = self.http
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .timeout(Duration::from_secs(120))
                .send()
                .await?;

            let status = resp.status().as_u16();
            if !resp.status().is_success() {
                let category = classify_status(status);
                let retry_after = parse_retry_after(&resp);

                match category {
                    ErrorCategory::NonRetryable => {
                        let body_text = resp.text().await.unwrap_or_default();
                        return Err(ApiClientError::Http { status, body: body_text });
                    }
                    ErrorCategory::NeedsDegradation => {
                        retry_state.consecutive_529 += 1;
                        if retry_state.should_degrade() {
                            return Err(ApiClientError::Overloaded { attempts: retry_state.attempts });
                        }
                        if retry_state.attempts > max {
                            let body_text = resp.text().await.unwrap_or_default();
                            return Err(ApiClientError::MaxRetries { attempts: retry_state.attempts, last_error: body_text });
                        }
                        let delay = backoff_delay(retry_state.attempts - 1, retry_after);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    ErrorCategory::Retryable => {
                        retry_state.consecutive_529 = 0;
                        if retry_state.attempts > max {
                            let body_text = resp.text().await.unwrap_or_default();
                            return Err(ApiClientError::MaxRetries { attempts: retry_state.attempts, last_error: body_text });
                        }
                        let delay = backoff_delay(retry_state.attempts - 1, retry_after);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                }
            }

            // Parse the non-streaming response.
            retry_state.consecutive_529 = 0;
            let json: serde_json::Value = resp.json().await?;
            let content = parse_non_stream_response(json)?;
            return Ok(content);
        }
    }
}

/// Parse a non-streaming Messages API response into a Message.
fn parse_non_stream_response(json: serde_json::Value) -> Result<Message, ApiClientError> {
    let role = json.get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("assistant")
        .to_string();

    let mut blocks = Vec::new();
    if let Some(content) = json.get("content").and_then(|c| c.as_array()) {
        for item in content {
            let block_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    blocks.push(ContentBlock::Text { text });
                }
                "tool_use" => {
                    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let input = item.get("input").cloned().unwrap_or(serde_json::Value::Object(Default::default()));
                    blocks.push(ContentBlock::ToolUse { id, name, input });
                }
                "thinking" => {
                    let thinking = item.get("thinking").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    blocks.push(ContentBlock::Thinking { thinking });
                }
                _ => {}
            }
        }
    }

    // Parse stop_reason from the response.
    let stop_reason = json.get("stop_reason")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "end_turn" => Some(StopReason::EndTurn),
            "tool_use" => Some(StopReason::ToolUse),
            "max_tokens" => Some(StopReason::MaxTokens),
            "stop_sequence" => Some(StopReason::StopSequence),
            _ => None,
        });

    // Parse output_tokens from usage.
    let usage_output_tokens = json.get("usage")
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_u64());

    Ok(Message {
        role,
        content: blocks,
        id: json.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        stop_reason,
        usage_output_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_classification() {
        assert_eq!(classify_status(429), ErrorCategory::Retryable);
        assert_eq!(classify_status(500), ErrorCategory::Retryable);
        assert_eq!(classify_status(503), ErrorCategory::Retryable);
        assert_eq!(classify_status(408), ErrorCategory::Retryable);
        assert_eq!(classify_status(529), ErrorCategory::NeedsDegradation);
        assert_eq!(classify_status(400), ErrorCategory::NonRetryable);
        assert_eq!(classify_status(401), ErrorCategory::NonRetryable);
        assert_eq!(classify_status(402), ErrorCategory::NonRetryable);
        assert_eq!(classify_status(403), ErrorCategory::NonRetryable);
    }

    #[test]
    fn backoff_delay_grows_exponentially() {
        let d0 = backoff_delay(0, None);
        let d1 = backoff_delay(1, None);
        let d2 = backoff_delay(2, None);
        // Base: 500ms, 1000ms, 2000ms + jitter
        assert!(d0.as_millis() >= 500);
        assert!(d1.as_millis() >= 1000);
        assert!(d2.as_millis() >= 2000);
    }

    #[test]
    fn backoff_respects_retry_after() {
        let d = backoff_delay(0, Some(Duration::from_secs(10)));
        assert_eq!(d.as_secs(), 10);
    }

    #[test]
    fn retry_state_degrades_after_threshold() {
        let mut state = RetryState::new();
        state.consecutive_529 = 2;
        assert!(!state.should_degrade());
        state.consecutive_529 = 3;
        assert!(state.should_degrade());
    }

    #[test]
    fn parse_non_stream_response_works() {
        let json = serde_json::json!({
            "id": "msg_123",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello!"},
                {"type": "tool_use", "id": "tu_1", "name": "read", "input": {"path": "/tmp"}}
            ]
        });
        let msg = parse_non_stream_response(json).unwrap();
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content.len(), 2);
    }

    /// Integration test: sends a simple message to the Anthropic API and verifies
    /// the SSE stream produces the expected event sequence.
    /// Requires ANTHROPIC_API_KEY env var. Run with: cargo test --lib -- --ignored
    #[tokio::test]
    #[ignore]
    async fn streaming_api_returns_expected_event_sequence() {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be set to run integration tests");
        let client = ApiClient::new(api_key, None).unwrap();

        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "Say hello in exactly 3 words."}],
        });

        let mut retry_state = RetryState::new();
        let stream = client.stream(body, &mut retry_state).await.expect("stream should start");
        tokio::pin!(stream);

        let mut saw_message_start = false;
        let mut saw_content_block_start = false;
        let mut saw_text_delta = false;
        let mut saw_content_block_stop = false;
        let mut saw_message_delta = false;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => match event {
                    StreamEvent::MessageStart { .. } => saw_message_start = true,
                    StreamEvent::ContentBlockStart { .. } => saw_content_block_start = true,
                    StreamEvent::ContentBlockDelta { delta: Delta::TextDelta { .. }, .. } => saw_text_delta = true,
                    StreamEvent::ContentBlockStop { .. } => saw_content_block_stop = true,
                    StreamEvent::MessageDelta { .. } => saw_message_delta = true,
                    StreamEvent::MessageStop { .. } => break,
                    _ => {}
                },
                Err(e) => panic!("Stream error: {e}"),
            }
        }

        assert!(saw_message_start, "expected MessageStart event");
        assert!(saw_content_block_start, "expected ContentBlockStart event");
        assert!(saw_text_delta, "expected at least one TextDelta event");
        assert!(saw_content_block_stop, "expected ContentBlockStop event");
        assert!(saw_message_delta, "expected MessageDelta event");
    }
}
