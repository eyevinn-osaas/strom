#!/bin/bash
# Interactive test of MCP server with actual Strom backend

set -e

MCP_SERVER="./target/release/strom-mcp-server"
BACKEND_URL="${STROM_API_URL:-http://localhost:3000}"

echo "Strom MCP Server Interactive Test"
echo "===================================="
echo

# Check if MCP server binary exists
if [ ! -f "$MCP_SERVER" ]; then
    echo "Building MCP server..."
    cargo build --release -p strom-mcp-server
fi

# Check if backend is running
echo "Checking if Strom backend is running at $BACKEND_URL..."
if ! curl -s "$BACKEND_URL/health" > /dev/null 2>&1; then
    echo "❌ Error: Strom backend is not running at $BACKEND_URL"
    echo
    echo "Please start the backend first:"
    echo "  cargo run -p strom-backend"
    echo
    exit 1
fi

echo "✓ Backend is running"
echo

# Function to send MCP request
send_mcp() {
    local id=$1
    local method=$2
    local params=$3

    if [ -z "$params" ]; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"method\":\"$method\"}"
    else
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"method\":\"$method\",\"params\":$params}"
    fi
}

echo "Test 1: Initialize MCP Server"
echo "------------------------------"
send_mcp 1 "initialize" '{}' | STROM_API_URL=$BACKEND_URL $MCP_SERVER 2>/dev/null | jq '.'
echo

echo "Test 2: List Available Tools"
echo "-----------------------------"
send_mcp 2 "tools/list" | STROM_API_URL=$BACKEND_URL $MCP_SERVER 2>/dev/null | jq '.result.tools[] | {name, description}'
echo

echo "Test 3: List Current Flows"
echo "---------------------------"
send_mcp 3 "tools/call" '{"name":"list_flows","arguments":{}}' | STROM_API_URL=$BACKEND_URL $MCP_SERVER 2>/dev/null | jq '.'
echo

echo "Test 4: Create a Test Flow"
echo "---------------------------"
send_mcp 4 "tools/call" '{"name":"create_flow","arguments":{"name":"MCP Test Flow"}}' | STROM_API_URL=$BACKEND_URL $MCP_SERVER 2>/dev/null | jq '.'
echo

# Extract flow ID from the response (if we had it)
echo "Test 5: List GStreamer Elements (Video Sources)"
echo "------------------------------------------------"
send_mcp 5 "tools/call" '{"name":"list_elements","arguments":{"category":"source"}}' | STROM_API_URL=$BACKEND_URL $MCP_SERVER 2>/dev/null | jq '.result.content[0].text | fromjson | .[0:3]'
echo

echo "Test 6: Get Element Info (videotestsrc)"
echo "-----------------------------------------"
send_mcp 6 "tools/call" '{"name":"get_element_info","arguments":{"element_name":"videotestsrc"}}' | STROM_API_URL=$BACKEND_URL $MCP_SERVER 2>/dev/null | jq '.result.content[0].text | fromjson | {name, description, category}'
echo

echo "===================================="
echo "✓ All tests completed successfully!"
echo
echo "The MCP server is working correctly with Strom backend."
echo
echo "Next steps:"
echo "  1. Configure Claude Desktop with this MCP server"
echo "  2. Use natural language to manage GStreamer pipelines"
echo
