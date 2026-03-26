use nucleus_common::messages::AgentToServer;
use nucleus_common::types::AdapterInfo;
use sysinfo::System;

pub fn collect_heartbeat() -> AgentToServer {
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu = sys.global_cpu_usage();
    let mem = sys.used_memory();
    let mem_total = sys.total_memory();

    // Disk usage (root partition)
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let (disk, disk_total) = disks
        .iter()
        .find(|d| d.mount_point() == std::path::Path::new("/"))
        .map(|d| (d.total_space() - d.available_space(), d.total_space()))
        .unwrap_or((0, 0));

    let uptime = System::uptime();

    AgentToServer::Heartbeat {
        cpu,
        mem,
        mem_total,
        disk,
        disk_total,
        uptime,
        agent_version: format!("vr{}", env!("CARGO_PKG_VERSION").split('.').nth(1).unwrap_or("19")),
        active_tunnels: 0, // TODO: Track from tunnel manager
        adapters: collect_adapters(),
    }
}

fn collect_adapters() -> Vec<AdapterInfo> {
    let mut adapters = Vec::new();
    // Scan /sys/class/net/ for all physical interfaces (skip lo, docker, veth)
    let net_dir = "/sys/class/net";
    let entries = match std::fs::read_dir(net_dir) {
        Ok(e) => e,
        Err(_) => return adapters,
    };

    // Build a map of interface-name → ipv4 method from NetworkManager configs
    let nm_methods = read_nm_methods();

    for entry in entries.flatten() {
        let iface = entry.file_name().to_string_lossy().to_string();

        // Skip virtual interfaces
        if iface == "lo"
            || iface.starts_with("veth")
            || iface.starts_with("br-")
            || iface.starts_with("docker")
            || iface.starts_with("wwan")  // Cellular WAN — not used for device networking
        {
            continue;
        }

        let path = format!("{}/{}", net_dir, iface);

        let is_up = std::fs::read_to_string(format!("{}/operstate", path))
            .map(|s| s.trim() == "up")
            .unwrap_or(false);

        let mac = std::fs::read_to_string(format!("{}/address", path))
            .map(|s| {
                let m = s.trim().to_string();
                if m == "00:00:00:00:00:00" { None } else { Some(m) }
            })
            .unwrap_or(None);

        // Get IP address from `ip addr show <iface>` output
        let (ip_address, subnet_mask, gateway) = read_ip_info(&iface);

        // Determine address mode from NetworkManager config (NOT from profile name)
        let mode = nm_methods.get(&iface).cloned();

        adapters.push(AdapterInfo {
            name: iface,
            mac_address: mac,
            ip_address,
            subnet_mask,
            gateway,
            mode,
            is_up,
        });
    }

    adapters
}

/// Read NetworkManager system-connections to determine the real ipv4.method
/// for each interface. This reads the actual config files, NOT the profile name.
///
/// /etc/NetworkManager/system-connections/*.nmconnection or the keyfile format
/// Looks for:
///   [connection]
///   interface-name=eth0
///   [ipv4]
///   method=manual    → "Static"
///   method=auto      → "DHCP"
fn read_nm_methods() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let nm_path = "/etc/NetworkManager/system-connections";

    let entries = match std::fs::read_dir(nm_path) {
        Ok(e) => e,
        Err(_) => return map,
    };

    for entry in entries.flatten() {
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut iface_name: Option<String> = None;
        let mut ipv4_method: Option<String> = None;
        let mut in_connection = false;
        let mut in_ipv4 = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[connection]" {
                in_connection = true;
                in_ipv4 = false;
                continue;
            }
            if trimmed == "[ipv4]" {
                in_ipv4 = true;
                in_connection = false;
                continue;
            }
            if trimmed.starts_with('[') {
                in_connection = false;
                in_ipv4 = false;
                continue;
            }

            if in_connection {
                if let Some(val) = trimmed.strip_prefix("interface-name=") {
                    iface_name = Some(val.to_string());
                }
            }
            if in_ipv4 {
                if let Some(val) = trimmed.strip_prefix("method=") {
                    ipv4_method = Some(match val {
                        "manual" => "Static".to_string(),
                        "auto" => "DHCP".to_string(),
                        "shared" => "Shared".to_string(),
                        "link-local" => "Link-Local".to_string(),
                        "disabled" => "Disabled".to_string(),
                        other => other.to_string(),
                    });
                }
            }
        }

        if let (Some(iface), Some(method)) = (iface_name, ipv4_method) {
            map.insert(iface, method);
        }
    }

    map
}

/// Read IP info for an interface using /proc/net or `ip` command parsing
fn read_ip_info(iface: &str) -> (Option<String>, Option<String>, Option<String>) {
    // Try reading from `ip -4 addr show <iface>` output
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show", iface])
        .output();

    let ip_address = match &output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Format: "2: eth0    inet 10.10.1.1/24 brd 10.10.1.255 scope global eth0"
            stdout.split_whitespace()
                .find(|s| s.contains('/'))
                .and_then(|cidr| cidr.split('/').next())
                .map(|s| s.to_string())
        }
        Err(_) => None,
    };

    let subnet_mask = match &output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.split_whitespace()
                .find(|s| s.contains('/'))
                .and_then(|cidr| cidr.split('/').nth(1))
                .and_then(|bits| cidr_to_mask(bits.parse().ok()?))
        }
        Err(_) => None,
    };

    // Gateway from `ip route show dev <iface>`
    let gateway = std::process::Command::new("ip")
        .args(["route", "show", "dev", iface])
        .output()
        .ok()
        .and_then(|out| {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.starts_with("default via") {
                    return line.split_whitespace().nth(2).map(|s| s.to_string());
                }
            }
            None
        });

    (ip_address, subnet_mask, gateway)
}

/// Convert CIDR prefix length to subnet mask string
fn cidr_to_mask(prefix: u8) -> Option<String> {
    if prefix > 32 { return None; }
    let mask: u32 = if prefix == 0 { 0 } else { !0u32 << (32 - prefix) };
    Some(format!(
        "{}.{}.{}.{}",
        (mask >> 24) & 0xFF,
        (mask >> 16) & 0xFF,
        (mask >> 8) & 0xFF,
        mask & 0xFF,
    ))
}
