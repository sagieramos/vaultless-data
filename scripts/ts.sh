#!/bin/bash

# Configuration
URL="http://localhost:8080/api/v1/messages/send"
API_KEY="vlt_bATyEhYa0PaFDop74GGeiQIH0PCp/N4Uv0AMol990Ok="
DATA='{
  "recipient_id": "os@s",
  "ciphertext": "SGVsbG8sIFZhdWx0bGVzcyBEYXRhIQ==",
  "nonce": "ZGVtb19ub25jZV8xMjM=",
  "content_type": "text/plain",
  "content_size_bytes": 21,
  "ttl_seconds": 86400
}'


# Output file
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
OUTPUT_FILE="responses_$TIMESTAMP.log"
> "$OUTPUT_FILE"  # clear file

echo "🚀 Sending 100 requests to $URL"
echo "Logs will be saved to: $OUTPUT_FILE"
echo "-------------------------------------"

for i in {1..2000}; do
  echo "-------------------------------------" >> "$OUTPUT_FILE"
  echo "📤 Request #$i at $(date)" >> "$OUTPUT_FILE"

  # Run request and capture both response and HTTP status
  response=$(curl -s -w "\nHTTP_STATUS:%{http_code}" \
    --location "$URL" \
    --header "Content-Type: application/json" \
    --header "Authorization: $API_KEY" \
    --data-raw "$DATA")

  # Extract status code
  status_code=$(echo "$response" | grep "HTTP_STATUS" | cut -d':' -f2)
  # Extract response body
  body=$(echo "$response" | sed '/HTTP_STATUS/d')

  echo "Request $i → HTTP $status_code"
  echo "HTTP $status_code" >> "$OUTPUT_FILE"
  echo "Response body:" >> "$OUTPUT_FILE"
  echo "$body" >> "$OUTPUT_FILE"
  echo "" >> "$OUTPUT_FILE"

  # Optional: slow down a bit to avoid rate limiting
  sleep 0.001
done

echo "-------------------------------------"
echo "✅ Test complete! Full responses saved in: $OUTPUT_FILE"
echo
echo "📊 Status code summary:"
grep "HTTP " "$OUTPUT_FILE" | awk '{print $2}' | sort | uniq -c | sort -nr