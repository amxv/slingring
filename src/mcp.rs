//! Stateless JSON-RPC MCP. Image paths are deliberately absent at this boundary.
use crate::{config::Config, model::NewTask, queue::Queue};
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
        "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
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
                "instructions":"Use new_task to start ChatGPT chats. This tool returns a conversation URL, not an answer. Never automatically retry an uncertain submission."})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":[tool()]}),
        "tools/call" => return None,
        _ => return Some(Some(error(id, -32601, "Method not found"))),
    };
    Some(Some(result(id, r)))
}
pub fn arguments(request: &Value) -> Result<NewTask, &'static str> {
    if request["params"]["name"] != "new_task" {
        return Err("Unknown tool");
    }
    let args = request["params"]["arguments"]
        .as_object()
        .ok_or("arguments must be an object")?;
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
    })
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
    let outcome = match queue.submit(key.clone(), task, config).await {
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
