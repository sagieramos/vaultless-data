Client → /v1/api/keys/*
Headers:
  Authorization: Bearer <user_session_token>
  X-Api-Key-Id: <uuid_key>

1️⃣ require_token_auth
   - validates user session
   - stores SessionData(user_id=...) in request extensions

2️⃣ require_api_key_ownership
   - checks that the X-Api-Key-Id header exists
   - validates UUID and ownership in DB
   - stores key_id in request extensions

3️⃣ Handler can now safely access both:
   - `SessionData` for who is acting
   - `Uuid` key_id for which API key


| Scenario                  | Authorization | X-Api-Key-Id       | Access | Response                 |
| ------------------------- | ------------- | ------------------ | ------ | ------------------------ |
| User logged in, owns key  | ✅             | ✅                  | ✅      | Success                  |
| User logged in, no header | ✅             | ❌                  | ❌      | 400 Missing X-Api-Key-Id |
| No session, has header    | ❌             | ✅                  | ❌      | 401 Unauthorized         |
| Wrong user for key        | ✅             | ✅ (other’s key)    | ❌      | 403 Forbidden            |
| Invalid UUID              | ✅             | ❌ (invalid format) | ❌      | 400 Bad Request          |
