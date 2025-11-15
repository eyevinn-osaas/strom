use rmcp::{
    model::ServerInfo,
    schemars::{self, JsonSchema},
    service::ServiceExt,
    tool, ServerHandler,
};
use serde::Deserialize;
use std::io::{stdin, stdout};
use strom_types::api::CreateFlowRequest;
use tracing::info;

mod client;
use client::StromClient;

/// MCP Server for Strom GStreamer Flow Engine
#[derive(Clone, Debug)]
pub struct StromMcpServer {
    /// HTTP client for Strom API
    client: StromClient,
}

impl StromMcpServer {
    pub fn new(api_url: String) -> Self {
        Self {
            client: StromClient::new(api_url),
        }
    }
}

// Request types for tools
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FlowIdRequest {
    /// The UUID of the flow
    pub flow_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateFlowParams {
    /// Name for the new flow
    pub name: String,
    /// Whether to auto-start this flow on server boot
    #[serde(default)]
    pub auto_start: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateFlowParams {
    /// The UUID of the flow to update
    pub flow_id: String,
    /// Complete flow data as JSON string
    pub flow_data: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListElementsParams {
    /// Optional category filter (e.g., 'source', 'codec', 'sink')
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ElementNameRequest {
    /// Name of the GStreamer element (e.g., 'videotestsrc', 'x264enc')
    pub element_name: String,
}

// Tool implementations
#[tool(tool_box)]
impl StromMcpServer {
    /// List all GStreamer flows
    #[tool(description = "List all GStreamer flows")]
    async fn list_flows(&self) -> String {
        info!("MCP: Listing all flows");
        match self.client.list_flows().await {
            Ok(flows) => serde_json::to_string_pretty(&flows).unwrap_or_else(|e| {
                format!("Error serializing flows: {}", e)
            }),
            Err(e) => format!("Error listing flows: {}", e),
        }
    }

    /// Get details of a specific flow by ID
    #[tool(description = "Get details of a specific flow by ID")]
    async fn get_flow(&self, #[tool(aggr)] req: FlowIdRequest) -> String {
        info!("MCP: Getting flow {}", req.flow_id);
        match self.client.get_flow(&req.flow_id).await {
            Ok(flow) => serde_json::to_string_pretty(&flow).unwrap_or_else(|e| {
                format!("Error serializing flow: {}", e)
            }),
            Err(e) => format!("Error getting flow: {}", e),
        }
    }

    /// Create a new flow
    #[tool(description = "Create a new flow")]
    async fn create_flow(&self, #[tool(aggr)] params: CreateFlowParams) -> String {
        info!("MCP: Creating flow '{}'", params.name);
        let request = CreateFlowRequest {
            name: params.name,
            auto_start: params.auto_start,
        };
        match self.client.create_flow(request).await {
            Ok(flow) => serde_json::to_string_pretty(&flow).unwrap_or_else(|e| {
                format!("Error serializing flow: {}", e)
            }),
            Err(e) => format!("Error creating flow: {}", e),
        }
    }

    /// Update an existing flow
    #[tool(description = "Update an existing flow")]
    async fn update_flow(&self, #[tool(aggr)] params: UpdateFlowParams) -> String {
        info!("MCP: Updating flow {}", params.flow_id);
        match serde_json::from_str(&params.flow_data) {
            Ok(flow) => match self.client.update_flow(&params.flow_id, flow).await {
                Ok(updated) => serde_json::to_string_pretty(&updated).unwrap_or_else(|e| {
                    format!("Error serializing flow: {}", e)
                }),
                Err(e) => format!("Error updating flow: {}", e),
            },
            Err(e) => format!("Error parsing flow_data: {}", e),
        }
    }

    /// Delete a flow
    #[tool(description = "Delete a flow")]
    async fn delete_flow(&self, #[tool(aggr)] req: FlowIdRequest) -> String {
        info!("MCP: Deleting flow {}", req.flow_id);
        match self.client.delete_flow(&req.flow_id).await {
            Ok(_) => format!("Flow {} deleted successfully", req.flow_id),
            Err(e) => format!("Error deleting flow: {}", e),
        }
    }

    /// Start a flow's GStreamer pipeline
    #[tool(description = "Start a flow's GStreamer pipeline")]
    async fn start_flow(&self, #[tool(aggr)] req: FlowIdRequest) -> String {
        info!("MCP: Starting flow {}", req.flow_id);
        match self.client.start_flow(&req.flow_id).await {
            Ok(_) => format!("Flow {} started successfully", req.flow_id),
            Err(e) => format!("Error starting flow: {}", e),
        }
    }

    /// Stop a running flow
    #[tool(description = "Stop a running flow")]
    async fn stop_flow(&self, #[tool(aggr)] req: FlowIdRequest) -> String {
        info!("MCP: Stopping flow {}", req.flow_id);
        match self.client.stop_flow(&req.flow_id).await {
            Ok(_) => format!("Flow {} stopped successfully", req.flow_id),
            Err(e) => format!("Error stopping flow: {}", e),
        }
    }

    /// List available GStreamer elements, optionally filtered by category
    #[tool(description = "List available GStreamer elements, optionally filtered by category")]
    async fn list_elements(&self, #[tool(aggr)] params: ListElementsParams) -> String {
        info!("MCP: Listing elements (category: {:?})", params.category);
        match self.client.list_elements().await {
            Ok(elements) => {
                let filtered = if let Some(cat) = params.category {
                    elements
                        .into_iter()
                        .filter(|e| {
                            e.category
                                .as_ref()
                                .map(|c| c.to_lowercase().contains(&cat.to_lowercase()))
                                .unwrap_or(false)
                        })
                        .collect::<Vec<_>>()
                } else {
                    elements
                };

                serde_json::to_string_pretty(&filtered).unwrap_or_else(|e| {
                    format!("Error serializing elements: {}", e)
                })
            }
            Err(e) => format!("Error listing elements: {}", e),
        }
    }

    /// Get detailed information about a specific GStreamer element
    #[tool(description = "Get detailed information about a specific GStreamer element")]
    async fn get_element_info(&self, #[tool(aggr)] req: ElementNameRequest) -> String {
        info!("MCP: Getting info for element '{}'", req.element_name);
        match self.client.get_element_info(&req.element_name).await {
            Ok(info) => serde_json::to_string_pretty(&info).unwrap_or_else(|e| {
                format!("Error serializing element info: {}", e)
            }),
            Err(e) => format!("Error getting element info: {}", e),
        }
    }
}

// Implement ServerHandler
#[tool(tool_box)]
impl ServerHandler for StromMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            name: "strom".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            instructions: Some(
                "MCP server for Strom GStreamer Flow Engine. \
                Manage GStreamer pipelines through natural language. \
                Create, update, start, and stop flows. \
                Discover and inspect GStreamer elements."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Get Strom API URL from environment or use default
    let api_url =
        std::env::var("STROM_API_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    info!("Starting Strom MCP Server");
    info!("Connecting to Strom API at: {}", api_url);

    // Create the server
    let service = StromMcpServer::new(api_url);

    // Create stdio transport
    let transport = (stdin(), stdout());

    // Serve the MCP server
    let server = service.serve(transport).await?;

    // Wait for the server to complete
    server.waiting().await?;

    Ok(())
}
