use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn, error};

const TCP_BUF_SIZE: usize = 32 * 1024; // 32 KB
const TCP_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Manages active tunnel sessions, bridging TCP connections to the WebSocket.
pub struct TunnelManager {
    sessions: HashMap<String, TunnelSession>,
    stream_to_session: HashMap<u32, String>,
    tx: mpsc::UnboundedSender<Message>,
}

struct TunnelSession {
    session_id: String,
    stream_id: u32,
    tcp_writer: Option<tokio::io::WriteHalf<TcpStream>>,
    reader_handle: Option<JoinHandle<()>>,
    bytes_tx: u64,
    bytes_rx: u64,
}

impl TunnelManager {
    pub fn new(tx: mpsc::UnboundedSender<Message>) -> Self {
        Self {
            sessions: HashMap::new(),
            stream_to_session: HashMap::new(),
            tx,
        }
    }

    /// Returns the number of currently active tunnel sessions.
    pub fn active_count(&self) -> u32 {
        self.sessions.len() as u32
    }

    /// Handle a `session.open` command from the backend.
    ///
    /// Connects to `target_ip:target_port` via TCP with a 5-second timeout,
    /// spawns a reader task that forwards TCP data as binary WS frames,
    /// and sends `session.ready` or `session.error` back through the channel.
    pub async fn handle_session_open(
        &mut self,
        session_id: String,
        target_ip: String,
        target_port: u16,
        stream_id: u32,
    ) {
        info!(
            session_id = %session_id,
            target = %format!("{}:{}", target_ip, target_port),
            stream_id = stream_id,
            "Opening tunnel session"
        );

        // Prevent duplicate sessions
        if self.sessions.contains_key(&session_id) {
            warn!(session_id = %session_id, "Session already exists, ignoring duplicate open");
            return;
        }

        // Connect to the local target with a timeout
        let addr = format!("{}:{}", target_ip, target_port);
        let tcp_stream = match tokio::time::timeout(
            std::time::Duration::from_secs(TCP_CONNECT_TIMEOUT_SECS),
            TcpStream::connect(&addr),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => {
                error!(session_id = %session_id, addr = %addr, error = %e, "TCP connect failed");
                self.send_session_error(&session_id, &format!("TCP connect failed: {}", e));
                return;
            }
            Err(_) => {
                error!(session_id = %session_id, addr = %addr, "TCP connect timed out");
                self.send_session_error(&session_id, "TCP connect timed out (5s)");
                return;
            }
        };

        info!(session_id = %session_id, addr = %addr, "TCP connected");

        let (tcp_read, tcp_write) = tokio::io::split(tcp_stream);

        // Spawn the TCP reader task
        let reader_tx = self.tx.clone();
        let reader_session_id = session_id.clone();
        let reader_handle = tokio::spawn(tcp_reader_task(
            tcp_read,
            stream_id,
            reader_session_id,
            reader_tx,
        ));

        // Store the session
        let session = TunnelSession {
            session_id: session_id.clone(),
            stream_id,
            tcp_writer: Some(tcp_write),
            reader_handle: Some(reader_handle),
            bytes_tx: 0,
            bytes_rx: 0,
        };

        self.stream_to_session.insert(stream_id, session_id.clone());
        self.sessions.insert(session_id.clone(), session);

        // Notify the backend that the session is ready
        self.send_session_ready(&session_id, stream_id);
    }

    /// Handle a `session.close` command from the backend.
    ///
    /// Closes the TCP connection, aborts the reader task, and sends
    /// `session.closed` with byte counts.
    pub async fn handle_session_close(&mut self, session_id: &str) {
        info!(session_id = %session_id, "Closing tunnel session");

        let Some(mut session) = self.sessions.remove(session_id) else {
            warn!(session_id = %session_id, "Session not found for close");
            return;
        };

        self.stream_to_session.remove(&session.stream_id);

        // Drop the TCP writer (closes the write half)
        session.tcp_writer.take();

        // Abort the reader task
        if let Some(handle) = session.reader_handle.take() {
            handle.abort();
        }

        self.send_session_closed(&session.session_id, session.bytes_tx, session.bytes_rx);
    }

    /// Handle incoming binary data from the backend destined for a TCP socket.
    ///
    /// The payload is written to the TCP socket associated with `stream_id`.
    pub async fn handle_data(&mut self, stream_id: u32, payload: &[u8]) {
        let Some(session_id) = self.stream_to_session.get(&stream_id) else {
            warn!(stream_id = stream_id, "No session for stream, dropping data");
            return;
        };

        let session_id = session_id.clone();
        let Some(session) = self.sessions.get_mut(&session_id) else {
            warn!(session_id = %session_id, "Session not found, dropping data");
            return;
        };

        session.bytes_rx += payload.len() as u64;

        let Some(writer) = session.tcp_writer.as_mut() else {
            warn!(session_id = %session_id, "TCP writer gone, dropping data");
            return;
        };

        if let Err(e) = writer.write_all(payload).await {
            error!(
                session_id = %session_id,
                error = %e,
                "Failed to write to TCP, closing session"
            );
            // Close the session on write failure
            self.close_session_on_error(&session_id).await;
        }
    }

    /// Handle a control frame (streamId 0) from the backend.
    ///
    /// Control frames are JSON payloads with `cmd` and `streamId` fields.
    /// Supported commands: SYN, FIN, RST.
    pub async fn handle_control(&mut self, cmd: &serde_json::Value) {
        let Some(cmd_str) = cmd.get("cmd").and_then(|v| v.as_str()) else {
            warn!("Control frame missing 'cmd' field");
            return;
        };

        let stream_id = cmd
            .get("streamId")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        match cmd_str {
            "SYN" => {
                // SYN is handled via session.open text message, but if it arrives
                // as a control frame, log and ignore.
                info!(stream_id = stream_id, "Received SYN control frame (no-op)");
            }
            "FIN" => {
                info!(stream_id = stream_id, "Received FIN control frame");
                if let Some(session_id) = self.stream_to_session.get(&stream_id).cloned() {
                    self.handle_session_close(&session_id).await;
                }
            }
            "RST" => {
                info!(stream_id = stream_id, "Received RST control frame");
                if let Some(session_id) = self.stream_to_session.get(&stream_id).cloned() {
                    self.handle_session_close(&session_id).await;
                }
            }
            other => {
                warn!(cmd = other, "Unknown control frame command");
            }
        }
    }

    /// Close all active sessions. Called on WebSocket disconnect.
    pub async fn close_all(&mut self) {
        let session_ids: Vec<String> = self.sessions.keys().cloned().collect();
        for session_id in session_ids {
            self.handle_session_close(&session_id).await;
        }
    }

    // ── Private helpers ──

    /// Close a session due to a TCP write error.
    async fn close_session_on_error(&mut self, session_id: &str) {
        let session_id = session_id.to_string();
        self.handle_session_close(&session_id).await;
    }

    fn send_session_ready(&self, session_id: &str, stream_id: u32) {
        let msg = nucleus_common::messages::AgentToServer::SessionReady {
            session_id: session_id.to_string(),
            stream_id,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = self.tx.send(Message::Text(json));
        }
    }

    fn send_session_error(&self, session_id: &str, error: &str) {
        let msg = nucleus_common::messages::AgentToServer::SessionError {
            session_id: session_id.to_string(),
            error: error.to_string(),
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = self.tx.send(Message::Text(json));
        }
    }

    fn send_session_closed(&self, session_id: &str, bytes_tx: u64, bytes_rx: u64) {
        let msg = nucleus_common::messages::AgentToServer::SessionClosed {
            session_id: session_id.to_string(),
            bytes_tx,
            bytes_rx,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = self.tx.send(Message::Text(json));
        }
    }
}

// ── Binary frame helpers ──

/// Build a binary frame: `[4B stream_id BE][4B length BE][payload]`
fn build_frame(stream_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(&stream_id.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Build a control frame (stream_id 0) containing a JSON command.
fn build_control_frame(cmd: &str, stream_id: u32) -> Vec<u8> {
    let json = format!(r#"{{"cmd":"{}","streamId":{}}}"#, cmd, stream_id);
    build_frame(0, json.as_bytes())
}

// ── TCP reader task ──

/// Reads from the TCP socket in a loop and sends binary frames through the
/// WebSocket channel. On EOF or error, sends a FIN control frame and a
/// `session.closed` JSON message.
async fn tcp_reader_task(
    mut tcp_read: tokio::io::ReadHalf<TcpStream>,
    stream_id: u32,
    session_id: String,
    tx: mpsc::UnboundedSender<Message>,
) {
    let mut buf = vec![0u8; TCP_BUF_SIZE];
    let mut bytes_tx: u64 = 0;

    loop {
        match tcp_read.read(&mut buf).await {
            Ok(0) => {
                // EOF — TCP connection closed by the remote
                info!(
                    session_id = %session_id,
                    stream_id = stream_id,
                    bytes_tx = bytes_tx,
                    "TCP read EOF"
                );
                break;
            }
            Ok(n) => {
                bytes_tx += n as u64;
                let frame = build_frame(stream_id, &buf[..n]);
                if tx.send(Message::Binary(frame)).is_err() {
                    // WS channel closed; stop reading
                    warn!(
                        session_id = %session_id,
                        "WS channel closed, stopping TCP reader"
                    );
                    return;
                }
            }
            Err(e) => {
                error!(
                    session_id = %session_id,
                    stream_id = stream_id,
                    error = %e,
                    "TCP read error"
                );
                break;
            }
        }
    }

    // Send FIN control frame to notify the backend the stream is done
    let fin_frame = build_control_frame("FIN", stream_id);
    let _ = tx.send(Message::Binary(fin_frame));

    // Send session.closed JSON message with byte counts
    // Note: bytes_rx is not tracked here (the writer side tracks it),
    // so we send 0 for bytes_rx. The TunnelManager may send a more
    // accurate session.closed if it processes the close first.
    let closed_msg = nucleus_common::messages::AgentToServer::SessionClosed {
        session_id,
        bytes_tx,
        bytes_rx: 0,
    };
    if let Ok(json) = serde_json::to_string(&closed_msg) {
        let _ = tx.send(Message::Text(json));
    }
}
