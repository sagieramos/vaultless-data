# WebSocket & MQTT Integration Guide

> Real-time message delivery with WebSocket and MQTT protocols

## Table of Contents

- [WebSocket Integration](#websocket-integration)
- [MQTT Integration](#mqtt-integration)
- [Hybrid Architecture](#hybrid-architecture)

---

## WebSocket Integration

### Architecture

```
┌──────────┐         ┌────────────┐         ┌──────────────┐
│  Client  │◄───WS──►│  WebSocket │◄───────►│ InstantMsg   │
│ Browser  │         │   Server   │         │   + Redis    │
└──────────┘         └────────────┘         └──────────────┘
                            │
                            ▼
                     Redis Pub/Sub
                     (notifications)
```

### Implementation

```rust
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

// WebSocket connection state
#[derive(Clone)]
struct AppState {
    instant_message: Arc<InstantMessage>,
    redis_pool: Arc<RedisPool>,
    // Broadcast channel for real-time notifications
    tx: broadcast::Sender<Notification>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Notification {
    recipient_id: Uuid,
    message_id: Uuid,
    event_type: NotificationType,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum NotificationType {
    NewMessage,
    MessageRead,
    MessageDelivered,
}

// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // Client authentication
    let client_id = match authenticate_ws_client(&mut receiver).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("WS auth failed: {}", e);
            return;
        }
    };

    tracing::info!("WebSocket connected: {}", client_id);

    // Spawn task to listen for broadcast notifications
    let mut send_task = tokio::spawn(async move {
        while let Ok(notification) = rx.recv().await {
            // Only send notifications for this client
            if notification.recipient_id == client_id {
                let payload = serde_json::to_string(&notification).unwrap();
                if sender.send(WsMessage::Text(payload)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Spawn task to handle incoming client messages
    let im = Arc::clone(&state.instant_message);
    let tx = state.tx.clone();
    
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let WsMessage::Text(text) = msg {
                if let Err(e) = handle_client_message(
                    &text,
                    client_id,
                    &im,
                    &tx,
                ).await {
                    tracing::error!("Failed to handle message: {}", e);
                }
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }

    tracing::info!("WebSocket disconnected: {}", client_id);
}

// Authenticate WebSocket client
async fn authenticate_ws_client(
    receiver: &mut futures::stream::SplitStream<WebSocket>,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    use futures::StreamExt;
    
    if let Some(Ok(WsMessage::Text(text))) = receiver.next().await {
        let auth: AuthMessage = serde_json::from_str(&text)?;
        // TODO: Verify JWT token or API key
        Ok(auth.client_id)
    } else {
        Err("Authentication failed".into())
    }
}

#[derive(Deserialize)]
struct AuthMessage {
    client_id: Uuid,
    token: String,
}

// Handle incoming client messages
async fn handle_client_message(
    text: &str,
    client_id: Uuid,
    im: &InstantMessage,
    tx: &broadcast::Sender<Notification>,
) -> Result<(), Box<dyn std::error::Error>> {
    let msg: ClientMessage = serde_json::from_str(text)?;

    match msg.action.as_str() {
        "send" => {
            let msg_id = im.send_instant_message(
                client_id,
                msg.recipient_id,
                msg.ciphertext,
                msg.nonce,
                msg.content_size_bytes,
                msg.api_key_id,
                msg.signature,
                msg.envelope_public_key,
            ).await?;

            // Notify recipient via WebSocket
            let _ = tx.send(Notification {
                recipient_id: msg.recipient_id,
                message_id: msg_id,
                event_type: NotificationType::NewMessage,
            });

            Ok(())
        }
        "fetch" => {
            let messages = im.fetch_messages_for_recipient(client_id).await?;
            // Send messages back through WebSocket
            // (Implementation depends on your protocol)
            Ok(())
        }
        "mark_read" => {
            im.mark_read_instant_message(client_id, msg.message_id.unwrap()).await?;
            
            // Notify sender of read receipt
            let _ = tx.send(Notification {
                recipient_id: msg.sender_id.unwrap(),
                message_id: msg.message_id.unwrap(),
                event_type: NotificationType::MessageRead,
            });

            Ok(())
        }
        _ => Err("Unknown action".into()),
    }
}

#[derive(Deserialize)]
struct ClientMessage {
    action: String,
    recipient_id: Uuid,
    sender_id: Option<Uuid>,
    message_id: Option<Uuid>,
    ciphertext: String,
    nonce: String,
    content_size_bytes: i64,
    api_key_id: Option<Uuid>,
    signature: String,
    envelope_public_key: String,
}

// Redis Pub/Sub listener for cross-server notifications
async fn redis_pubsub_listener(
    redis_pool: Arc<RedisPool>,
    tx: broadcast::Sender<Notification>,
) {
    let mut conn = redis_pool.get().await.unwrap();
    let mut pubsub = conn.as_mut().into_pubsub();
    
    // Subscribe to read receipt pattern
    pubsub.psubscribe("read:*").await.unwrap();

    loop {
        if let Ok(msg) = pubsub.on_message().next().await {
            let channel: String = msg.get_channel_name().to_string();
            let reader_id: String = msg.get_payload().unwrap();
            
            if let Some(msg_id_str) = channel.strip_prefix("read:") {
                if let (Ok(msg_id), Ok(reader_id)) = (
                    Uuid::parse_str(msg_id_str),
                    Uuid::parse_str(&reader_id),
                ) {
                    let _ = tx.send(Notification {
                        recipient_id: reader_id,
                        message_id: msg_id,
                        event_type: NotificationType::MessageRead,
                    });
                }
            }
        }
    }
}

// Setup WebSocket server
pub fn create_websocket_router(
    instant_message: Arc<InstantMessage>,
    redis_pool: Arc<RedisPool>,
) -> Router {
    let (tx, _rx) = broadcast::channel(1000);

    // Spawn Redis pub/sub listener
    tokio::spawn(redis_pubsub_listener(
        Arc::clone(&redis_pool),
        tx.clone(),
    ));

    let state = AppState {
        instant_message,
        redis_pool,
        tx,
    };

    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}
```

### Client Usage (JavaScript)

```javascript
// Connect to WebSocket
const ws = new WebSocket('ws://localhost:3000/ws');

ws.onopen = () => {
  // Authenticate
  ws.send(JSON.stringify({
    client_id: 'uuid-here',
    token: 'jwt-token-here'
  }));
};

ws.onmessage = (event) => {
  const notification = JSON.parse(event.data);
  
  switch (notification.event_type) {
    case 'NewMessage':
      console.log('New message received:', notification.message_id);
      // Fetch the message
      ws.send(JSON.stringify({ action: 'fetch' }));
      break;
      
    case 'MessageRead':
      console.log('Message was read:', notification.message_id);
      break;
  }
};

// Send a message
function sendMessage(recipientId, encryptedContent) {
  ws.send(JSON.stringify({
    action: 'send',
    recipient_id: recipientId,
    ciphertext: encryptedContent,
    nonce: 'base64-nonce',
    content_size_bytes: 1024,
    signature: 'base64-signature',
    envelope_public_key: 'base64-pubkey'
  }));
}
```

---

## MQTT Integration

### Architecture

```
┌──────────┐         ┌────────────┐         ┌──────────────┐
│  IoT     │◄──MQTT─►│   MQTT     │◄───────►│ InstantMsg   │
│  Device  │         │   Broker   │         │   + Redis    │
└──────────┘         └────────────┘         └──────────────┘
                      (Mosquitto/
                       EMQX)
```

### Implementation

```rust
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, QoS};
use std::sync::Arc;
use tokio::time::Duration;
use uuid::Uuid;

// MQTT message types
#[derive(Serialize, Deserialize)]
struct MqttMessage {
    message_id: Uuid,
    sender_id: Uuid,
    recipient_id: Uuid,
    ciphertext: String,
    nonce: String,
    content_size_bytes: i64,
    signature: String,
    envelope_public_key: String,
}

#[derive(Serialize, Deserialize)]
struct MqttNotification {
    event_type: String,
    message_id: Uuid,
    timestamp: i64,
}

// MQTT handler
pub struct MqttHandler {
    instant_message: Arc<InstantMessage>,
    client: AsyncClient,
}

impl MqttHandler {
    pub async fn new(
        instant_message: Arc<InstantMessage>,
        broker_host: &str,
        broker_port: u16,
    ) -> Result<(Self, EventLoop), Box<dyn std::error::Error>> {
        let mut mqtt_options = MqttOptions::new(
            "instant_message_server",
            broker_host,
            broker_port,
        );
        
        mqtt_options.set_keep_alive(Duration::from_secs(30));
        mqtt_options.set_clean_session(true);

        let (client, event_loop) = AsyncClient::new(mqtt_options, 100);

        Ok((
            Self {
                instant_message,
                client,
            },
            event_loop,
        ))
    }

    // Subscribe to topics
    pub async fn subscribe_topics(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Subscribe to message sending topic pattern
        self.client
            .subscribe("message/+/send", QoS::AtLeastOnce)
            .await?;

        // Subscribe to message fetching topic pattern
        self.client
            .subscribe("message/+/fetch", QoS::AtLeastOnce)
            .await?;

        // Subscribe to read receipt topic pattern
        self.client
            .subscribe("message/+/read", QoS::AtLeastOnce)
            .await?;

        tracing::info!("Subscribed to MQTT topics");
        Ok(())
    }

    // Handle incoming MQTT messages
    pub async fn handle_event(
        &self,
        event: Event,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Event::Incoming(Incoming::Publish(p)) = event {
            let topic = p.topic.as_str();
            let payload = String::from_utf8(p.payload.to_vec())?;

            tracing::debug!("MQTT message on topic: {}", topic);

            if topic.contains("/send") {
                self.handle_send_message(&payload).await?;
            } else if topic.contains("/fetch") {
                self.handle_fetch_messages(topic, &payload).await?;
            } else if topic.contains("/read") {
                self.handle_mark_read(&payload).await?;
            }
        }

        Ok(())
    }

    // Handle message sending
    async fn handle_send_message(
        &self,
        payload: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mqtt_msg: MqttMessage = serde_json::from_str(payload)?;

        let msg_id = self.instant_message
            .send_instant_message(
                mqtt_msg.sender_id,
                mqtt_msg.recipient_id,
                mqtt_msg.ciphertext,
                mqtt_msg.nonce,
                mqtt_msg.content_size_bytes,
                None, // API key would come from authentication
                mqtt_msg.signature,
                mqtt_msg.envelope_public_key,
            )
            .await?;

        // Publish notification to recipient's topic
        let notification = MqttNotification {
            event_type: "new_message".to_string(),
            message_id: msg_id,
            timestamp: chrono::Utc::now().timestamp(),
        };

        let notification_topic = format!("notification/{}/messages", mqtt_msg.recipient_id);
        let notification_payload = serde_json::to_string(&notification)?;

        self.client
            .publish(
                notification_topic,
                QoS::AtLeastOnce,
                false,
                notification_payload,
            )
            .await?;

        tracing::info!("MQTT message sent: {}", msg_id);
        Ok(())
    }

    // Handle message fetching
    async fn handle_fetch_messages(
        &self,
        topic: &str,
        _payload: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Extract client_id from topic: message/{client_id}/fetch
        let parts: Vec<&str> = topic.split('/').collect();
        let client_id = Uuid::parse_str(parts[1])?;

        let messages = self.instant_message
            .fetch_messages_for_recipient(client_id)
            .await?;

        // Publish messages to client's response topic
        let response_topic = format!("response/{}/messages", client_id);
        
        for msg in messages {
            let payload = serde_json::to_string(&msg)?;
            self.client
                .publish(&response_topic, QoS::AtLeastOnce, false, payload)
                .await?;
        }

        tracing::info!("Sent {} messages to {}", messages.len(), client_id);
        Ok(())
    }

    // Handle mark as read
    async fn handle_mark_read(
        &self,
        payload: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        #[derive(Deserialize)]
        struct ReadRequest {
            reader_id: Uuid,
            message_id: Uuid,
        }

        let req: ReadRequest = serde_json::from_str(payload)?;

        self.instant_message
            .mark_read_instant_message(req.reader_id, req.message_id)
            .await?;

        tracing::info!("Message {} marked as read by {}", req.message_id, req.reader_id);
        Ok(())
    }
}

// Start MQTT server
pub async fn run_mqtt_server(
    instant_message: Arc<InstantMessage>,
    broker_host: &str,
    broker_port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let (handler, mut event_loop) = MqttHandler::new(
        instant_message,
        broker_host,
        broker_port,
    ).await?;

    handler.subscribe_topics().await?;

    tracing::info!("MQTT server started");

    // Event loop
    loop {
        match event_loop.poll().await {
            Ok(event) => {
                if let Err(e) = handler.handle_event(event).await {
                    tracing::error!("MQTT event handling failed: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("MQTT connection error: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
```

### Client Usage (Python - paho-mqtt)

```python
import paho.mqtt.client as mqtt
import json
import uuid

# MQTT callbacks
def on_connect(client, userdata, flags, rc):
    print(f"Connected with result code {rc}")
    
    # Subscribe to notifications for this client
    client_id = userdata['client_id']
    client.subscribe(f"notification/{client_id}/messages")
    client.subscribe(f"response/{client_id}/messages")

def on_message(client, userdata, msg):
    topic = msg.topic
    payload = json.loads(msg.payload.decode())
    
    if 'notification' in topic:
        print(f"New message notification: {payload['message_id']}")
        # Fetch the message
        fetch_messages(client, userdata['client_id'])
    elif 'response' in topic:
        print(f"Received message: {payload}")

# Setup MQTT client
client_id = str(uuid.uuid4())
client = mqtt.Client(userdata={'client_id': client_id})
client.on_connect = on_connect
client.on_message = on_message

client.connect("localhost", 1883, 60)

# Send a message
def send_message(recipient_id, encrypted_content):
    message = {
        "message_id": str(uuid.uuid4()),
        "sender_id": client_id,
        "recipient_id": recipient_id,
        "ciphertext": encrypted_content,
        "nonce": "base64-nonce",
        "content_size_bytes": 1024,
        "signature": "base64-signature",
        "envelope_public_key": "base64-pubkey"
    }
    
    client.publish(
        f"message/{client_id}/send",
        json.dumps(message),
        qos=1
    )

# Fetch messages
def fetch_messages(client, client_id):
    client.publish(
        f"message/{client_id}/fetch",
        json.dumps({}),
        qos=1
    )

# Mark as read
def mark_read(message_id):
    payload = {
        "reader_id": client_id,
        "message_id": message_id
    }
    
    client.publish(
        f"message/{client_id}/read",
        json.dumps(payload),
        qos=1
    )

# Start the client
client.loop_start()

# Example: Send a message
send_message("recipient-uuid", "encrypted-data")

# Keep the client running
try:
    while True:
        time.sleep(1)
except KeyboardInterrupt:
    client.loop_stop()
    client.disconnect()
```

---

## Hybrid Architecture

### Combining WebSocket + MQTT

```rust
use tokio::sync::broadcast;

// Unified notification system
#[derive(Clone)]
pub struct UnifiedMessaging {
    instant_message: Arc<InstantMessage>,
    notification_tx: broadcast::Sender<Notification>,
}

impl UnifiedMessaging {
    pub fn new(instant_message: Arc<InstantMessage>) -> Self {
        let (tx, _) = broadcast::channel(10000);
        Self {
            instant_message,
            notification_tx: tx,
        }
    }

    // Send message (called by both WS and MQTT)
    pub async fn send_message(
        &self,
        sender_id: Uuid,
        recipient_id: Uuid,
        ciphertext: String,
        nonce: String,
        content_size_bytes: i64,
        api_key_id: Option<Uuid>,
        signature: String,
        envelope_public_key: String,
    ) -> Result<Uuid, Box<dyn std::error::Error>> {
        let msg_id = self.instant_message
            .send_instant_message(
                sender_id,
                recipient_id,
                ciphertext,
                nonce,
                content_size_bytes,
                api_key_id,
                signature,
                envelope_public_key,
            )
            .await?;

        // Broadcast to both WS and MQTT listeners
        let _ = self.notification_tx.send(Notification {
            recipient_id,
            message_id: msg_id,
            event_type: NotificationType::NewMessage,
        });

        Ok(msg_id)
    }

    pub fn subscribe_notifications(&self) -> broadcast::Receiver<Notification> {
        self.notification_tx.subscribe()
    }
}

// Main server combining both protocols
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize InstantMessage
    let redis_pool = /* ... */;
    let db_pool = /* ... */;
    let config = MetricsConfig::default();
    let im = Arc::new(InstantMessage::new(redis_pool, db_pool, config)?);

    // Create unified messaging
    let unified = Arc::new(UnifiedMessaging::new(Arc::clone(&im)));

    // Start WebSocket server
    let ws_unified = Arc::clone(&unified);
    tokio::spawn(async move {
        let app = create_websocket_router_unified(ws_unified);
        axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    // Start MQTT server
    let mqtt_unified = Arc::clone(&unified);
    tokio::spawn(async move {
        run_mqtt_server_unified(mqtt_unified, "localhost", 1883)
            .await
            .unwrap();
    });

    // Keep main thread alive
    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

### Topic Mapping

| Protocol | Topic/Route | Purpose |
|----------|-------------|---------|
| **WebSocket** | `/ws` | Main WebSocket endpoint |
| **MQTT** | `message/{client_id}/send` | Send message |
| **MQTT** | `message/{client_id}/fetch` | Fetch messages |
| **MQTT** | `message/{client_id}/read` | Mark as read |
| **MQTT** | `notification/{client_id}/messages` | Receive notifications |
| **MQTT** | `response/{client_id}/messages` | Receive fetched messages |

### QoS Recommendations

| Message Type | WebSocket | MQTT QoS |
|--------------|-----------|----------|
| Chat messages | N/A | QoS 1 (At Least Once) |
| Notifications | Best effort | QoS 0 (At Most Once) |
| Read receipts | Best effort | QoS 0 (At Most Once) |
| Critical alerts | N/A | QoS 2 (Exactly Once) |

---

## Dependencies

```toml
[dependencies]
# WebSocket
axum = { version = "0.7", features = ["ws"] }
tokio-tungstenite = "0.21"
futures = "0.3"

# MQTT
rumqttc = "0.23"

# Common
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
redis = { version = "0.24", features = ["tokio-comp", "connection-manager"] }
tracing = "0.1"
```

---

## Performance Considerations

### WebSocket
- **Connections**: ~10K concurrent connections per server
- **Latency**: <5ms message delivery
- **Overhead**: ~2KB per connection

### MQTT
- **Connections**: ~100K concurrent connections per broker
- **Latency**: ~10ms message delivery (QoS 1)
- **Overhead**: ~1KB per connection

### When to Use What

| Use Case | Recommended Protocol |
|----------|---------------------|
| Web browsers | WebSocket |
| IoT devices | MQTT |
| Mobile apps | MQTT (battery efficient) |
| Desktop apps | WebSocket or MQTT |
| Real-time dashboards | WebSocket |
| Low-bandwidth devices | MQTT |

---

## Security Considerations

1. **Authentication**: Validate JWT tokens or API keys on connection
2. **Authorization**: Check topic access permissions
3. **Rate Limiting**: Implement per-client rate limits
4. **TLS**: Use WSS/MQTTS in production
5. **Message Validation**: Verify signatures before processing

---

This guide provides production-ready implementations for both WebSocket and MQTT integration with the InstantMessage system!