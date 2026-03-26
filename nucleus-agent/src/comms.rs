use futures_util::{SinkExt, StreamExt};
use nucleus_common::messages::AgentToServer;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn, error};

/// Manages /comms WebSocket relay connections.
/// Each relay opens a WebSocket to a target URL (e.g., ws://127.0.0.1:1880/comms)
/// and relays frames bidirectionally between the backend and the target.
pub struct CommsManager {
    ws_tx: mpsc::UnboundedSender<Message>,
    /// comms_id → channel to send frames to the target WebSocket
    relays: HashMap<String, mpsc::UnboundedSender<String>>,
}

impl CommsManager {
    pub fn new(ws_tx: mpsc::UnboundedSender<Message>) -> Self {
        Self {
            ws_tx,
            relays: HashMap::new(),
        }
    }

    /// Open a new /comms WebSocket relay to the target URL.
    pub async fn open(&mut self, comms_id: String, target_url: String) {
        info!(comms_id = %comms_id, target_url = %target_url, "Opening /comms relay");

        // Channel for sending frames from backend → target
        let (relay_tx, relay_rx) = mpsc::unbounded_channel::<String>();
        self.relays.insert(comms_id.clone(), relay_tx);

        let ws_tx = self.ws_tx.clone();
        let id = comms_id.clone();

        tokio::spawn(async move {
            if let Err(e) = run_comms_relay(id.clone(), target_url, relay_rx, ws_tx.clone()).await {
                warn!(comms_id = %id, error = %e, "/comms relay error");
                let _ = send_json(&ws_tx, &AgentToServer::CommsError {
                    comms_id: id,
                    error: e.to_string(),
                });
            }
        });
    }

    /// Forward a frame from the backend to the target WebSocket.
    pub fn send_frame(&self, comms_id: &str, data: String) {
        if let Some(tx) = self.relays.get(comms_id) {
            if tx.send(data).is_err() {
                warn!(comms_id = %comms_id, "/comms relay: target channel closed");
            }
        } else {
            warn!(comms_id = %comms_id, "/comms relay: no relay found");
        }
    }

    /// Close a /comms relay.
    pub fn close(&mut self, comms_id: &str) {
        if self.relays.remove(comms_id).is_some() {
            info!(comms_id = %comms_id, "/comms relay closed");
        }
    }

    /// Close all active relays.
    pub fn close_all(&mut self) {
        let count = self.relays.len();
        self.relays.clear();
        if count > 0 {
            info!(count = count, "Closed all /comms relays");
        }
    }
}

/// Run a single /comms WebSocket relay connection.
async fn run_comms_relay(
    comms_id: String,
    target_url: String,
    mut from_backend: mpsc::UnboundedReceiver<String>,
    to_backend: mpsc::UnboundedSender<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (ws_stream, _) = connect_async(&target_url).await?;
    info!(comms_id = %comms_id, "Connected to target /comms");

    // Notify backend that /comms is open
    send_json(&to_backend, &AgentToServer::CommsOpened {
        comms_id: comms_id.clone(),
    })?;

    let (mut ws_write, mut ws_read) = ws_stream.split();

    loop {
        tokio::select! {
            // Target → Backend: relay frames from Node-RED to the backend
            msg = ws_read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        send_json(&to_backend, &AgentToServer::CommsFrame {
                            comms_id: comms_id.clone(),
                            data: text,
                        })?;
                    }
                    Some(Ok(Message::Binary(data))) => {
                        // /comms uses text frames; binary is unexpected but forward as UTF-8
                        if let Ok(text) = String::from_utf8(data) {
                            send_json(&to_backend, &AgentToServer::CommsFrame {
                                comms_id: comms_id.clone(),
                                data: text,
                            })?;
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!(comms_id = %comms_id, "Target /comms closed");
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = ws_write.send(Message::Pong(data)).await;
                    }
                    Some(Err(e)) => {
                        error!(comms_id = %comms_id, error = %e, "Target /comms read error");
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
            // Backend → Target: relay frames from the backend to Node-RED
            frame = from_backend.recv() => {
                match frame {
                    Some(data) => {
                        if let Err(e) = ws_write.send(Message::Text(data)).await {
                            error!(comms_id = %comms_id, error = %e, "Target /comms write error");
                            break;
                        }
                    }
                    None => {
                        // Channel closed — backend closed the relay
                        info!(comms_id = %comms_id, "Backend closed /comms relay channel");
                        break;
                    }
                }
            }
        }
    }

    // Notify backend that /comms is closed
    let _ = send_json(&to_backend, &AgentToServer::CommsClosed {
        comms_id,
    });

    Ok(())
}

fn send_json(
    tx: &mpsc::UnboundedSender<Message>,
    msg: &AgentToServer,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::to_string(msg)?;
    tx.send(Message::Text(json))
        .map_err(|e| format!("Channel send error: {}", e))?;
    Ok(())
}
