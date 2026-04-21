use async_trait::async_trait;
use log::{debug, error, info};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{LLMProvider, LLMResponse, Message, ToolCall};

pub struct OpenAIProvider {
    model: String,
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new(model: String, api_key: String, base_url: Option<String>) -> Self {
        Self {
            model,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".into()),
            client: reqwest::Client::new(),
        }
    }

    fn messages_to_openai(system: &str, messages: &[Message]) -> Vec<Value> {
        let mut result = vec![json!({"role": "system", "content": system})];
        for msg in messages {
            let mut m = json!({"role": msg.role, "content": msg.content});
            if let Some(tcs) = &msg.tool_calls {
                m["tool_calls"] = json!(tcs);
            }
            if let Some(id) = &msg.tool_call_id {
                m["tool_call_id"] = json!(id);
            }
            if let Some(name) = &msg.name {
                m["name"] = json!(name);
            }
            result.push(m);
        }
        result
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Value],
        max_tokens: u32,
    ) -> Result<LLMResponse, String> {
        let all_messages = Self::messages_to_openai(system, messages);
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": all_messages,
        });
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        info!("[llm] OpenAI complete: model={}, messages={}, tools={}", self.model, all_messages.len(), tools.len());
        debug!("[llm] OpenAI request body: {}", serde_json::to_string(&body).unwrap_or_default());

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                error!("[llm] OpenAI request failed: {e}");
                format!("OpenAI request failed: {e}")
            })?;

        let status = resp.status();
        info!("[llm] OpenAI response status: {}", status);
        let resp_body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse OpenAI response: {e}"))?;

        if !status.is_success() {
            error!("[llm] OpenAI API error {}: {}", status, resp_body);
            return Err(format!(
                "OpenAI API error {}: {}",
                status,
                resp_body.get("error").unwrap_or(&resp_body)
            ));
        }

        let usage = resp_body.get("usage");
        if let Some(u) = usage {
            info!("[llm] OpenAI usage: prompt_tokens={}, completion_tokens={}, total={}",
                u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0));
        }

        let choice = &resp_body["choices"][0]["message"];
        let text = choice
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let mut tool_calls = Vec::new();
        if let Some(tcs) = choice.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tcs {
                let id = tc
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args_str = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }

        Ok(LLMResponse {
            content: text,
            tool_calls,
        })
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
    ) -> Result<mpsc::Receiver<String>, String> {
        let all_messages = Self::messages_to_openai(system, messages);
        let body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": all_messages,
            "stream": true,
        });

        info!("[llm] OpenAI stream: model={}, messages={}", self.model, all_messages.len());

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                error!("[llm] OpenAI stream request failed: {e}");
                format!("OpenAI stream request failed: {e}")
            })?;

        info!("[llm] OpenAI stream response status: {}", resp.status());
        if !resp.status().is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            error!("[llm] OpenAI stream API error: {body}");
            return Err(format!("OpenAI API error: {body}"));
        }

        let (tx, rx) = mpsc::channel::<String>(256);
        let mut stream = resp.bytes_stream();

        tokio::spawn(async move {
            use futures::StreamExt;
            let mut buffer = String::new();

            while let Some(chunk_result) = stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(_) => break,
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if !line.starts_with("data: ") {
                        continue;
                    }
                    let data = &line[6..];
                    if data == "[DONE]" {
                        return;
                    }
                    if let Ok(event) = serde_json::from_str::<Value>(data) {
                        if let Some(content) = event
                            .get("choices")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("delta"))
                            .and_then(|d| d.get("content"))
                            .and_then(|c| c.as_str())
                        {
                            let _ = tx.send(content.to_string()).await;
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

}
