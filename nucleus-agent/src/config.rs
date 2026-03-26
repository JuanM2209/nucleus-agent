use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub server: ServerConfig,
    pub heartbeat: HeartbeatConfig,
    pub discovery: DiscoveryConfig,
    pub tunnel: TunnelConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatConfig {
    #[serde(default = "default_heartbeat_interval")]
    pub interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveryConfig {
    #[serde(default = "default_passive_interval")]
    pub passive_interval_secs: u64,
    #[serde(default)]
    pub active_enabled: bool,
    #[serde(default = "default_arp_rate")]
    pub arp_rate_pps: u32,
    #[serde(default = "default_scan_ports")]
    pub port_scan_ports: Vec<u16>,
    #[serde(default = "default_scan_timeout")]
    pub port_scan_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TunnelConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

fn default_heartbeat_interval() -> u64 { 30 }
fn default_passive_interval() -> u64 { 300 }
fn default_arp_rate() -> u32 { 2 }
fn default_scan_ports() -> Vec<u16> {
    vec![22, 80, 443, 502, 1880, 4840, 9090, 44818, 47808]
}
fn default_scan_timeout() -> u64 { 2000 }
fn default_max_concurrent() -> usize { 10 }

impl AgentConfig {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }
}
