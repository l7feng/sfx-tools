// 主仓内可插拔插件模块的最小注册表。三模块逻辑独立、feature 默认全关。
// 只读能力：只做 AAF／OMF 文件头＋字符串启发式探测，以及本机路径／端口／目录的只读状态探测；
// 不解析完整 AAF／OMF 对象模型、不连接真实 Pro Tools、不写任何工程、音频、素材库记录或数据库。
use serde::Serialize;

pub mod adapter;
pub mod sfx_aaf_import;
pub mod sfx_pt_overlay;
pub mod sfx_pt_bouncer;

pub const COMPONENT_EPOCH: u32 = 1;
pub const MIN_APP_VERSION: &str = "0.1.25";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const PLUGIN_AAF_IMPORT: &str = "sfx_aaf_import";
pub const PLUGIN_PT_OVERLAY: &str = "sfx_pt_overlay";
pub const PLUGIN_PT_BOUNCER: &str = "sfx_pt_bouncer";

// 证据标签：静态 &'static str。none 只用于失败／阻断，成功结果不得使用。
pub const EVIDENCE_STATIC_PARSE: &str = "static_parse";
pub const EVIDENCE_LOCAL_PROBE: &str = "local_probe";
pub const EVIDENCE_CONTRACT_ONLY: &str = "contract_only";
pub const EVIDENCE_NONE: &str = "none";

#[derive(Serialize)]
pub struct PluginResult {
    pub plugin_id: &'static str,
    pub command: &'static str,
    pub ok: bool,
    pub code: &'static str,
    pub message: String,
    pub component_epoch: u32,
    pub data: serde_json::Value,
    pub evidence: &'static str,
    pub warnings: Vec<String>,
}

/// 双层开关中的编译层。generate_handler! 宏参数不能逐条加 cfg，
/// 因此命令常驻注册，由 cfg! 在运行时常量位置决定是否放行。
pub fn enabled(plugin_id: &str) -> bool {
    match plugin_id {
        PLUGIN_AAF_IMPORT => cfg!(feature = "plugin-aaf-import"),
        PLUGIN_PT_OVERLAY => cfg!(feature = "plugin-pt-overlay"),
        PLUGIN_PT_BOUNCER => cfg!(feature = "plugin-pt-bouncer"),
        _ => false,
    }
}

fn parse_version(value: &str) -> (u32, u32, u32) {
    let mut parts = value.split('.').map(|item| item.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

pub fn version_at_least(current: &str, min: &str) -> bool {
    parse_version(current) >= parse_version(min)
}

fn result(
    plugin_id: &'static str,
    command: &'static str,
    ok: bool,
    code: &'static str,
    message: &str,
    data: serde_json::Value,
    evidence: &'static str,
    warnings: &[&str],
) -> PluginResult {
    PluginResult {
        plugin_id,
        command,
        ok,
        code,
        message: message.to_string(),
        component_epoch: COMPONENT_EPOCH,
        data,
        evidence,
        warnings: warnings.iter().map(|item| item.to_string()).collect(),
    }
}

pub fn disabled(plugin_id: &'static str, command: &'static str) -> PluginResult {
    result(
        plugin_id,
        command,
        false,
        "PLUGIN_DISABLED",
        "模块未启用：Cargo feature 与设置页开关均为关闭态",
        serde_json::Value::Null,
        "none",
        &["关闭时命令不执行，界面不显示入口"],
    )
}

/// 每条命令执行前的统一闸门：开关 -> 版本门槛。Some 表示应提前返回。
pub fn gate(plugin_id: &'static str, command: &'static str) -> Option<PluginResult> {
    if !enabled(plugin_id) {
        return Some(disabled(plugin_id, command));
    }
    if !version_at_least(APP_VERSION, MIN_APP_VERSION) {
        return Some(result(
            plugin_id,
            command,
            false,
            "MIN_APP_VERSION_UNMET",
            "主程序版本低于 min_app_version",
            serde_json::Value::Null,
            "none",
            &["请升级主程序后重试"],
        ));
    }
    None
}

/// 只读骨架结果。契约禁止 contract_only 下 ok:true 且不带警告，故此处恒带警告。
pub fn contract_only(
    plugin_id: &'static str,
    command: &'static str,
    data: serde_json::Value,
    note: &str,
) -> PluginResult {
    result(
        plugin_id,
        command,
        true,
        "OK",
        "只读回显执行完毕，未接触真实工程",
        data,
        EVIDENCE_CONTRACT_ONLY,
        &[
            "模块 status 为 read_only，此为无真实探测的只读回显、未经真实工程验证",
            "未写任何工程、音频、素材库记录或数据库",
            note,
        ],
    )
}

/// 真实只读能力成功结果：evidence 为真实探测标签，恒带诚实警告，
/// 绝不出现 evidence=none 却 ok=true。
pub fn read_only_ok(
    plugin_id: &'static str,
    command: &'static str,
    message: &str,
    data: serde_json::Value,
    evidence: &'static str,
    notes: &[&str],
) -> PluginResult {
    let mut warnings: Vec<String> = vec![
        "启发式／本机只读探测，未解析完整工程对象模型；未经真实工程验证".to_string(),
        "未连接真实 Pro Tools 会话；本机端口探测（若有）仅为 TCP 连通性、无协议握手、不发送任何数据"
            .to_string(),
        "未写任何工程、音频、素材库记录或数据库；本次为只读探测".to_string(),
    ];
    warnings.extend(notes.iter().map(|item| item.to_string()));
    PluginResult {
        plugin_id,
        command,
        ok: true,
        code: "OK",
        message: message.to_string(),
        component_epoch: COMPONENT_EPOCH,
        data,
        evidence,
        warnings,
    }
}

/// 只读探测失败：ok=false 且 evidence 恒为 none，绝不假装成功。
pub fn probe_error(
    plugin_id: &'static str,
    command: &'static str,
    message: &str,
    data: serde_json::Value,
    note: &str,
) -> PluginResult {
    result(
        plugin_id,
        command,
        false,
        "PROBE_FAILED",
        message,
        data,
        EVIDENCE_NONE,
        &[note],
    )
}

pub fn blocked(plugin_id: &'static str, command: &'static str) -> PluginResult {
    result(
        plugin_id,
        command,
        false,
        "WRITE_BLOCKED",
        "写入类命令默认拒绝：需人类授权、真实测试工程与 allow_write",
        serde_json::Value::Null,
        "none",
        &["未写任何工程、音频或素材库记录"],
    )
}

/// 保留 PT_UNAVAILABLE 契约结果（失败／阻断用 none）；当前只读探测走 read_only_ok，故此结果暂不被调用。
#[allow(dead_code)]
pub fn pt_unavailable(plugin_id: &'static str, command: &'static str) -> PluginResult {
    result(
        plugin_id,
        command,
        false,
        "PT_UNAVAILABLE",
        "未检测到运行中的 Pro Tools 会话；骨架不降级假装成功",
        serde_json::Value::Null,
        "none",
        &["需真实 Pro Tools 环境与测试工程"],
    )
}

pub fn unsupported(plugin_id: &'static str, command: &'static str, what: &str) -> PluginResult {
    result(
        plugin_id,
        command,
        false,
        "INPUT_UNSUPPORTED",
        "输入路径不是绝对路径或格式不受支持",
        serde_json::json!({ "input": what }),
        "none",
        &["所有路径必须是绝对路径"],
    )
}

/// 三份 plugin.toml 必须真实被编译进产物，避免出现无人读取的死文件。
pub fn describe_all() -> Vec<&'static str> {
    vec![
        sfx_aaf_import::META,
        sfx_pt_overlay::META,
        sfx_pt_bouncer::META,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_plugin_is_never_enabled() {
        assert!(!enabled("not_a_plugin"));
    }

    #[test]
    fn version_gate_accepts_current_app() {
        assert!(version_at_least("0.1.26", "0.1.25"));
        assert!(version_at_least("0.1.25", "0.1.25"));
        assert!(!version_at_least("0.1.24", "0.1.25"));
        assert!(version_at_least(APP_VERSION, MIN_APP_VERSION));
    }

    #[test]
    fn three_manifests_are_embedded_and_read_only() {
        let all = describe_all();
        assert_eq!(all.len(), 3);
        for meta in all {
            assert!(meta.contains("status = \"read_only\""));
            assert!(meta.contains("kind = \"in_process_module\""));
        }
    }

    #[test]
    fn write_commands_are_never_ok() {
        for item in [
            sfx_aaf_import::plugin_sfx_aaf_import_to_session(None, Some(true)),
            sfx_pt_overlay::plugin_sfx_pt_overlay_spot(None, None, None, Some(true)),
            sfx_pt_bouncer::plugin_sfx_pt_bouncer_run(None, Some(true)),
        ] {
            assert!(!item.ok);
            assert!(item.code == "PLUGIN_DISABLED" || item.code == "WRITE_BLOCKED");
            assert_eq!(item.evidence, "none");
        }
    }

    #[test]
    fn contract_only_results_always_carry_warnings() {
        for item in [
            sfx_aaf_import::plugin_sfx_aaf_import_parse(None, None, None),
            sfx_pt_bouncer::plugin_sfx_pt_bouncer_preview(None, None),
            sfx_pt_bouncer::plugin_sfx_pt_bouncer_cancel(None),
        ] {
            if item.ok {
                assert_ne!(item.evidence, "none");
                assert!(!item.warnings.is_empty());
            }
        }
    }
}
