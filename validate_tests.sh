#!/bin/bash
# Validate that all test_*.sh scripts conform to Claude Messages API spec

set -e

BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  Claude API Spec Compliance Validator${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo ""

PASS=0
FAIL=0

check_file() {
  local file=$1
  local errors=0

  echo -e "${YELLOW}Checking: ${file}${NC}"

  # Check 1: Uses /v1/messages endpoint
  if grep -q "/v1/messages" "$file"; then
    echo -e "  ${GREEN}✓${NC} Uses /v1/messages endpoint"
  else
    echo -e "  ${RED}✗${NC} Missing /v1/messages endpoint"
    ((errors++))
  fi

  # Check 2: Includes Content-Type header
  if grep -q "content-type.*application/json" "$file"; then
    echo -e "  ${GREEN}✓${NC} Sets Content-Type: application/json"
  else
    echo -e "  ${RED}✗${NC} Missing Content-Type header"
    ((errors++))
  fi

  # Check 3: Uses MODEL from env
  if grep -q "MODEL" "$file" && grep -q '\"model\".*\${MODEL}' "$file"; then
    echo -e "  ${GREEN}✓${NC} Uses MODEL from environment"
  else
    echo -e "  ${YELLOW}⚠${NC}  Should use MODEL from .env"
  fi

  # Check 4: Includes messages array
  if grep -q '\"messages\"' "$file"; then
    echo -e "  ${GREEN}✓${NC} Includes messages array"
  else
    echo -e "  ${RED}✗${NC} Missing messages array"
    ((errors++))
  fi

  # Check 5: Sets stream: true
  if grep -q '\"stream\":.*true' "$file"; then
    echo -e "  ${GREEN}✓${NC} Sets stream: true"
  else
    echo -e "  ${RED}✗${NC} Missing stream: true"
    ((errors++))
  fi

  # Check 6: Includes max_tokens
  if grep -q '\"max_tokens\"' "$file"; then
    echo -e "  ${GREEN}✓${NC} Includes max_tokens"
  else
    echo -e "  ${RED}✗${NC} Missing max_tokens"
    ((errors++))
  fi

  # Check 7: Checks for Claude SSE events (text, text_delta, message_start, content_block, tool_use)
  # Note: Simple passthrough tests (like test_request.sh) can skip validation for manual inspection
  if grep -qE "(text_delta|message_start|content_block|grep.*text|tool_use)" "$file"; then
    echo -e "  ${GREEN}✓${NC} Validates Claude SSE events"
  elif [[ "$file" == "test_request.sh" ]]; then
    echo -e "  ${YELLOW}⚠${NC}  Manual inspection test (no automated validation)"
  else
    echo -e "  ${RED}✗${NC} Doesn't check for Claude SSE events"
    ((errors++))
  fi

  # Check 8: Uses Authorization header properly
  if grep -q "Authorization.*Bearer" "$file"; then
    echo -e "  ${GREEN}✓${NC} Uses Authorization: Bearer header"
  else
    echo -e "  ${YELLOW}⚠${NC}  No Authorization header (optional)"
  fi

  echo ""

  if [ $errors -eq 0 ]; then
    echo -e "  ${GREEN}✓ PASS${NC} - Compliant with Claude API spec"
    ((PASS++))
  else
    echo -e "  ${RED}✗ FAIL${NC} - ${errors} compliance issue(s)"
    ((FAIL++))
  fi

  echo ""
}

# Find and check all test_*.sh files
for test_file in tests/test_*.sh; do
  if [ -f "$test_file" ]; then
    check_file "$test_file"
  fi
done

echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  Summary${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════${NC}"
echo ""
echo -e "Tests checked: $((PASS + FAIL))"
echo -e "${GREEN}Passed: ${PASS}${NC}"
echo -e "${RED}Failed: ${FAIL}${NC}"
echo ""

if [ $FAIL -eq 0 ]; then
  echo -e "${GREEN}✓ All test scripts are compliant with Claude Messages API spec!${NC}"
  exit 0
else
  echo -e "${RED}✗ Some test scripts need fixes. See API_REFERENCE.md for details.${NC}"
  exit 1
fi
