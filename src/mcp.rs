//! Stateless JSON-RPC MCP. Image paths are deliberately absent at this boundary.
use crate::{
    config::Config,
    model::{NewTask, PauseTask, SendMessage, Task},
    queue::{self, Queue},
};
use serde_json::{Value, json};
use std::path::Path;

/// Keep the tool response comfortably below common caller/proxy timeouts. The
/// browser budget remains independent and accepted jobs continue in memory.
pub const RECEIPT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

pub const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];
pub fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
pub fn tool() -> Value {
    json!({"name":"new_task","description":"Submit a new ChatGPT Chat task once. Within about five seconds of admission, return either a confirmed activity URL or an accepted in_progress receipt. Accepted jobs continue after this call returns; use get_job_status(request_id) for the outcome, never submit again to poll. Provide a fresh unique request_id BEFORE sending so a caller timeout cannot lose the recovery key. Never automatically retry uncertain submissions. Omitted effort uses the configured default.",
        "inputSchema":{"type":"object","properties":{
            "prompt":{"description":"Task instructions to submit once in a new ChatGPT Chat conversation. Generated answers are never returned.","type":"string","minLength":1,"maxLength":131072},
            "reasoning_effort":{"description":"Requested effort. Omit or use an empty string to use the configured default; new chats require verified selection.","type":"string","enum":["","Instant","Medium","High","Extra High","Pro"]},
            "request_id":request_id_schema()},
            "required":["prompt"],"additionalProperties":false},
        "outputSchema":output_schema(),
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
}
pub fn message_tool() -> Value {
    json!({"name":"send_message","description":"Submit a follow-up once to an exact existing ChatGPT Chat URL, without visiting the homepage. Optional reasoning_effort selects Instant, Medium, High, Extra High or Pro on a best-effort basis. If omitted, leave the chat's reasoning setting untouched: no selector reads or changes. A missing/rehydrating effort UI must not block submission; a requested level may remain unverified if unavailable, but this never changes the conversation target. Within about five seconds of admission, return either a confirmed activity URL or an accepted in_progress receipt. Accepted jobs continue after this call returns; use get_job_status(request_id), never resubmit to poll. Supply a fresh unique request_id before sending for timeout recovery. Wait for idle voice readiness, do not interrupt a response and never automatically retry uncertain submissions.",
        "inputSchema":{"type":"object","properties":{
            "chatgpt_url":{"description":"Exact canonical existing conversation URL, with no query, fragment or trailing slash.","type":"string","minLength":23,"maxLength":150,
                "pattern":"^https://chatgpt\\.com/c/[A-Za-z0-9-]{1,128}$"},
            "message":{"description":"Follow-up text to submit once to the exact existing conversation.","type":"string","minLength":1,"maxLength":131072},
            "reasoning_effort":{"description":"Optional best-effort selection; omit or use an empty string to leave the current setting untouched. Check requested_effort, verified_effort and effort_verification in the result.","type":"string","enum":["","Instant","Medium","High","Extra High","Pro"]},
            "request_id":request_id_schema()},
            "required":["chatgpt_url","message"],"additionalProperties":false},
        "outputSchema":output_schema(),
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
}
fn request_id_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_:-]+$","description":"Stable recovery key: 1..128 ASCII letters, digits, underscore, hyphen or colon. Choose a fresh unique value before each logical write; reuse it only for get_job_status, never resubmit to poll."})
}
pub fn status_tool() -> Value {
    json!({"name":"get_job_status","description":"Read the Mac's local task journal without opening a browser or submitting anything. Pass the request_id you supplied with new_task/send_message or the Idempotency-Key HTTP header. A missing/expired record is UNKNOWN, not proof that nothing was submitted. Never blindly retry an uncertain job.",
        "inputSchema":{"type":"object","properties":{"request_id":request_id_schema()},"required":["request_id"],"additionalProperties":false},
        "outputSchema":output_schema(),
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
}
pub fn tools() -> Vec<Value> {
    vec![tool(), message_tool(), status_tool(), pause_tool()]
}
pub fn pause_tool() -> Value {
    json!({"name":"pause_task","description":"Attempt to stop the current response in one exact ChatGPT conversation using its UI. Verify idle using stable completion controls on the latest assistant turn, or its exact Stopped thinking UI status with adjacent Dictate and Voice controls. Missing Stop or a voice icon alone never proves completion. No generated response text is read or returned. Returns stopped or already_idle when verified, otherwise explicit uncertainty. Within about five seconds of admission a pending operation returns a request_id for read-only get_job_status; never call pause_task again to poll. Do not automatically retry an uncertain pause.",
        "inputSchema":{"type":"object","properties":{
            "chatgpt_url":{"type":"string","minLength":23,"maxLength":150,
                "pattern":"^https://chatgpt\\.com/c/[A-Za-z0-9-]{1,128}$",
                "description":"Exact canonical conversation URL to pause, with no query, fragment or trailing slash."}},
            "required":["chatgpt_url"],"additionalProperties":false},
        "outputSchema":output_schema(),
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}})
}
pub fn tools_for_config(config: &Config) -> Vec<Value> {
    let mut tools = tools();
    for tool in &mut tools {
        if let Some(values) =
            tool["inputSchema"]["properties"]["reasoning_effort"]["enum"].as_array_mut()
        {
            values.retain(|value| {
                value
                    .as_str()
                    .is_some_and(|effort| effort.is_empty() || config.subscription.allows(effort))
            });
        }
    }
    tools
}
pub fn valid_request_id(id: &str) -> bool {
    queue::valid_job_id(id)
}
pub fn declared_request_id(request: &Value) -> Result<Option<&str>, &'static str> {
    let Some(id) = request["params"]["arguments"].get("request_id") else {
        return Ok(None);
    };
    let id = id.as_str().ok_or("request_id must be a string")?;
    if !valid_request_id(id) {
        return Err("request_id must be 1..128 ASCII letters, digits, underscore, hyphen or colon");
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
    let jobs = match queue::recovery_records(root) {
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
                "instructions":"Use new_task and send_message once with a fresh unique request_id for each logical write. The call returns a confirmed URL or an accepted in_progress receipt within about five seconds of admission; accepted jobs continue in memory after the response or disconnect. Use read-only get_job_status(request_id) to recover the outcome, including after a caller timeout. Never resubmit to poll or replay uncertain operations. No generated answer is returned."})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":tools()}),
        "tools/call" => return None,
        _ => return Some(Some(error(id, -32601, "Method not found"))),
    };
    Some(Some(result(id, r)))
}
/// Control requests are completed outside the submission wait pool.
pub fn immediate_response(root: &Path, request: &Value) -> Option<Option<Value>> {
    if let Some(mut response) = metadata(request) {
        if request["method"] == "tools/list"
            && let Some(value) = response
                .as_mut()
                .filter(|value| value.get("result").is_some())
        {
            *value = match Config::load(root) {
                Ok(config) => result(
                    request["id"].clone(),
                    json!({"tools":tools_for_config(&config)}),
                ),
                Err(_) => error(
                    request["id"].clone(),
                    -32603,
                    "Service configuration unavailable",
                ),
            };
        }
        return Some(response);
    }
    status_request(request).then(|| Some(status_response(root, request)))
}
pub fn arguments(request: &Value) -> Result<Task, &'static str> {
    let name = request["params"]["name"].as_str().unwrap_or("");
    if !matches!(name, "new_task" | "send_message" | "pause_task") {
        return Err("Unknown tool");
    }
    let args = request["params"]["arguments"]
        .as_object()
        .ok_or("arguments must be an object")?;
    if name == "pause_task" {
        if args.len() != 1 || !args.contains_key("chatgpt_url") {
            return Err("Only chatgpt_url is accepted");
        }
        let url = args["chatgpt_url"]
            .as_str()
            .ok_or("chatgpt_url must be a string")?;
        return PauseTask {
            chatgpt_url: url.into(),
        }
        .resolve()
        .map(Task::from)
        .map_err(|_| "chatgpt_url must be an exact canonical conversation URL");
    }
    if name == "send_message" {
        if args.keys().any(|k| {
            k != "chatgpt_url" && k != "message" && k != "reasoning_effort" && k != "request_id"
        }) {
            return Err("Only chatgpt_url, message, reasoning_effort and request_id are accepted");
        }
        let chatgpt_url = args
            .get("chatgpt_url")
            .and_then(Value::as_str)
            .ok_or("chatgpt_url must be a string")?;
        let message = args
            .get("message")
            .and_then(Value::as_str)
            .ok_or("message must be a string")?;
        let reasoning_effort = match args.get("reasoning_effort") {
            None => None,
            Some(Value::String(value)) => Some(value.clone()),
            _ => return Err("reasoning_effort must be a string"),
        };
        return Ok(SendMessage {
            chatgpt_url: chatgpt_url.into(),
            message: message.into(),
            reasoning_effort,
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
    if let Some(response) = immediate_response(root, &request) {
        return response;
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
        // Let admission recover an existing ID before deciding a new expired
        // request is unsent. A past deadline cannot erase an earlier success.
        config.start_timeout_secs = config.start_timeout_secs.min(seconds.max(1));
    }
    let task = match task.resolve(&config) {
        Ok(task) => task,
        Err(e) => return Some(error(id, -32602, &e.to_string())),
    };
    let external_deadline = deadline_ms.map(|deadline| {
        tokio::time::Instant::now()
            + std::time::Duration::from_millis(deadline.saturating_sub(crate::queue::now()))
    });
    let is_pause = task.is_pause();
    let receiver = match queue.accept_before(key.clone(), task, config, external_deadline) {
        Ok(receiver) => receiver,
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
            let mut details = queue.recovery_status(&key);
            if details["known"] == false {
                details["outcome"] = json!(if is_pause {
                    "not_attempted"
                } else {
                    "not_submitted"
                });
                details["guidance"] = json!(if is_pause {
                    "Pause was rejected before queue admission. The conversation may still be generating."
                } else {
                    "This attempt was rejected before queue admission. Nothing was submitted by this attempt. Diagnose the error before intentionally creating a new request."
                });
            }
            details["job_id"] = json!(key);
            details["accepted"] = json!(false);
            details["error"] = json!(e.to_string());
            return Some(tool_result(id, details, true));
        }
    };
    match tokio::time::timeout(RECEIPT_WAIT, receiver).await {
        Ok(Ok(Ok(r))) => {
            let url = r.url.clone();
            let paused = r.pause_status.is_some();
            let mut details = serde_json::to_value(r).expect("serializable task result");
            details["request_id"] = json!(key);
            details["outcome"] = json!(if paused { "paused" } else { "submitted" });
            details["accepted"] = json!(true);
            Some(result(
                id,
                json!({"content":[{"type":"text","text":if paused { details.to_string() } else { url }}],
                "structuredContent":details,"isError":false}),
            ))
        }
        Ok(Ok(Err(error))) => {
            let mut details = queue.recovery_status(&key);
            details["job_id"] = json!(key);
            details["accepted"] = json!(true);
            details["error"] = json!(error.to_string());
            Some(tool_result(id, details, true))
        }
        outcome => {
            let mut details = queue.recovery_status(&key);
            details["job_id"] = json!(key);
            details["accepted"] = json!(true);
            details["recovery_tool"] = json!("get_job_status");
            // A dropped worker is not an in-progress receipt. Normal deadline
            // receipts leave the already-admitted job owned by the queue.
            if matches!(outcome, Ok(Err(_))) {
                details["outcome"] = json!("uncertain");
                details["guidance"] = json!(if is_pause {
                    "The queue stopped before verifying pause. The response may still be generating. Query the returned request_id; never retry automatically."
                } else {
                    "The queue stopped before replying. Query the same request_id; never resubmit automatically."
                });
            }
            Some(tool_result(id, details, false))
        }
    }
}
fn tool_result(id: Value, details: Value, is_error: bool) -> Value {
    result(
        id,
        json!({"content":[{"type":"text","text":details.to_string()}],
        "structuredContent":details,"isError":is_error}),
    )
}

/// Optional fields cover confirmed results, accepted receipts and recovery records.
/// Error results may add error/guidance fields without pretending a URL exists.
pub fn output_schema() -> Value {
    json!({"type":"object","properties":{
        "request_id":request_id_schema(), "job_id":{"type":"string"},
        "url":{"type":"string","description":"Canonical conversation URL when known; never generated answer text."},
        "accepted":{"type":"boolean"}, "known":{"type":"boolean"},
        "outcome":{"type":"string","description":"Recovery outcome; unknown is not evidence of no submission."},
        "reasoning_effort":{"type":["string","null"],"description":"Legacy verified effort field."},
        "requested_effort":{"type":["string","null"]},
        "verified_effort":{"type":["string","null"]},
        "effort_verification":{"type":"string","enum":["not_requested","verified","unverified"]},
        "pause_status":{"type":"string","enum":["stopped","already_idle"]},
        "recovery_tool":{"type":"string"}, "job":{"type":["object","null"]}
    },"additionalProperties":true})
}
