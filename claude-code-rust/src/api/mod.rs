//! API Module - OpenAI/DeepSeek compatible API Client

use crate::config::Settings;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct ApiClient {
    settings: Settings,
    http_client: Client,
}

impl ApiClient {
    pub fn new(settings: Settings) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(settings.api.timeout))
            .build()
            .unwrap_or_default();

        Self {
            settings,
            http_client,
        }
    }

    pub fn get_api_key(&self) -> Option<String> {
        self.settings.api.get_api_key()
    }

    pub fn get_base_url(&self) -> String {
        self.settings.api.get_base_url()
    }

    pub fn get_model(&self) -> &str {
        &self.settings.model
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> anyhow::Result<ChatResponse> {
        let api_key = self.get_api_key()
            .ok_or_else(|| anyhow::anyhow!("API key not configured"))?;

        let request = ChatRequest {
            model: self.settings.api.get_model_id(&self.settings.model),
            messages,
            max_tokens: self.settings.api.max_tokens,
            stream: false,
            temperature: 0.7,
        };

        let url = format!("{}/v1/chat/completions", self.get_base_url());
        
        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("API error ({}): {}", status, body));
        }

        let chat_response: ChatResponse = response.json().await?;
        Ok(chat_response)
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
    ) -> anyhow::Result<reqwest::Response> {
        let api_key = self.get_api_key()
            .ok_or_else(|| anyhow::anyhow!("API key not configured"))?;

        let request = ChatRequest {
            model: self.settings.api.get_model_id(&self.settings.model),
            messages,
            max_tokens: self.settings.api.max_tokens,
            stream: true,
            temperature: 0.7,
        };

        let url = format!("{}/v1/chat/completions", self.get_base_url());
        
        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        Ok(response)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_calls: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: usize,
    stream: bool,
    temperature: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    pub index: i32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Delta {
    pub role: Option<String>,
    pub content: Option<String>,
}

pub type AnthropicClient = ApiClient;
