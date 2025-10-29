#!/bin/bash
# Test 404 handling and model list response

PROXY_URL="${PROXY_URL:-http://127.0.0.1:8080}"

echo "🔍 Testing 404 Model Not Found Response"
echo "========================================"
echo ""

echo "📤 Requesting non-existent model..."
echo ""

# Request a model that definitely doesn't exist
PAYLOAD='{"model":"definitely-not-a-real-model-12345","messages":[{"role":"user","content":"test"}],"max_tokens":50,"stream":true}'

response=$(curl -s -N "$PROXY_URL/v1/messages" \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer test' \
  -d "$PAYLOAD" 2>&1 | head -50)

echo "📥 Response received:"
echo ""

# Check for proper Claude SSE format
if echo "$response" | grep -q "message_start"; then
  echo "✅ Contains message_start event"
else
  echo "❌ Missing message_start event"
  exit 1
fi

if echo "$response" | grep -q "content_block_start"; then
  echo "✅ Contains content_block_start event"
else
  echo "❌ Missing content_block_start event"
  exit 1
fi

if echo "$response" | grep -q "content_block_delta"; then
  echo "✅ Contains content_block_delta event"
else
  echo "❌ Missing content_block_delta event"
  exit 1
fi

# Check for model list content
if echo "$response" | grep -q "Available Models"; then
  echo "✅ Contains available models list"
else
  echo "❌ Missing available models list"
  exit 1
fi

# Check for categorization
if echo "$response" | grep -qE "(FREE|REASONING|STANDARD)"; then
  echo "✅ Models are categorized"
else
  echo "❌ Model categorization missing"
  exit 1
fi

# Check for helpful instructions
if echo "$response" | grep -q "/model"; then
  echo "✅ Contains model switching instructions"
else
  echo "❌ Missing model switching instructions"
  exit 1
fi

if echo "$response" | grep -q "message_stop"; then
  echo "✅ Contains message_stop event"
else
  echo "❌ Missing message_stop event"
  exit 1
fi

echo ""
echo "✅ All 404 handling checks passed!"
echo ""
echo "Sample response preview:"
echo "$response" | grep "text_delta" | head -3
echo ""

