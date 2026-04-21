use log::{debug, info};
use serde_json::json;
use tokio::sync::mpsc;

use crate::providers::{LLMProvider, Message};
use crate::skills::execute_tool;

pub async fn run_agent_streaming(
    provider: &dyn LLMProvider,
    system_prompt: &str,
    user_message: &str,
    tools: &[serde_json::Value],
    tx: &mpsc::Sender<String>,
) -> Result<String, String> {
    let mut messages: Vec<Message> = vec![Message {
        role: "user".into(),
        content: Some(user_message.into()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];

    let mut had_tool_calls = false;

    loop {
        let response = provider
            .complete(system_prompt, &messages, tools, 4096)
            .await?;

        if !response.has_tool_calls() {
            if had_tool_calls {
                // Tools were used — stream the final report via stream_complete
                // for true token-by-token streaming. Drop the non-streamed text
                // from this complete() call and redo it as a stream.
                let mut rx = provider
                    .stream_complete(system_prompt, &messages, 4096)
                    .await?;
                let mut full_text = String::new();
                while let Some(chunk) = rx.recv().await {
                    full_text.push_str(&chunk);
                    let _ = tx.send(chunk).await;
                }
                return Ok(full_text);
            }
            // No tools were ever called — stream the already-available text
            // in small chunks for a progressive UI.
            let chars: Vec<char> = response.content.chars().collect();
            for chunk in chars.chunks(30) {
                let text: String = chunk.iter().collect();
                let _ = tx.send(text).await;
            }
            return Ok(response.content);
        }

        had_tool_calls = true;

        // Emit tool-call status
        for tc in &response.tool_calls {
            let _ = tx.send(format!("🔧 调用 {}...\n", tc.name)).await;
        }

        append_tool_cycle(&mut messages, &response).await?;

        // Emit tool-done status
        for tc in &response.tool_calls {
            let _ = tx.send(format!("✅ {} 完成\n", tc.name)).await;
        }
        let _ = tx.send("📝 正在撰写报告...\n\n".to_string()).await;
    }
}

async fn append_tool_cycle(
    messages: &mut Vec<Message>,
    response: &crate::providers::LLMResponse,
) -> Result<(), String> {
    let tool_calls_json: Vec<serde_json::Value> = response
        .tool_calls
        .iter()
        .map(|tc| {
            json!({
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.name,
                    "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                }
            })
        })
        .collect();

    messages.push(Message {
        role: "assistant".into(),
        content: if response.content.is_empty() {
            None
        } else {
            Some(response.content.clone())
        },
        tool_calls: Some(tool_calls_json),
        tool_call_id: None,
        name: None,
    });

    let tool_calls = response.tool_calls.clone();
    let mut handles = Vec::new();
    for tc in &tool_calls {
        info!("[tool] Executing: {} args={}", tc.name, tc.arguments);
        let name = tc.name.clone();
        let args = tc.arguments.clone();
        handles.push(tokio::task::spawn_blocking(move || {
            execute_tool(&name, &args)
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        let result = handle
            .await
            .map_err(|e| format!("Tool execution error: {e}"))?;
        results.push(result);
    }

    for (tc, result) in tool_calls.iter().zip(results) {
        debug!("[tool] Result for {}: {} chars", tc.name, result.len());
        messages.push(Message {
            role: "tool".into(),
            content: Some(result),
            tool_calls: None,
            tool_call_id: Some(tc.id.clone()),
            name: Some(tc.name.clone()),
        });
    }

    Ok(())
}

pub async fn run_agent_stream(
    provider: &dyn LLMProvider,
    system_prompt: &str,
    user_message: &str,
) -> Result<mpsc::Receiver<String>, String> {
    let messages = vec![Message {
        role: "user".into(),
        content: Some(user_message.into()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];

    provider
        .stream_complete(system_prompt, &messages, 4096)
        .await
}
