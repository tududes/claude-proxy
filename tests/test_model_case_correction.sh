#!/bin/bash
# Test case-insensitive model name matching

PROXY_URL="${PROXY_URL:-http://127.0.0.1:8080}"
MODEL="${MODEL:-zai-org/GLM-4.5-Air}"

echo "🔤 Testing Model Name Case Correction"
echo "======================================"
echo ""

# Test different case variations
echo "Testing case variations of model name..."
echo ""

test_case_variant() {
  local variant=$1
  local description=$2
  
  echo "📤 Testing: $variant ($description)"
  
  PAYLOAD="{\"model\":\"$variant\",\"messages\":[{\"role\":\"user\",\"content\":\"test\"}],\"max_tokens\":50,\"stream\":true}"
  
  response=$(curl -s -N "$PROXY_URL/v1/messages" \
    -H 'Content-Type: application/json' \
    -H 'Authorization: Bearer test' \
    -d "$PAYLOAD" 2>&1 | head -20)
  
  if echo "$response" | grep -qE "(message_start|content_block)"; then
    echo "   ✅ Accepted and processed"
    return 0
  else
    echo "   ❌ Rejected or error"
    echo "   Response: ${response:0:200}..."
    return 1
  fi
}

# Get model name parts
if [[ "$MODEL" == *"/"* ]]; then
  PREFIX="${MODEL%%/*}"
  SUFFIX="${MODEL#*/}"
else
  PREFIX=""
  SUFFIX="$MODEL"
fi

passed=0
total=0

# Test 1: Original case (should work)
((total++))
if test_case_variant "$MODEL" "original case"; then
  ((passed++))
fi
echo ""

# Test 2: All lowercase
((total++))
lowercase_model=$(echo "$MODEL" | tr '[:upper:]' '[:lower:]')
if test_case_variant "$lowercase_model" "all lowercase"; then
  ((passed++))
fi
echo ""

# Test 3: All uppercase (suffix only if it has a prefix)
if [ -n "$PREFIX" ]; then
  ((total++))
  uppercase_suffix="$PREFIX/$(echo "$SUFFIX" | tr '[:lower:]' '[:upper:]')"
  if test_case_variant "$uppercase_suffix" "uppercase suffix"; then
    ((passed++))
  fi
  echo ""
fi

# Test 4: Mixed case
((total++))
if [ -n "$PREFIX" ]; then
  mixed_case="$PREFIX/$(echo "$SUFFIX" | sed 's/.*/\L&/; s/[a-z]/\U&/2; s/[a-z]/\U&/5')"
else
  mixed_case=$(echo "$SUFFIX" | sed 's/.*/\L&/; s/[a-z]/\U&/2')
fi
if test_case_variant "$mixed_case" "mixed case"; then
  ((passed++))
fi
echo ""

echo "========================================="
echo "Results: $passed/$total case variations accepted"
echo ""

if [ $passed -ge 2 ]; then
  echo "✅ Case-insensitive model matching is working!"
  exit 0
else
  echo "❌ Case correction may not be functioning properly"
  exit 1
fi

