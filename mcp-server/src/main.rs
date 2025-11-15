use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use tracing::{debug, error, info};

mod client;
use client::StromClient;

/// JSON-RPC 2.0 Request
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// Tool call parameters
#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    arguments: Option<Value>,
}

/// MCP Server
struct McpServer {
    client: StromClient,
}

impl McpServer {
    fn new(api_url: String) -> Self {
        Self {
            client: StromClient::new(api_url),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&self) -> Value {
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "strom",
                "version": env!("CARGO_PKG_VERSION")
            }
        })
    }

    /// Handle tools/list request
    fn handle_list_tools(&self) -> Value {
        json!({
            "tools": [
                {
                    "name": "list_flows",
                    "description": "List all GStreamer flows",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "get_flow",
                    "description": "Get details of a specific flow by ID",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "flow_id": {
                                "type": "string",
                                "description": "The UUID of the flow"
                            }
                        },
                        "required": ["flow_id"]
                    }
                },
                {
                    "name": "create_flow",
                    "description": "Create a new flow",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Name for the new flow"
                            }
                        },
                        "required": ["name"]
                    }
                },
                {
                    "name": "delete_flow",
                    "description": "Delete a flow",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "flow_id": {
                                "type": "string",
                                "description": "The UUID of the flow to delete"
                            }
                        },
                        "required": ["flow_id"]
                    }
                },
                {
                    "name": "start_flow",
                    "description": "Start a flow's GStreamer pipeline",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "flow_id": {
                                "type": "string",
                                "description": "The UUID of the flow to start"
                            }
                        },
                        "required": ["flow_id"]
                    }
                },
                {
                    "name": "stop_flow",
                    "description": "Stop a running flow",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "flow_id": {
                                "type": "string",
                                "description": "The UUID of the flow to stop"
                            }
                        },
                        "required": ["flow_id"]
                    }
                },
                {
                    "name": "list_elements",
                    "description": "List available GStreamer elements, optionally filtered by category",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "category": {
                                "type": "string",
                                "description": "Optional category filter (e.g., 'source', 'codec', 'sink')"
                            }
                        },
                        "required": []
                    }
                },
                {
                    "name": "get_element_info",
                    "description": "Get detailed information about a specific GStreamer element",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "element_name": {
                                "type": "string",
                                "description": "Name of the GStreamer element (e.g., 'videotestsrc', 'x264enc')"
                            }
                        },
                        "required": ["element_name"]
                    }
                }
            ]
        })
    }

    /// Handle tools/call request
    async fn handle_call_tool(&self, params: Value) -> Result<Value> {
        let tool_params: ToolCallParams = serde_json::from_value(params)?;
        let args = tool_params.arguments.unwrap_or(json!({}));

        let result = match tool_params.name.as_str() {
            "list_flows" => {
                info!("MCP: Listing all flows");
                let flows = self.client.list_flows().await?;
                serde_json::to_value(&flows)?
            }
            "get_flow" => {
                let flow_id = args["flow_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("flow_id is required"))?;
                info!("MCP: Getting flow {}", flow_id);
                let flow = self.client.get_flow(flow_id).await?;
                serde_json::to_value(&flow)?
            }
            "create_flow" => {
                let name = args["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("name is required"))?
                    .to_string();
                info!("MCP: Creating flow '{}'", name);
                let request = strom_types::api::CreateFlowRequest { name };
                let flow = self.client.create_flow(request).await?;
                serde_json::to_value(&flow)?
            }
            "delete_flow" => {
                let flow_id = args["flow_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("flow_id is required"))?;
                info!("MCP: Deleting flow {}", flow_id);
                self.client.delete_flow(flow_id).await?;
                json!({ "success": true, "message": format!("Flow {} deleted", flow_id) })
            }
            "start_flow" => {
                let flow_id = args["flow_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("flow_id is required"))?;
                info!("MCP: Starting flow {}", flow_id);
                self.client.start_flow(flow_id).await?;
                json!({ "success": true, "message": format!("Flow {} started", flow_id) })
            }
            "stop_flow" => {
                let flow_id = args["flow_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("flow_id is required"))?;
                info!("MCP: Stopping flow {}", flow_id);
                self.client.stop_flow(flow_id).await?;
                json!({ "success": true, "message": format!("Flow {} stopped", flow_id) })
            }
            "list_elements" => {
                let category = args["category"].as_str().map(|s| s.to_string());
                info!("MCP: Listing elements (category: {:?})", category);
                let elements = self.client.list_elements().await?;
                let filtered: Vec<_> = if let Some(cat) = category {
                    elements
                        .into_iter()
                        .filter(|e| e.category.to_lowercase().contains(&cat.to_lowercase()))
                        .collect()
                } else {
                    elements
                };
                serde_json::to_value(&filtered)?
            }
            "get_element_info" => {
                let element_name = args["element_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("element_name is required"))?;
                info!("MCP: Getting info for element '{}'", element_name);
                let info = self.client.get_element_info(element_name).await?;
                serde_json::to_value(&info)?
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown tool: {}", tool_params.name));
            }
        };

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result)?
            }]
        }))
    }

    /// Handle a JSON-RPC request
    async fn handle_request(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.unwrap_or(Value::Null);

        debug!("Handling method: {}", req.method);

        let result = match req.method.as_str() {
            "initialize" => Ok(self.handle_initialize()),
            "initialized" => {
                // Notification, no response needed
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: None,
                };
            }
            "tools/list" => Ok(self.handle_list_tools()),
            "tools/call" => {
                match self
                    .handle_call_tool(req.params.unwrap_or(json!({})))
                    .await
                {
                    Ok(result) => Ok(result),
                    Err(e) => Err(JsonRpcError {
                        code: -32603,
                        message: format!("Tool call failed: {}", e),
                        data: None,
                    }),
                }
            }
            _ => Err(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
                data: None,
            }),
        };

        match result {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(error),
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr) // Log to stderr, not stdout (stdout is for JSON-RPC)
        .init();

    // Get Strom API URL from environment or use default
    let api_url =
        std::env::var("STROM_API_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    info!("Starting Strom MCP Server");
    info!("Connecting to Strom API at: {}", api_url);

    let server = McpServer::new(api_url);

    // Read from stdin and write to stdout
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        debug!("Received: {}", line);

        match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => {
                let response = server.handle_request(req).await;
                let response_json = serde_json::to_string(&response)?;
                debug!("Sending: {}", response_json);
                writeln!(stdout, "{}", response_json)?;
                stdout.flush()?;
            }
            Err(e) => {
                error!("Failed to parse request: {}", e);
                let error_response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: "Parse error".to_string(),
                        data: Some(json!({ "error": e.to_string() })),
                    }),
                };
                let response_json = serde_json::to_string(&error_response)?;
                writeln!(stdout, "{}", response_json)?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}
