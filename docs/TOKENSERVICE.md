# Vaultless TokenService Flow

This document explains the `TokenService` implementation used in Vaultless for access and refresh token management, including the opaque token flow, caching, and database interactions.

---

## 1. Overview

Vaultless uses a **custom opaque token flow** to manage user authentication and session state. Instead of JWTs, tokens are random, unguessable strings stored in both a cache (Dragonfly/Redis) and Postgres for auditing and fallback purposes. This design allows immediate revocation, secure token rotation, and prevents sensitive data exposure.

### Key Components

* **Access Token**: Short-lived opaque token (default 1 hour). Stored in cache and Postgres.
* **Refresh Token**: Longer-lived opaque token (default 30 days). Stored in cache and Postgres. Supports rotation.
* **SessionData**: Contains user identity, email, scope, admin flag, and creation timestamp.
* **Cache**: Dragonfly/Redis stores active sessions and refresh tokens.
* **Database (Postgres)**: Stores historical sessions and refresh tokens for auditing and recovery.

---

## 2. TokenService Flow

```mermaid
flowchart TD
    A[User Login] --> B[TokenService::create_token_pair]
    B --> C[Generate Access Token]
    B --> D[Generate Refresh Token]
    C --> E[Hash Access Token]
    D --> F[Hash Refresh Token]
    E --> G[Store Access Token in Cache]
    F --> H[Store Refresh Token in Cache]
    G --> I[Log Session in Postgres (async)]
    H --> J[Store Refresh Token in Postgres (async)]
    B --> K[Return TokenPair to Client]

    subgraph Access Token Validation
        L[Client Requests Resource with Access Token] --> M[TokenService::verify_access_token]
        M --> N[Check Cache for SessionData]
        N -->|Hit| O[Return SessionData]
        N -->|Miss| P[Check Postgres]
        P --> Q[Repopulate Cache]
        Q --> O
    end

    subgraph Refresh Token Flow
        R[Client Sends Refresh Token] --> S[TokenService::refresh_token]
        S --> T[Check Cache for RefreshTokenCache]
        T -->|Hit| U[Validate & Rotate Token]
        T -->|Miss| V[Check Postgres]
        U --> W[Generate New TokenPair]
        V --> W
        W --> X[Store New Tokens in Cache & Postgres]
        X --> Y[Return New TokenPair to Client]
    end

    subgraph Revocation
        Z[Revoke Access Token] --> AA[Delete from Cache]
        AA --> AB[Mark revoked in Postgres (async)]
    end

```
### Vaultless Opaque Token Flow
```
Client
  |
  |  GET /notifications
  |  Authorization: Bearer <access_token>
  v
Server
  |
  |-- Extract token
  |-- Hash token
  |-- Check cache (Dragonfly/Redis) ----+
  |                                     |
  |            Cache hit? --------------+--> YES: session data returned
  |                                     |
  |            Cache miss?              v
  |-- Check Postgres (fallback) ---> Load session data
  |-- Repopulate cache
  |
Handler
  |
  |-- Process request using SessionData
  |
Response ---> Client

```

### Token Refresh Flow
```
Client
  |
  |-- Refresh token used --> POST /refresh
  v
Server
  |
  |-- Verify refresh token in cache (or DB)
  |-- Check if already used/revoked
  |-- Generate new access + refresh token
  |-- Update cache & DB
  |
Response ---> Client
```
