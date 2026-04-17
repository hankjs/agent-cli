use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;

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

/// Streaming API client for the Anthropic Messages API.
pub struct ApiClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

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
}

const RETRYABLE_STATUSES: &[u16] = &[429, 500, 502, 503, 529];

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
    /// Retries on transient errors with exponential backoff.
    pub async fn stream(
        &self,
        body: serde_json::Value,
    ) -> Result<Pin<Box<dyn futures_util::Stream<Item = Result<StreamEvent, ApiClientError>> + Send>>, ApiClientError> {
        use eventsource_stream::Eventsource;

        let mut body = body;
        body.as_object_mut().unwrap().insert("stream".into(), serde_json::Value::Bool(true));

        let mut attempts = 0u32;
        let max_retries = 3u32;

        loop {
            attempts += 1;
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
                let is_retryable = RETRYABLE_STATUSES.contains(&status);
                if is_retryable && attempts <= max_retries {
                    let delay = 1u64 << (attempts - 1); // 1s, 2s, 4s
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    continue;
                }
                let body_text = resp.text().await.unwrap_or_default();
                if attempts > max_retries && is_retryable {
                    return Err(ApiClientError::MaxRetries { attempts, last_error: body_text });
                }
                return Err(ApiClientError::Http { status, body: body_text });
            }

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
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let stream = client.stream(body).await.expect("stream should start");
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
