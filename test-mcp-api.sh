#!/bin/bash
# Test script to verify Strom API endpoints that MCP would use

API_URL="${STROM_API_URL:-http://localhost:3000}"

echo "Testing Strom API endpoints..."
echo "================================"
echo

# Test 1: List flows
echo "1. List all flows:"
curl -s "$API_URL/api/flows" | jq '.'
echo

# Test 2: Create a flow
echo "2. Create a test flow:"
FLOW_ID=$(curl -s -X POST "$API_URL/api/flows" \
  -H "Content-Type: application/json" \
  -d '{"name":"MCP Test Flow","auto_start":false}' | jq -r '.flow.id')
echo "Created flow: $FLOW_ID"
echo

# Test 3: Get flow details
echo "3. Get flow details:"
curl -s "$API_URL/api/flows/$FLOW_ID" | jq '.'
echo

# Test 4: List elements
echo "4. List available GStreamer elements (first 5):"
curl -s "$API_URL/api/elements" | jq '.[0:5]'
echo

# Test 5: Get element info
echo "5. Get videotestsrc element info:"
curl -s "$API_URL/api/elements/videotestsrc" | jq '.'
echo

# Test 6: Delete the test flow
echo "6. Delete test flow:"
curl -s -X DELETE "$API_URL/api/flows/$FLOW_ID"
echo "Flow deleted"
echo

echo "================================"
echo "All MCP API endpoints working!"
