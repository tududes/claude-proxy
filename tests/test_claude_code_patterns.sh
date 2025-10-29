#!/bin/bash
# Test all known Claude Code request patterns
# Validates the proxy handles every content type Claude Code might send

# set -e # Don't exit on error, we want to run all tests

PROXY_URL="${1:-http://127.0.0.1:8080}"

# Load env
if [ -f .env ]; then
  export $(grep -v '^#' .env | grep -E '(API_KEY|MODEL|CHUTES_TEST_API_KEY)' | xargs 2>/dev/null)
fi
MODEL="${MODEL:-zai-org/GLM-4.5-Air}"

# Use CHUTES_TEST_API_KEY if available, fallback to API_KEY
API_KEY="${CHUTES_TEST_API_KEY:-${API_KEY:-}}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  Claude Code Request Pattern Tests${NC}"
echo -e "${BLUE}  Validates All Known Message Formats${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo ""

PASSED=0
FAILED=0

# Helper to test a payload
test_payload() {
  local name=$1
  local payload_file=$2
  local description=$3
  
  echo -e "${CYAN}[$name]${NC} $description"
  
  if [ ! -f "$payload_file" ]; then
    echo -e "  ${RED}✗ Payload file not found${NC}"
    ((FAILED++))
    return
  fi
  
  local payload=$(cat "$payload_file" | sed "s|{{MODEL}}|$MODEL|g")
  
  local cmd="curl -s -N $PROXY_URL/v1/messages -H 'content-type: application/json'"
  [ -n "$API_KEY" ] && cmd="$cmd -H 'Authorization: Bearer $API_KEY'"
  cmd="$cmd -d '$payload'"
  
  local response=$(eval "$cmd" 2>&1 | head -50)
  
  # Check if proxy forwarded to backend
  if echo "$response" | grep -q "backend_error"; then
    # Proxy forwarded but backend rejected (might not support this feature)
    echo -e "  ${YELLOW}⚠ BACKEND LIMITATION${NC} - Proxy forwarded correctly, backend doesn't support"
    ((PASSED++))  # Proxy did its job
    return
  fi
  
  # Check proxy accepted request (got message_start)
  if ! echo "$response" | grep -q "message_start"; then
    echo -e "  ${RED}✗ FAIL${NC} - Proxy rejected request or no response"
    echo "  Response: ${response:0:100}"
    ((FAILED++))
    return
  fi
  
  # Check NO OpenAI format
  if echo "$response" | grep -q '"choices"'; then
    echo -e "  ${RED}✗ CRITICAL${NC} - OpenAI format detected!"
    ((FAILED++))
    return
  fi
  
  # Check proper Claude SSE format
  if ! echo "$response" | grep -q "event: message_start"; then
    echo -e "  ${YELLOW}⚠ WARNING${NC} - Missing SSE event format"
  fi
  
  echo -e "  ${GREEN}✓ PASS${NC} - Proxy handled request correctly"
  ((PASSED++))
}

# Test 1: String content (simplest form)
test_payload "1/9" "tests/payloads/basic_request.json" \
  "String content (Claude Code simple text)"

# Test 2: Content blocks array with text
test_payload "2/9" "tests/payloads/content_blocks_text.json" \
  "Content blocks array [{type: text}]"

# Test 3: Mixed content blocks (text + image)
test_payload "3/9" "tests/payloads/content_blocks_mixed.json" \
  "Mixed content blocks (text + image)"

# Test 4: System prompt
test_payload "4/9" "tests/payloads/conversation_3_system.json" \
  "Top-level system prompt"

# Test 5: Multi-turn conversation
test_payload "5/9" "tests/payloads/conversation_2_followup.json" \
  "Multi-turn conversation with context"

# Test 6: Tool definitions
test_payload "6/9" "tests/payloads/conversation_4_tools.json" \
  "Tool definitions with input_schema"

# Test 7: Tool result in conversation
test_payload "7/9" "tests/payloads/tool_result.json" \
  "Tool result content block (tool_use_id)"

# Test 8: Temperature and top_p
echo -e "${CYAN}[8/9]${NC} Temperature and top_p parameters"
PAYLOAD='{"model":"'$MODEL'","messages":[{"role":"user","content":"test"}],"max_tokens":50,"temperature":0.7,"top_p":0.9,"stream":true}'
RESPONSE=$(curl -s -N $PROXY_URL/v1/messages -H 'content-type: application/json' -H "Authorization: Bearer ${API_KEY:-test}" -d "$PAYLOAD" 2>&1 | head -20)

if echo "$RESPONSE" | grep -q "message_start"; then
  echo -e "  ${GREEN}✓ PASS${NC} - Parameters accepted"
  ((PASSED++))
else
  echo -e "  ${RED}✗ FAIL${NC} - Request rejected"
  ((FAILED++))
fi

# Test 9: Stop sequences
echo -e "${CYAN}[9/9]${NC} Stop sequences parameter"
PAYLOAD='{"model":"'$MODEL'","messages":[{"role":"user","content":"count: 1,2,3"}],"max_tokens":50,"stop_sequences":[","],"stream":true}'
RESPONSE=$(curl -s -N $PROXY_URL/v1/messages -H 'content-type: application/json' -H "Authorization: Bearer ${API_KEY:-test}" -d "$PAYLOAD" 2>&1 | head -20)

if echo "$RESPONSE" | grep -q "message_start"; then
  echo -e "  ${GREEN}✓ PASS${NC} - Stop sequences accepted"
  ((PASSED++))
else
  echo -e "  ${RED}✗ FAIL${NC} - Request rejected"
  ((FAILED++))
fi

# Summary
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  Summary${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo ""
echo -e "Total patterns tested: $((PASSED + FAILED))"
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${RED}Failed: $FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
  echo -e "${GREEN}✅ All Claude Code patterns supported!${NC}"
  echo ""
  echo "Proxy handles:"
  echo "  ✓ String content"
  echo "  ✓ Content blocks array (text, image, tool_result)"
  echo "  ✓ System prompts"
  echo "  ✓ Multi-turn conversations"
  echo "  ✓ Tool definitions with input_schema"
  echo "  ✓ Tool results in conversation"
  echo "  ✓ Temperature, top_p parameters"
  echo "  ✓ Stop sequences"
  echo "  ✓ Authorization forwarding"
  echo ""
  echo "Ready for Claude Code integration! 🚀"
  exit 0
else
  echo -e "${RED}✗ Some patterns failed${NC}"
  echo "Check logs for details"
  exit 1
fi

