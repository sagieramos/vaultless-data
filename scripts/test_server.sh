#!/bin/bash

# -----------------------------------------------
# Vaultless Data Server Load Test Script
# -----------------------------------------------

# Usage check
if [ -z "$1" ]; then
  echo "Usage: $0 <url-to-test>"
  echo "Example: $0 http://127.0.0.1:8080/health"
  exit 1
fi

URL="$1"

# 1. Set max open files (important for high concurrency)
echo "🔧 Setting ulimit for max open files..."
ulimit -n 65535
echo "✅ ulimit set to $(ulimit -n)"

# 2. Start your server in the background
echo "🚀 Starting Vaultless Data API server..."
cargo run --bin vaultless-api &
SERVER_PID=$!
echo "Server PID: $SERVER_PID"

# Wait a few seconds for the server to be ready
sleep 5

# 3. Run wrk benchmark
THREADS=100
CONNECTIONS=1000
DURATION="30s"

echo "⚡ Running benchmark on $URL:"
echo "    Threads: $THREADS, Connections: $CONNECTIONS, Duration: $DURATION"
wrk -t$THREADS -c$CONNECTIONS -d$DURATION --latency "$URL"

# 4. Stop the server
echo "🛑 Stopping server..."
kill $SERVER_PID

echo "✅ Load test complete!"
