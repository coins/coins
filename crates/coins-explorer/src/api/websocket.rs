use axum::{
    extract::{State, ws::{WebSocket, Message, WebSocketUpgrade}},
    response::Response,
};
use tokio::sync::broadcast;
use serde::{Serialize, Deserialize};
use crate::api::AppState;
use crate::models::{BlockSummary, StatsResponse};
use futures::{StreamExt, SinkExt};

pub struct WebSocketState {
    tx: broadcast::Sender<ServerMessage>,
}

impl WebSocketState {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(100);
        Self { tx }
    }

    pub fn broadcast(&self, msg: ServerMessage) {
        let _ = self.tx.send(msg);
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "new_block")]
    NewBlock { block: BlockSummary },
    #[serde(rename = "stats_update")]
    StatsUpdate { stats: StatsResponse },
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "subscribe")]
    Subscribe { topics: Vec<String> },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { topics: Vec<String> },
    #[serde(rename = "ping")]
    Ping,
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: AppState) {
    let mut rx = app_state.ws_state.tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Spawn task to send messages from broadcast channel
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let json = serde_json::to_string(&msg).unwrap();
            if sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    // Receive messages from client
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            if let Ok(_msg) = serde_json::from_str::<ClientMessage>(&text) {
                // Handle client messages (subscription management, ping/pong)
                // For now, we broadcast to all clients regardless of subscription
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }
}
