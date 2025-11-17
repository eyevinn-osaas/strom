#!/bin/bash
# Complete demo: Start backend, test MCP server, cleanup

set -e

echo "Strom MCP Server Full Demo"
echo "==========================="
echo

# Step 1: Build if needed
if [ ! -f "./target/release/strom-backend" ] || [ ! -f "./target/release/strom-mcp-server" ]; then
    echo "Building Strom components..."
    cargo build --release -p strom-backend -p strom-mcp-server
    echo "✓ Build complete"
    echo
fi

# Step 2: Start backend in background
echo "Starting Strom backend..."
RUST_LOG=info cargo run --release -p strom-backend > /tmp/strom-backend.log 2>&1 &
BACKEND_PID=$!
echo "Backend started (PID: $BACKEND_PID)"

# Wait for backend to be ready
echo -n "Waiting for backend to be ready"
for i in {1..30}; do
    if curl -s http://localhost:3000/health > /dev/null 2>&1; then
        echo " ✓"
        break
    fi
    echo -n "."
    sleep 0.5
done
echo

# Step 3: Test MCP server
echo "Testing MCP Server"
echo "=================="
echo

# Test 1: Initialize
echo "1. Initialize:"
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | \
    ./target/release/strom-mcp-server 2>/dev/null | jq -c '.result.serverInfo'
echo

# Test 2: List tools
echo "2. Available tools:"
echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | \
    ./target/release/strom-mcp-server 2>/dev/null | jq -r '.result.tools[].name'
echo

# Test 3: List flows
echo "3. List flows:"
echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_flows","arguments":{}}}' | \
    ./target/release/strom-mcp-server 2>/dev/null | jq '.result.content[0].text | fromjson'
echo

# Test 4: Create a flow
echo "4. Create flow 'MCP Demo Flow':"
echo '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"create_flow","arguments":{"name":"MCP Demo Flow"}}}' | \
    ./target/release/strom-mcp-server 2>/dev/null | jq '.result.content[0].text | fromjson | .flow | {id, name}'
echo

# Test 5: List elements
echo "5. List video source elements:"
echo '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"list_elements","arguments":{"category":"source"}}}' | \
    ./target/release/strom-mcp-server 2>/dev/null | jq '.result.content[0].text | fromjson | map(select(.name | contains("video"))) | .[0:3] | map(.name)'
echo

# Test 6: Get element info
echo "6. Get videotestsrc info:"
echo '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"get_element_info","arguments":{"element_name":"videotestsrc"}}}' | \
    ./target/release/strom-mcp-server 2>/dev/null | jq '.result.content[0].text | fromjson | {name, category, properties: (.properties | length)}'
echo

echo "=================="
echo "✓ Demo complete!"
echo

# Cleanup
echo "Stopping backend..."
kill $BACKEND_PID 2>/dev/null || true
wait $BACKEND_PID 2>/dev/null || true
echo "✓ Cleanup done"
echo

echo "MCP server is fully functional!"
echo
echo "To use with Claude Desktop, add to your config:"
echo "  {\"strom\": {\"command\": \"$(pwd)/target/release/strom-mcp-server\"}}"
