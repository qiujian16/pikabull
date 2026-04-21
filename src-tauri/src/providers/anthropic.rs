use async_trait::async_trait;
use log::{debug, error, info};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{LLMProvider, LLMResponse, Message, ToolCall};

pub struct AnthropicProvider {
    model: String,
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(model: String, api_key: String) -> Self {
        Self {
            model,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn tools_to_anthropic(tools: &[Value]) -> Vec<Value> {
        tools
            .iter()
            .filter_map(|t| {
                let func = t.get("function")?;
                Some(json!({
                    "name": func.get("name")?,
                    "description": func.get("description").unwrap_or(&json!("")),
                    "input_schema": func.get("parameters").unwrap_or(&json!({})),
                }))
            })
            .collect()
    }

    fn messages_to_anthropic(messages: &[Message]) -> Vec<Value> {
        let mut result: Vec<Value> = Vec::new();
        let mut i = 0;
        while i < messages.len() {
            let msg = &messages[i];
            match msg.role.as_str() {
                "user" => {
                    result.push(json!({
                        "role": "user",
                        "content": msg.content.as_deref().unwrap_or(""),
                    }));
                    i += 1;
                }
                "assistant" => {
                    if let Some(tool_calls) = &msg.tool_calls {
                        let mut blocks: Vec<Value> = Vec::new();
                        if let Some(text) = &msg.content {
                            if !text.is_empty() {
                                blocks.push(json!({"type": "text", "text": text}));
                            }
                        }
                        for tc in tool_calls {
                            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            let name = tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let args_str = tc
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");
                            let input: Value =
                                serde_json::from_str(args_str).unwrap_or(json!({}));
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input,
                            }));
                        }
                        result.push(json!({"role": "assistant", "content": blocks}));
                    } else {
                        result.push(json!({
                            "role": "assistant",
                            "content": msg.content.as_deref().unwrap_or(""),
                        }));
                    }
                    i += 1;
                }
                "tool" => {
                    let mut tool_results: Vec<Value> = Vec::new();
                    while i < messages.len() && messages[i].role == "tool" {
                        let tr = &messages[i];
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tr.tool_call_id.as_deref().unwrap_or(""),
                            "content": tr.content.as_deref().unwrap_or(""),
                        }));
                        i += 1;
                    }
                    result.push(json!({"role": "user", "content": tool_results}));
                }
                _ => {
                    i += 1;
                }
            }
        }
        result
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Value],
        max_tokens: u32,
    ) -> Result<LLMResponse, String> {
        let anthropic_messages = Self::messages_to_anthropic(messages);
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": system,
            "messages": anthropic_messages,
        });
        if !tools.is_empty() {
            body["tools"] = json!(Self::tools_to_anthropic(tools));
        }

        info!("[llm] Anthropic complete: model={}, messages={}, tools={}", self.model, anthropic_messages.len(), tools.len());
        debug!("[llm] Anthropic request body: {}", serde_json::to_string(&body).unwrap_or_default());

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                error!("[llm] Anthropic request failed: {e}");
                format!("Anthropic request failed: {e}")
            })?;

        let status = resp.status();
        info!("[llm] Anthropic response status: {}", status);
        let resp_body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Anthropic response: {e}"))?;

        if !status.is_success() {
            error!("[llm] Anthropic API error {}: {}", status, resp_body);
            return Err(format!(
                "Anthropic API error {}: {}",
                status,
                resp_body.get("error").unwrap_or(&resp_body)
            ));
        }

        let usage = resp_body.get("usage");
        if let Some(u) = usage {
            info!("[llm] Anthropic usage: input_tokens={}, output_tokens={}",
                u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0));
        }

        let mut text = String::new();
        let mut tool_calls = Vec::new();

        if let Some(content) = resp_body.get("content").and_then(|c| c.as_array()) {
            for block in content {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text.push_str(t);
                        }
                    }
                    Some("tool_use") => {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = block.get("input").cloned().unwrap_or(json!({}));
                        tool_calls.push(ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {}
                }
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
        let anthropic_messages = Self::messages_to_anthropic(messages);
        let body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": system,
            "messages": anthropic_messages,
            "stream": true,
        });

        info!("[llm] Anthropic stream: model={}, messages={}", self.model, anthropic_messages.len());

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                error!("[llm] Anthropic stream request failed: {e}");
                format!("Anthropic stream request failed: {e}")
            })?;

        info!("[llm] Anthropic stream response status: {}", resp.status());
        if !resp.status().is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            error!("[llm] Anthropic stream API error: {body}");
            return Err(format!("Anthropic API error: {body}"));
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
                        if event.get("type").and_then(|t| t.as_str())
                            == Some("content_block_delta")
                        {
                            if let Some(text) = event
                                .get("delta")
                                .and_then(|d| d.get("text"))
                                .and_then(|t| t.as_str())
                            {
                                let _ = tx.send(text.to_string()).await;
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

}
