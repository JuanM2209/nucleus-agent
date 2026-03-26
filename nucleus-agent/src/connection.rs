use crate::config::AgentConfig;
use crate::comms::CommsManager;
use crate::tunnel::TunnelManager;
use futures_util::{SinkExt, StreamExt};
use nucleus_common::messages::{AgentToServer, ServerToAgent};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn, error};
use std::time::Duration;

const MAX_BACKOFF_SECS: u64 = 60;

/// Main connection loop with automatic reconnection and exponential backoff.
pub async fn run(config: AgentConfig) {
    let mut backoff_secs: u64 = 1;

    loop {
        info!("Connecting to {}...", config.server.url);

        match connect_and_run(&config).await {
            Ok(()) => {
                info!("Connection closed gracefully");
                backoff_secs = 1;
            }
            Err(e) => {
                error!("Connection error: {}", e);
            }
        }

        warn!("Reconnecting in {} seconds...", backoff_secs);
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;

        // Exponential backoff with jitter
        backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
        let jitter = rand_jitter(backoff_secs);
        backoff_secs = backoff_secs.saturating_add(jitter);
    }
}

async fn connect_and_run(config: &AgentConfig) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}?token={}", config.server.url, config.server.token);

    let (ws_stream, _response) = connect_async(&url).await?;
    info!("Connected to control plane");

    let (ws_write, mut ws_read) = ws_stream.split();

    // Create an unbounded channel for outbound WebSocket messages.
    // All tasks (heartbeat, tunnel reader, main loop) send through this
    // channel, and the writer task drains it to the WS write half.
    let (tx, rx) = mpsc::unbounded_channel::<Message>();

    // Create managers
    let mut tunnel_mgr = TunnelManager::new(tx.clone());
    let mut comms_mgr = CommsManager::new(tx.clone());

    // Writer task: drains the channel and sends messages through the WS
    let writer_handle = tokio::spawn(ws_writer_task(ws_write, rx));

    // Heartbeat task: periodically sends health metrics
    let hb_tx = tx.clone();
    let hb_interval = config.heartbeat.interval_secs;
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(hb_interval));
        loop {
            interval.tick().await;
            let heartbeat = crate::health::collect_heartbeat();
            match serde_json::to_string(&heartbeat) {
                Ok(json) => {
                    if hb_tx.send(Message::Text(json)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to serialize heartbeat: {}", e);
                }
            }
        }
    });

    // Main read loop: dispatch incoming WS messages
    while let Some(msg) = ws_read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                error!("WebSocket read error: {}", e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                handle_text_message(&text, &mut tunnel_mgr, &mut comms_mgr, &tx).await;
            }
            Message::Binary(data) => {
                handle_binary_frame(&data, &mut tunnel_mgr).await;
            }
            Message::Ping(data) => {
                let _ = tx.send(Message::Pong(data));
            }
            Message::Close(_) => {
                info!("Server closed connection");
                break;
            }
            _ => {}
        }
    }

    // Clean up
    heartbeat_handle.abort();
    comms_mgr.close_all();
    tunnel_mgr.close_all().await;
    // Drop tx so the writer task's rx completes
    drop(tx);
    // Wait briefly for the writer to flush remaining messages
    let _ = tokio::time::timeout(Duration::from_secs(2), writer_handle).await;

    Ok(())
}

/// Writer task: reads from the channel and sends to the WebSocket write half.
async fn ws_writer_task(
    mut ws_write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    mut rx: mpsc::UnboundedReceiver<Message>,
) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = ws_write.send(msg).await {
            error!("WebSocket write error: {}", e);
            break;
        }
    }
}

/// Dispatch a text (JSON) message from the server.
async fn handle_text_message(
    text: &str,
    tunnel_mgr: &mut TunnelManager,
    comms_mgr: &mut CommsManager,
    tx: &mpsc::UnboundedSender<Message>,
) {
    match serde_json::from_str::<ServerToAgent>(text) {
        Ok(ServerToAgent::SessionOpen {
            session_id,
            target_ip,
            target_port,
            stream_id,
        }) => {
            tunnel_mgr
                .handle_session_open(session_id, target_ip, target_port, stream_id)
                .await;
        }
        Ok(ServerToAgent::SessionClose { session_id }) => {
            tunnel_mgr.handle_session_close(&session_id).await;
        }
        Ok(ServerToAgent::DiscoveryTrigger { adapter_id, scan_type }) => {
            info!(
                adapter_id = ?adapter_id,
                scan_type = %scan_type,
                "Discovery trigger received (not yet implemented)"
            );
        }
        Ok(ServerToAgent::Ping) => {
            let pong = AgentToServer::Pong;
            if let Ok(json) = serde_json::to_string(&pong) {
                let _ = tx.send(Message::Text(json));
            }
        }
        // /comms WebSocket relay
        Ok(ServerToAgent::CommsOpen { payload }) => {
            comms_mgr.open(payload.comms_id, payload.target_url).await;
        }
        Ok(ServerToAgent::CommsFrame { payload }) => {
            comms_mgr.send_frame(&payload.comms_id, payload.data);
        }
        Ok(ServerToAgent::CommsClose { payload }) => {
            comms_mgr.close(&payload.comms_id);
        }
        Err(e) => {
            warn!("Failed to parse control message: {} — raw: {}", e, text);
        }
    }
}

/// Dispatch a binary frame from the server.
///
/// Binary frame format: `[4B streamId BE][4B length BE][payload]`
/// - streamId 0 = control frame (JSON SYN/FIN/RST)
/// - streamId > 0 = tunnel data
async fn handle_binary_frame(data: &[u8], tunnel_mgr: &mut TunnelManager) {
    if data.len() < 8 {
        warn!("Binary frame too short: {} bytes", data.len());
        return;
    }

    let stream_id = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let _length = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let payload = &data[8..];

    if stream_id == 0 {
        // Control frame: parse JSON and dispatch
        match serde_json::from_slice::<serde_json::Value>(payload) {
            Ok(cmd) => {
                tunnel_mgr.handle_control(&cmd).await;
            }
            Err(e) => {
                warn!("Failed to parse control frame JSON: {}", e);
            }
        }
    } else {
        // Tunnel data: forward to TCP socket
        tunnel_mgr.handle_data(stream_id, payload).await;
    }
}

fn rand_jitter(max: u64) -> u64 {
    // Simple jitter without requiring rand crate
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    t % (max / 4 + 1)
}
