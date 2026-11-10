// PT 集成叠层只读模块：只做本机安装标记／环境变量／回环端口的只读探测，不连接真实 Pro Tools 会话。
use super::{
    blocked, gate, read_only_ok, PluginResult, EVIDENCE_LOCAL_PROBE, PLUGIN_PT_OVERLAY as ID,
};
use serde::Serialize;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

pub const META: &str = include_str!("plugin.toml");

// PTSL 常用本机回环端口；只探测 127.0.0.1，超时 100ms，不发起任何协议握手、不发送任何数据。
const PTSL_PORTS: [u16; 2] = [9999, 25600];
const CONNECT_TIMEOUT: Duration = Duration::from_millis(100);

// 常见 Pro Tools 安装标记（只读存在性检查，不启动任何进程）。
const INSTALL_MARKERS: [&str; 4] = [
    "C:\\Program Files\\Avid\\Pro Tools",
    "C:\\Program Files (x86)\\Avid\\Pro Tools",
    "/Applications/Avid/Pro Tools",
    "/opt/Avid/Pro Tools",
];

// 可能指向 Pro Tools 安装的环境变量（只读读取）。
const PT_ENV_VARS: [&str; 3] = ["PT_HOME", "AVID_PT_HOME", "PROTOOLS_PATH"];

#[derive(Serialize)]
struct ProbeReport {
    install_marker_found: bool,
    env_marker_found: bool,
    ptsl_port_open: bool,
    open_ports: Vec<u16>,
    probe_method: &'static str,
    checked_paths: Vec<String>,
    checked_env_vars: Vec<String>,
    checked_ports: Vec<u16>,
    loopback_only: bool,
    connect_timeout_ms: u64,
}

/// 本机只读探测：路径／环境变量存在性 + 回环端口 TCP 连通性。
/// 不连接 Pro Tools 会话、不发起 PTSL 协议握手、不发送任何数据、不启动任何进程。
fn probe_local() -> ProbeReport {
    let mut checked_paths: Vec<String> = Vec::new();
    let mut install_marker_found = false;
    for marker in INSTALL_MARKERS {
        checked_paths.push(marker.to_string());
        if Path::new(marker).exists() {
            install_marker_found = true;
        }
    }

    let mut checked_env_vars: Vec<String> = Vec::new();
    let mut env_marker_found = false;
    for key in PT_ENV_VARS {
        checked_env_vars.push(key.to_string());
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                env_marker_found = true;
                let candidate = format!("env:{}={}", key, value);
                checked_paths.push(candidate.clone());
                if Path::new(value.trim()).exists() {
                    install_marker_found = true;
                }
            }
        }
    }

    let mut checked_ports: Vec<u16> = Vec::new();
    let mut open_ports: Vec<u16> = Vec::new();
    for port in PTSL_PORTS {
        checked_ports.push(port);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        // 仅 TCP 连通性探测；连接成功后立即丢弃 stream，不读写任何字节。
        if TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_ok() {
            open_ports.push(port);
        }
    }

    ProbeReport {
        install_marker_found,
        env_marker_found,
        ptsl_port_open: !open_ports.is_empty(),
        open_ports,
        probe_method: "local_probe",
        checked_paths,
        checked_env_vars,
        checked_ports,
        loopback_only: true,
        connect_timeout_ms: CONNECT_TIMEOUT.as_millis() as u64,
    }
}

#[tauri::command]
pub fn plugin_sfx_pt_overlay_capabilities() -> PluginResult {
    if let Some(early) = gate(ID, "capabilities") {
        return early;
    }
    let report = probe_local();
    let found = report.install_marker_found || report.ptsl_port_open;
    let message = if found {
        "本机只读探测完成：发现 Pro Tools 相关标记，但未连接任何会话"
    } else {
        "本机只读探测完成：未发现安装标记或开放的 PTSL 端口，未连接任何会话"
    };
    read_only_ok(
        ID,
        "capabilities",
        message,
        json!(report),
        EVIDENCE_LOCAL_PROBE,
        &[
            "仅做安装路径／环境变量／回环端口的只读存在性与连通性探测；探测结果不代表已连接真实 Pro Tools，也不能证明 PT 一定可用",
        ],
    )
}

/// 素材落点属写入类，read_only 状态下一律阻断。
#[tauri::command]
pub fn plugin_sfx_pt_overlay_spot(
    asset_ids: Option<Vec<String>>,
    timecode: Option<String>,
    track: Option<String>,
    allow_write: Option<bool>,
) -> PluginResult {
    if let Some(early) = gate(ID, "spot") {
        return early;
    }
    let _ = (asset_ids, timecode, track, allow_write);
    blocked(ID, "spot")
}

#[tauri::command]
pub fn plugin_sfx_pt_overlay_status() -> PluginResult {
    if let Some(early) = gate(ID, "status") {
        return early;
    }
    let report = probe_local();
    read_only_ok(
        ID,
        "status",
        "本机只读状态探测完成（未连接真实 Pro Tools 会话）",
        json!(report),
        EVIDENCE_LOCAL_PROBE,
        &[
            "状态仅来自本机只读探测；未连接 Pro Tools、未读取任何工程内容、未发起 PTSL 协议握手",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_probe_is_self_contained_and_feature_independent() {
        // 端口探测允许失败（connect_timeout 100ms），不依赖真实 PT 或网络可达性。
        let report = probe_local();
        assert_eq!(report.probe_method, "local_probe");
        assert!(!report.checked_paths.is_empty());
        assert_eq!(report.checked_ports.len(), PTSL_PORTS.len());
        assert!(report.loopback_only);
        assert_eq!(report.connect_timeout_ms, 100);
    }

    #[test]
    fn capabilities_and_status_report_local_probe() {
        for result in [
            plugin_sfx_pt_overlay_capabilities(),
            plugin_sfx_pt_overlay_status(),
        ] {
            if cfg!(feature = "plugin-pt-overlay") {
                assert!(result.ok);
                assert_eq!(result.evidence, EVIDENCE_LOCAL_PROBE);
                assert!(!result.warnings.is_empty());
                assert!(result.data.get("install_marker_found").is_some());
                assert!(result.data.get("ptsl_port_open").is_some());
                assert!(result.data.get("probe_method").is_some());
                assert!(result.data.get("checked_paths").is_some());
            } else {
                assert!(!result.ok);
                assert_eq!(result.code, "PLUGIN_DISABLED");
            }
        }
    }

    #[test]
    fn spot_is_always_write_blocked() {
        let result = plugin_sfx_pt_overlay_spot(None, None, None, Some(true));
        assert!(!result.ok);
        assert!(result.code == "WRITE_BLOCKED" || result.code == "PLUGIN_DISABLED");
        assert_eq!(result.evidence, "none");
        if cfg!(feature = "plugin-pt-overlay") {
            assert_eq!(result.code, "WRITE_BLOCKED");
        }
    }
}
