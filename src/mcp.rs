//! Stateless JSON-RPC MCP. Image paths are deliberately absent at this boundary.
use crate::{
    config::Config,
    model::{NewTask, SendMessage, Task},
    queue::{self, Queue},
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
    json!({"name":"new_task","description":"Start a new ChatGPT Chat conversation and return its URL when real activity begins. Provide a fresh unique request_id BEFORE sending so you can query get_job_status if the MCP caller times out. Never automatically retry an uncertain submission. Omitted effort uses the configured default.",
        "inputSchema":{"type":"object","properties":{
            "prompt":{"type":"string","minLength":1,"maxLength":131072},
            "reasoning_effort":{"type":"string","enum":["","Instant","Medium","High","Extra High","Pro"]},
            "request_id":request_id_schema()},
            "required":["prompt"],"additionalProperties":false},
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
}
pub fn message_tool() -> Value {
    json!({"name":"send_message","description":"Send a follow-up to an existing ChatGPT Chat conversation. Supply a fresh unique request_id before submitting so get_job_status can recover the outcome after a caller timeout without resending. Preserve the chat's effort, do not interrupt a response and never automatically retry uncertain submissions.",
        "inputSchema":{"type":"object","properties":{
            "chatgpt_url":{"type":"string","minLength":23,"maxLength":150,
                "pattern":"^https://chatgpt\\.com/c/[A-Za-z0-9-]{1,128}$"},
            "message":{"type":"string","minLength":1,"maxLength":131072},
            "request_id":request_id_schema()},
            "required":["chatgpt_url","message"],"additionalProperties":false},
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
}
fn request_id_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":100,"pattern":"^[A-Za-z0-9_-]+$"})
}
pub fn status_tool() -> Value {
    json!({"name":"get_job_status","description":"Read the Mac's local task journal without opening a browser or submitting anything. Pass the request_id you supplied with new_task/send_message or the Idempotency-Key HTTP header. A missing/expired record is UNKNOWN, not proof that nothing was submitted. Never blindly retry an uncertain job.",
        "inputSchema":{"type":"object","properties":{"request_id":request_id_schema()},"required":["request_id"],"additionalProperties":false},
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
}
pub fn tools() -> Vec<Value> {
    vec![tool(), message_tool(), status_tool()]
}
pub fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
pub fn declared_request_id(request: &Value) -> Result<Option<&str>, &'static str> {
    let Some(id) = request["params"]["arguments"].get("request_id") else {
        return Ok(None);
    };
    let id = id.as_str().ok_or("request_id must be a string")?;
    if !valid_request_id(id) {
        return Err("request_id must be 1..100 ASCII letters, digits, underscore or hyphen");
    }
    Ok(Some(id))
}
pub fn status_request(request: &Value) -> bool {
    request["method"] == "tools/call" && request["params"]["name"] == "get_job_status"
}
/// This is a read-only lookup of local metadata; no task is admitted to the queue.
pub fn status_response(root: &Path, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let args = request["params"]["arguments"].as_object();
    let Some(args) = args else {
        return error(id, -32602, "arguments must be an object");
    };
    if args.len() != 1 || !args.contains_key("request_id") {
        return error(id, -32602, "Only request_id is accepted");
    }
    let request_id = match declared_request_id(request) {
        Ok(Some(id)) => id,
        Ok(None) | Err(_) => return error(id, -32602, "invalid request_id"),
    };
    let jobs = match queue::records(root) {
        Ok(jobs) => jobs,
        Err(_) => return error(id, -32603, "Private task journal unavailable"),
    };
    let status = queue::recovery_status(jobs.iter().find(|job| job.id == request_id), request_id);
    result(
        id,
        json!({"content":[{"type":"text","text":status.to_string()}],
        "structuredContent":status,"isError":false}),
    )
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
                "instructions":"Use new_task and send_message with a fresh unique request_id for each logical write. If the caller times out, use read-only get_job_status(request_id) to inspect the Mac's journal; never resubmit an uncertain operation. No generated answer is returned."})
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
        if args
            .keys()
            .any(|k| k != "chatgpt_url" && k != "message" && k != "request_id")
        {
            return Err("Only chatgpt_url, message and request_id are accepted");
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
        .any(|k| k != "prompt" && k != "reasoning_effort" && k != "request_id")
    {
        return Err(
            "Only prompt, reasoning_effort and request_id are accepted; local files are CLI-only",
        );
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
    if status_request(&request) {
        return Some(status_response(root, &request));
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let requested_key = match declared_request_id(&request) {
        Ok(value) => value,
        Err(message) => return Some(error(id, -32602, message)),
    };
    if let Some(requested_key) = requested_key
        && key != requested_key
    {
        return Some(error(
            id,
            -32602,
            "request_id does not match the relay's Idempotency-Key",
        ));
    }
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
            if e.to_string()
                .contains("request ID was already used with different input")
            {
                let details = json!({"job_id":key,"outcome":"rejected","isError":true,
                    "error":"request ID was already used for a different task",
                    "guidance":"This ID belongs to another request. It did NOT admit this payload; do not confuse the earlier job with this attempt. Choose a fresh ID only for an intentionally distinct request."});
                return Some(result(
                    id,
                    json!({"content":[{"type":"text","text":details.to_string()}],
                    "structuredContent":details,"isError":true}),
                ));
            }
            let record = queue.snapshot().into_iter().find(|j| j["id"] == key);
            let recovered = queue::records(root).ok().and_then(|jobs| {
                jobs.iter()
                    .find(|job| job.id == key)
                    .map(|job| queue::recovery_status(Some(job), &key))
            });
            let details = json!({"job_id":key,"error":e.to_string(),"job":record,
                "outcome":recovered.as_ref().map(|r|r["outcome"].clone()).unwrap_or(json!("unknown")),
                "guidance":recovered.as_ref().map(|r|r["guidance"].clone()).unwrap_or(json!("The request may not have reached the Mac. Do not assume it failed; query get_job_status using this request ID.")),
                "diagnosis":recovered.as_ref().and_then(|r|r.get("diagnosis"))});
            json!({"content":[{"type":"text","text":details.to_string()}],"structuredContent":details,"isError":true})
        }
    };
    Some(result(id, outcome))
}
