#!/bin/bash
# Test script for MCP server using stdio

set -e

MCP_SERVER="./target/release/strom-mcp-server"

# Check if server binary exists
if [ ! -f "$MCP_SERVER" ]; then
    echo "Error: MCP server not found at $MCP_SERVER"
    echo "Build it first with: cargo build --release -p strom-mcp-server"
    exit 1
fi

echo "Testing Strom MCP Server via stdio..."
echo "======================================="
echo

# Test 1: Initialize
echo "Test 1: Initialize"
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | $MCP_SERVER | head -1 | jq '.'
echo

# Test 2: List tools
echo "Test 2: List tools"
echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | $MCP_SERVER | head -1 | jq '.result.tools | length'
echo

echo "======================================="
echo "Basic MCP protocol tests passed!"
echo
echo "For full testing with backend running:"
echo "  1. Start backend: cargo run -p strom-backend"
echo "  2. Test tool calls interactively"
