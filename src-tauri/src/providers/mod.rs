pub mod anthropic;
pub mod openai;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

impl LLMResponse {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[serde_json::Value],
        max_tokens: u32,
    ) -> Result<LLMResponse, String>;

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
    ) -> Result<tokio::sync::mpsc::Receiver<String>, String>;

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub provider: String,
    pub model: String,
}

pub fn create_provider_from_env() -> Result<Box<dyn LLMProvider>, String> {
    let _ = dotenvy::dotenv();
    let provider = env::var("LLM_PROVIDER").unwrap_or_else(|_| "anthropic".into());

    match provider.to_lowercase().as_str() {
        "anthropic" => {
            let model = env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".into());
            let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| "ANTHROPIC_API_KEY not set")?;
            Ok(Box::new(anthropic::AnthropicProvider::new(model, api_key)))
        }
        "openai" => {
            let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".into());
            let api_key = env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set")?;
            let base_url = env::var("OPENAI_BASE_URL").ok();
            Ok(Box::new(openai::OpenAIProvider::new(model, api_key, base_url)))
        }
        "ollama" => {
            let model = env::var("OLLAMA_MODEL").unwrap_or_else(|_| "hermes3".into());
            let api_key = env::var("OLLAMA_API_KEY").unwrap_or_else(|_| "ollama".into());
            let base_url = env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".into());
            Ok(Box::new(openai::OpenAIProvider::new(
                model,
                api_key,
                Some(base_url),
            )))
        }
        "minimax" => {
            let model =
                env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-M2.7-highspeed".into());
            let api_key = env::var("MINIMAX_API_KEY").map_err(|_| "MINIMAX_API_KEY not set")?;
            let base_url = env::var("MINIMAX_BASE_URL")
                .unwrap_or_else(|_| "https://api.minimax.chat/v1".into());
            Ok(Box::new(openai::OpenAIProvider::new(
                model,
                api_key,
                Some(base_url),
            )))
        }
        other => Err(format!("Unknown LLM_PROVIDER: '{other}'")),
    }
}

pub fn get_provider_info() -> ProviderInfo {
    let _ = dotenvy::dotenv();
    let provider = env::var("LLM_PROVIDER").unwrap_or_else(|_| "anthropic".into());
    let model = match provider.as_str() {
        "anthropic" => env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".into()),
        "openai" => env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".into()),
        "ollama" => env::var("OLLAMA_MODEL").unwrap_or_else(|_| "hermes3".into()),
        "minimax" => {
            env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-M2.7-highspeed".into())
        }
        _ => "unknown".into(),
    };
    ProviderInfo { provider, model }
}

pub fn create_provider(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
) -> Result<Box<dyn LLMProvider>, String> {
    match provider {
        "anthropic" => {
            if api_key.is_empty() {
                return Err("Anthropic API key is required".into());
            }
            Ok(Box::new(anthropic::AnthropicProvider::new(
                model.to_string(),
                api_key.to_string(),
            )))
        }
        "openai" | "ollama" | "minimax" => {
            let url = if base_url.is_empty() {
                match provider {
                    "ollama" => Some("http://localhost:11434/v1".to_string()),
                    "minimax" => Some("https://api.minimax.chat/v1".to_string()),
                    _ => None,
                }
            } else {
                Some(base_url.to_string())
            };
            let key = if api_key.is_empty() {
                if provider == "ollama" {
                    "ollama".to_string()
                } else {
                    return Err(format!("{provider} API key is required"));
                }
            } else {
                api_key.to_string()
            };
            Ok(Box::new(openai::OpenAIProvider::new(
                model.to_string(),
                key,
                url,
            )))
        }
        other => Err(format!("Unknown provider: '{other}'")),
    }
}

pub fn create_active_provider() -> Result<Box<dyn LLMProvider>, String> {
    if let Some(config) = crate::config_store::get_active() {
        return create_provider(&config.provider, &config.model, &config.api_key, &config.base_url);
    }
    create_provider_from_env()
}
