# 🔐 Vaultless Data

**Author:** Stanley Osagie

Vaultless Data is a **privacy-first, end-to-end encrypted message relay platform** built with Rust and PostgreSQL.  
It allows sending and receiving messages **without storing any private keys or plaintext** on the backend. Perfect for anyone who wants to develop **privacy-conscious, secure messaging applications**.

---

## 🎯 What Makes Vaultless Data Unique

### Technical Edge
- True zero-knowledge architecture (backend never sees keys or plaintext)
- Cryptographically verifiable message proofs
- Enterprise-grade encryption (AES-256-GCM + Ed25519)

### Business Edge
- Developer API for encrypted messaging infrastructure
- Privacy-as-a-Service model
- Verifiable proof system (audit trail without compromising privacy)

---

## ⚡ Features
- End-to-end encryption for messages
- Zero-knowledge backend
- User roles and message TTL (Free / Starter / Pro)
- API key and rate-limiting support
- PostgreSQL database with Docker Compose setup
- Developer-friendly REST API
- Audit trail and verifiable proofs

---

## 🛠️ Tech Stack
- **Backend:** Rust (Axum framework)  
- **Database:** PostgreSQL  
- **Encryption:** AES-256-GCM + Ed25519 signatures  
- **Containerization:** Docker Compose  
- **Logging:** `tracing` crate for structured logs  

---

## 📦 Project Structure


---

## 🚀 Quick Start

### 1. Clone the repository
```bash
git clone https://github.com/yourusername/vaultless-data.git
cd vaultless-data
```

### 2. Create `.env` file
Copy the example and edit:
```bash
cp .env.example .env
# Then update database password and salts as needed
```

### 3. Start PostgreSQL
```bash
docker-compose up -d
```

### 4. Run migrations
```bash
cargo install sqlx-cli --no-default-features --features postgres
sqlx migrate run
```

### 5. Build and run the server
```bash
cargo build
cargo run
```

### 6. Access API
- Default host: `http://0.0.0.0:8080`  
- API docs: `docs/api/index.html`

---

## 🔑 Environment Variables
| Variable | Description | Default / Example |
|----------|-------------|-----------------|
| `HOST` | Server bind address | `0.0.0.0` |
| `PORT` | Server port | `8080` |
| `RUST_LOG` | Logging level | `**info**,vaultless_api=debug,vaultless_core=debug` |
| `DATABASE_URL` | PostgreSQL connection string | `postgresql://vaultless:vaultless_dev_pass@localhost:5432/vaultless_db` |
| `DATABASE_MAX_CONNECTIONS` | Max DB connections | `10` |
| `API_KEY_SALT` | Salt for API key hashing | `change-this-random-salt-in-production` |
| `RATE_LIMIT_PER_MINUTE` | Requests per minute | `60` |
| `MESSAGE_TTL_FREE` | Free user message TTL (seconds) | `604800` |
| `MESSAGE_TTL_STARTER` | Starter user message TTL (seconds) | `2592000` |
| `MESSAGE_TTL_PRO` | Pro user message TTL (seconds) | `7776000` |

---

## 🏗️ Architecture Overview

```
Frontend <-> API Server (Axum) <-> PostgreSQL
                     |
                     +-> Zero-knowledge layer
                     +-> AES-256-GCM encryption
                     +-> Ed25519 signatures
```

- The backend **never has access to plaintext messages**.  
- All messages are stored **encrypted** in the database.  
- Audit trails and verifiable proofs are generated for integrity verification.

---

## 📄 API Endpoints (examples)

### POST /messages
Send a new encrypted message
```json
{
  "recipient_id": "user123",
  "ciphertext": "BASE64_ENCRYPTED_MESSAGE",
  "nonce": "BASE64_NONCE"
}
```

### GET /messages
Fetch messages for the authenticated user

### POST /keys/request
Request recipient public key (for end-to-end encryption)

---

## 📝 Contributing
1. Fork the repo  
2. Create a feature branch (`git checkout -b feature/awesome-feature`)  
3. Commit your changes (`git commit -m 'Add awesome feature'`)  
4. Push (`git push origin feature/awesome-feature`)  
5. Open a pull request

---

## 📜 License
MIT License – see [LICENSE](LICENSE) file for details

---

## 💡 Roadmap
- [ ] Full WebSocket support for real-time messaging  
- [ ] Multi-tenant privacy-as-a-service model  
- [ ] Optional decentralized storage (IPFS / S3)  
- [ ] End-to-end encrypted group chat  
- [ ] Dashboard for monitoring message delivery and proofs

---

## 🤝 Support
For help, please open an issue or reach out to `contact@vaultless.io`.


<!-- let period_start = chrono::Utc::now().with_minute(0).unwrap().with_second(0).unwrap();
 -->