//! Official MCP Rust SDK stdio transport. The existing relay's JSON-RPC
//! envelope stays wire-compatible and still dispatches through `mcp`.
use crate::{config::Config, daemon, mcp, model::MAX_JSON, service};
use anyhow::Result;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
    transport::async_rw::AsyncRwTransport,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct LocalServer {
    root: PathBuf,
}

fn tools() -> Vec<Tool> {
    mcp::tools()
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("valid MCP tool schema"))
        .collect()
}

impl ServerHandler for LocalServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("slingring", env!("CARGO_PKG_VERSION")))
            .with_instructions("Use new_task to start a ChatGPT Chat conversation and send_message to continue an existing chat. Both submit through your account and return the chat URL, never generated answer text. Do not retry uncertain submissions. Files are available through the local CLI only.")
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().into_iter().find(|tool| tool.name == name)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        if request.request_state.is_some() || request.input_responses.is_some() {
            return Err(ErrorData::invalid_params(
                "Multi-round-trip calls are unsupported",
                None,
            ));
        }
        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call", "params":{
            "name": request.name, "arguments": request.arguments.unwrap_or_default()}});
        let task =
            mcp::arguments(&body).map_err(|message| ErrorData::invalid_params(message, None))?;
        let config = Config::load(&self.root)
            .map_err(|_| ErrorData::internal_error("Slingring configuration unavailable", None))?;
        task.resolve(&config)
            .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
        service::start(&self.root, true).await.map_err(|_| {
            ErrorData::internal_error("Failed to start local Slingring service", None)
        })?;
        let response = daemon::call(
            &self.root,
            &daemon::Request::Mcp {
                body,
                key: uuid::Uuid::new_v4().to_string(),
            },
        )
        .await
        .map_err(|_| ErrorData::internal_error("Local Slingring service unavailable", None))?;
        let response = response
            .get("response")
            .ok_or_else(|| ErrorData::internal_error("Missing local MCP response", None))?;
        if let Some(error) = response.get("error") {
            let code = error["code"].as_i64().unwrap_or(-32603);
            let message = error["message"].as_str().unwrap_or("Invalid tool request");
            return Err(if code == -32602 {
                ErrorData::invalid_params(message.to_owned(), None)
            } else {
                ErrorData::internal_error(message.to_owned(), None)
            });
        }
        let result: Value = response
            .get("result")
            .cloned()
            .ok_or_else(|| ErrorData::internal_error("Missing local MCP result", None))?;
        let result: CallToolResult = serde_json::from_value(result)
            .map_err(|_| ErrorData::internal_error("Invalid local MCP result", None))?;
        Ok(result.into())
    }
}

pub async fn stdio_server(root: &Path) -> Result<()> {
    // Preserve Slingring's 256 KiB request limit instead of allowing the SDK's
    // default 16 MiB incoming stdio frames. The SDK bounds and discards an
    // oversized frame without accumulating it in memory.
    let transport = AsyncRwTransport::new_server(tokio::io::stdin(), tokio::io::stdout())
        .with_max_line_length(MAX_JSON);
    let server = LocalServer { root: root.into() }.serve(transport).await?;
    server.waiting().await?;
    Ok(())
}
