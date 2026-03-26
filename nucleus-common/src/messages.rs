use serde::{Deserialize, Serialize};
use crate::types::{AdapterInfo, DiscoveredEndpointInfo};

// ── Server → Agent ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerToAgent {
    #[serde(rename = "session.open")]
    SessionOpen {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "targetIp")]
        target_ip: String,
        #[serde(rename = "targetPort")]
        target_port: u16,
        #[serde(rename = "streamId")]
        stream_id: u32,
    },
    #[serde(rename = "session.close")]
    SessionClose {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    #[serde(rename = "discovery.trigger")]
    DiscoveryTrigger {
        #[serde(rename = "adapterId")]
        adapter_id: Option<String>,
        #[serde(rename = "scanType")]
        scan_type: String,
    },
    #[serde(rename = "ping")]
    Ping,

    // /comms WebSocket relay (browser → agent → device Node-RED /comms)
    #[serde(rename = "comms_open")]
    CommsOpen {
        #[serde(alias = "payload")]
        payload: CommsOpenPayload,
    },
    #[serde(rename = "comms_frame")]
    CommsFrame {
        #[serde(alias = "payload")]
        payload: CommsFramePayload,
    },
    #[serde(rename = "comms_close")]
    CommsClose {
        #[serde(alias = "payload")]
        payload: CommsClosePayload,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommsOpenPayload {
    pub comms_id: String,
    pub target_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommsFramePayload {
    pub comms_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommsClosePayload {
    pub comms_id: String,
}

// ── Agent → Server ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentToServer {
    #[serde(rename = "heartbeat")]
    Heartbeat {
        cpu: f32,
        mem: u64,
        #[serde(rename = "memTotal")]
        mem_total: u64,
        disk: u64,
        #[serde(rename = "diskTotal")]
        disk_total: u64,
        uptime: u64,
        #[serde(rename = "agentVersion")]
        agent_version: String,
        #[serde(rename = "activeTunnels")]
        active_tunnels: u32,
        adapters: Vec<AdapterInfo>,
    },
    #[serde(rename = "session.ready")]
    SessionReady {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "streamId")]
        stream_id: u32,
    },
    #[serde(rename = "session.error")]
    SessionError {
        #[serde(rename = "sessionId")]
        session_id: String,
        error: String,
    },
    #[serde(rename = "session.closed")]
    SessionClosed {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "bytesTx")]
        bytes_tx: u64,
        #[serde(rename = "bytesRx")]
        bytes_rx: u64,
    },
    #[serde(rename = "discovery.result")]
    DiscoveryResult {
        #[serde(rename = "adapterId")]
        adapter_id: String,
        #[serde(rename = "adapterName")]
        adapter_name: String,
        endpoints: Vec<DiscoveredEndpointInfo>,
    },
    #[serde(rename = "pong")]
    Pong,

    // /comms WebSocket relay responses
    #[serde(rename = "comms_opened")]
    CommsOpened {
        comms_id: String,
    },
    #[serde(rename = "comms_frame")]
    CommsFrame {
        comms_id: String,
        data: String,
    },
    #[serde(rename = "comms_closed")]
    CommsClosed {
        comms_id: String,
    },
    #[serde(rename = "comms_error")]
    CommsError {
        comms_id: String,
        error: String,
    },
}
