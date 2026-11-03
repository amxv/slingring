//! Stateless JSON-RPC MCP. Image paths are deliberately absent at this boundary.
use crate::{
    config::Config,
    model::{NewTask, SendMessage, Task},
    queue::Queue,
};
use serde_json::{Value, json};
use std::path::Path;

pub const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];
pub fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
pub fn tool() -> Value {
    json!({"name":"new_task","description":"Start a new ChatGPT conversation in Chat mode, never Work. Return its URL after genuine thinking or response activity starts. No generated answer is returned. Omit or leave reasoning_effort blank to use the configured default. An uncertain result must not be blindly retried.",
        "inputSchema":{"type":"object","properties":{
            "prompt":{"type":"string","minLength":1,"maxLength":131072},
            "reasoning_effort":{"type":"string","enum":["","Instant","Medium","High","Extra High","Pro"]}},
            "required":["prompt"],"additionalProperties":false},
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
}
pub fn message_tool() -> Value {
    json!({"name":"send_message","description":"Send a follow-up to an existing ChatGPT Chat conversation. Accept only its exact https://chatgpt.com/c/<id> URL. Wait for voice-mode readiness without stopping an existing response, preserve the chat's effort, send once, and return the same URL after new thinking or response activity starts. No generated answer is returned. An uncertain result must not be blindly retried.",
        "inputSchema":{"type":"object","properties":{
            "chatgpt_url":{"type":"string","minLength":23,"maxLength":150,
                "pattern":"^https://chatgpt\\.com/c/[A-Za-z0-9-]{1,128}$"},
            "message":{"type":"string","minLength":1,"maxLength":131072}},
            "required":["chatgpt_url","message"],"additionalProperties":false},
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
}
pub fn tools() -> Vec<Value> {
    vec![tool(), message_tool()]
}
/// Outer None means this is a write request; Some(None) means a notification.
pub fn metadata(request: &Value) -> Option<Option<Value>> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !request.is_object()
        || request["jsonrpc"] != "2.0"
        || !request["method"].is_string()
        || !(id.is_null() || id.is_string() || id.is_i64() || id.is_u64())
    {
        return Some(Some(error(id, -32600, "Invalid Request")));
    }
    if request.get("id").is_none() {
        return Some(None);
    } // A notification never performs a write.
    let r = match request["method"].as_str().unwrap_or("") {
        "initialize" => {
            let requested = request["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = if VERSIONS.contains(&requested) {
                requested
            } else {
                "2025-11-25"
            };
            json!({"protocolVersion":version,"capabilities":{"tools":{}},
                "serverInfo":{"name":"slingring","version":env!("CARGO_PKG_VERSION")},
                "instructions":"Use new_task to start ChatGPT chats and send_message to follow up in an existing Chat. Both tools submit prompts through the connected account and return a conversation URL, not an answer. Never automatically retry an uncertain submission."})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":tools()}),
        "tools/call" => return None,
        _ => return Some(Some(error(id, -32601, "Method not found"))),
    };
    Some(Some(result(id, r)))
}
pub fn arguments(request: &Value) -> Result<Task, &'static str> {
    let name = request["params"]["name"].as_str().unwrap_or("");
    if !matches!(name, "new_task" | "send_message") {
        return Err("Unknown tool");
    }
    let args = request["params"]["arguments"]
        .as_object()
        .ok_or("arguments must be an object")?;
    if name == "send_message" {
        if args.keys().any(|k| k != "chatgpt_url" && k != "message") {
            return Err("Only chatgpt_url and message are accepted");
        }
        let chatgpt_url = args
            .get("chatgpt_url")
            .and_then(Value::as_str)
            .ok_or("chatgpt_url must be a string")?;
        let message = args
            .get("message")
            .and_then(Value::as_str)
            .ok_or("message must be a string")?;
        return Ok(SendMessage {
            chatgpt_url: chatgpt_url.into(),
            message: message.into(),
            image_paths: Vec::new(),
        }
        .into());
    }
    if args
        .keys()
        .any(|k| k != "prompt" && k != "reasoning_effort")
    {
        return Err("Only prompt and reasoning_effort are accepted; local files are CLI-only");
    }
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or("prompt must be a string")?;
    let reasoning_effort = match args.get("reasoning_effort") {
        None => None,
        Some(Value::String(v)) => Some(v.clone()),
        _ => return Err("reasoning_effort must be a string"),
    };
    Ok(NewTask {
        prompt: prompt.into(),
        reasoning_effort,
        image_paths: Vec::new(),
    }
    .into())
}
pub async fn dispatch(
    root: &Path,
    queue: &Queue,
    request: Value,
    key: String,
    deadline_ms: Option<u64>,
) -> Option<Value> {
    if let Some(response) = metadata(&request) {
        return response;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let task = match arguments(&request) {
        Ok(task) => task,
        Err(message) => return Some(error(id, -32602, message)),
    };
    let mut config = match Config::load(root) {
        Ok(c) => c,
        Err(_) => return Some(error(id, -32603, "Service configuration unavailable")),
    };
    if let Some(deadline) = deadline_ms {
        let seconds = deadline.saturating_sub(crate::queue::now()) / 1000;
        if seconds == 0 {
            return Some(error(
                id,
                -32000,
                "Request expired before launch; nothing was submitted",
            ));
        }
        config.start_timeout_secs = config.start_timeout_secs.min(seconds);
    }
    let task = match task.resolve(&config) {
        Ok(task) => task,
        Err(e) => return Some(error(id, -32602, &e.to_string())),
    };
    let external_deadline = deadline_ms.map(|deadline| {
        tokio::time::Instant::now()
            + std::time::Duration::from_millis(deadline.saturating_sub(crate::queue::now()))
    });
    let outcome = match queue
        .submit_before(key.clone(), task, config, external_deadline)
        .await
    {
        Ok(r) => {
            json!({"content":[{"type":"text","text":r.url}],"structuredContent":r,"isError":false})
        }
        Err(e) => {
            let record = queue.snapshot().into_iter().find(|j| j["id"] == key);
            let details = json!({"job_id":key,"error":e.to_string(),"job":record});
            json!({"content":[{"type":"text","text":details.to_string()}],"structuredContent":details,"isError":true})
        }
    };
    Some(result(id, outcome))
}
