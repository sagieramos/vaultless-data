#!/bin/bash

# Test rate limiting
API_KEY="vlt_S99WkUE8f6JflPCZmu4u8ov86LqSFRXyIIYGMjdRE90="
BASE_URL="http://localhost:8080"

echo "🧪 Testing Rate Limiting..."
echo ""

# Test 1: Normal requests (should succeed)
echo "Test 1: Sending 5 requests (should succeed)..."
for i in {1..5}; do
  echo "Request $i:"
  curl -s -w "\nHTTP Status: %{http_code}\n" \
    -H "Authorization: $API_KEY" \
    "$BASE_URL/api/v1/analytics/dashboard" \
    -o /dev/null
  sleep 0.5
done

echo ""
echo "Test 2: Rapid fire (testing rate limit)..."
for i in {1..65}; do
  response=$(curl -s -w "\n%{http_code}" \
    -H "Authorization: $API_KEY" \
    "$BASE_URL/api/v1/analytics/dashboard")
  
  status=$(echo "$response" | tail -n1)
  
  if [ "$status" = "429" ]; then
    echo "✅ Rate limit triggered at request $i"
    echo "Response headers:"
    curl -I -H "Authorization: $API_KEY" "$BASE_URL/api/v1/analytics/dashboard" 2>/dev/null | grep -E "X-RateLimit|Retry-After"
    break
  fi
done

echo ""
echo "Test 3: Check rate limit status..."
KEY_ID=$(curl -s "$BASE_URL/api/v1/admin/keys" | jq -r '.[0].id')
curl -s "$BASE_URL/api/v1/admin/keys/$KEY_ID/rate-limit" | jq .

echo ""
echo "✅ Rate limiting tests complete!"
