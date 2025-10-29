#!/bin/bash
# Test tool_result message handling

set -e

PROXY_URL="${PROXY_URL:-http://localhost:8080}"
MODEL="${MODEL:-claude-3-5-sonnet-20241022}"

echo "🔧 Testing Tool Result Message Handling"
echo "========================================"
echo ""

# Replace model placeholder
sed "s/{{MODEL}}/$MODEL/g" tests/payloads/tool_use_with_result.json > /tmp/tool_test.json

echo "📤 Sending tool use conversation with result..."
echo ""

response=$(curl -s -X POST "$PROXY_URL/v1/messages" \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d @/tmp/tool_test.json)

echo "📥 Response:"
echo "$response" | jq '.' || echo "$response"
echo ""

# Check for streaming events
if echo "$response" | grep -q "message_start"; then
    echo "✅ PASS: Received message_start event"
else
    echo "❌ FAIL: Missing message_start event"
    exit 1
fi

if echo "$response" | grep -q "message_delta"; then
    echo "✅ PASS: Received message_delta event"
else
    echo "⚠️  WARNING: No message_delta"
fi

echo ""
echo "✅ Tool result test completed!"
echo ""
echo "💡 Backend should receive:"
echo "   1. system message (if present)"
echo "   2. user: 'What's 25 * 4?'"
echo "   3. assistant: text + tool_calls"
echo "   4. tool: result='100' with tool_call_id"

rm -f /tmp/tool_test.json

