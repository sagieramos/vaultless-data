# ==============================
#  Vaultless API Dockerfile
# ==============================
FROM rust:1.80 AS builder

WORKDIR /app
COPY . .

# Build release binary
RUN cargo build --release

# ==============================
#  Runtime stage
# ==============================
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/vaultless-api /usr/local/bin/vaultless-api

EXPOSE 3000
ENTRYPOINT ["vaultless-api"]
