// 插件适配器层：把「宿主 ↔ 三个 in_process 模块」的调用收敛到统一接口。
//
// 边界（与 02-接入契约.md 一致）：不加载外部二进制／动态库，不运行任何 EXE，
// 不连接真实 Pro Tools 会话、不解析完整 AAF／OMF 对象模型、不写任何工程、音频或素材库记录。
// 只读命令允许做文件头启发式探测与本机路径／端口／目录的只读状态探测。
// 适配器只做：能力协商、双层开关、参数归一、命令分发、结果归一。
use super::{
    blocked, describe_all, disabled, enabled, unsupported, version_at_least, PluginResult,
    APP_VERSION, COMPONENT_EPOCH, MIN_APP_VERSION, PLUGIN_AAF_IMPORT, PLUGIN_PT_BOUNCER,
    PLUGIN_PT_OVERLAY,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

pub const ADAPTER_EPOCH: u32 = 1;

/// 供前端读取的适配器描述。compiled 是编译层 feature 状态，runtime_enabled 是偏好层状态。
#[derive(Clone, Copy, Serialize)]
pub struct AdapterDescriptor {
    pub plugin_id: &'static str,
    pub kind: &'static str,
    pub status: &'static str,
    pub feature: &'static str,
    pub compiled: bool,
    pub runtime_enabled: bool,
    pub requires_pt: bool,
    pub capabilities: &'static [&'static str],
    pub read_only_commands: &'static [&'static str],
    pub write_commands: &'static [&'static str],
    pub adapter_epoch: u32,
}

/// 运行时偏好开关。默认全关；由宿主在挂载时通过 plugin_adapter_set_flags 同步一次。
/// 只是开关值的同步，不等于新增了通用插件宿主。
#[derive(Clone, Copy)]
struct RuntimeFlags {
    aaf: bool,
    overlay: bool,
    bouncer: bool,
}

static RUNTIME_FLAGS: OnceLock<Mutex<RuntimeFlags>> = OnceLock::new();

fn flags() -> &'static Mutex<RuntimeFlags> {
    RUNTIME_FLAGS.get_or_init(|| {
        Mutex::new(RuntimeFlags {
            aaf: false,
            overlay: false,
            bouncer: false,
        })
    })
}

pub fn set_runtime_flags(aaf: bool, overlay: bool, bouncer: bool) {
    if let Ok(mut guard) = flags().lock() {
        *guard = RuntimeFlags {
            aaf,
            overlay,
            bouncer,
        };
    }
}

pub fn runtime_enabled(plugin_id: &str) -> bool {
    let guard = match flags().lock() {
        Ok(item) => item,
        Err(_) => return false,
    };
    match plugin_id {
        PLUGIN_AAF_IMPORT => guard.aaf,
        PLUGIN_PT_OVERLAY => guard.overlay,
        PLUGIN_PT_BOUNCER => guard.bouncer,
        _ => false,
    }
}

/// 适配器契约：每个模块只暴露 id、能力集与一个统一分发函数。
/// 模块内部实现仍保留九个 Tauri 直连命令，适配器是新增收敛入口，不替换它们。
pub trait PluginAdapter {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> &'static [&'static str];
    fn dispatch(&self, command: &'static str, params: &Value) -> PluginResult;
}

struct AafImportAdapter;
struct PtOverlayAdapter;
struct PtBouncerAdapter;

fn opt_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(|item| item.to_string())
}

fn opt_bool(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}

fn opt_str_vec(params: &Value, key: &str) -> Option<Vec<String>> {
    params.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(|item| item.to_string())
            .collect()
    })
}

impl PluginAdapter for AafImportAdapter {
    fn id(&self) -> &'static str {
        PLUGIN_AAF_IMPORT
    }
    fn capabilities(&self) -> &'static [&'static str] {
        &["reads_external_files", "reads_user_library"]
    }
    fn dispatch(&self, command: &'static str, params: &Value) -> PluginResult {
        match command {
            "parse" => super::sfx_aaf_import::plugin_sfx_aaf_import_parse(
                opt_str(params, "source_path"),
                opt_str_vec(params, "media_roots"),
                opt_bool(params, "read_only"),
            ),
            "candidates" => super::sfx_aaf_import::plugin_sfx_aaf_import_candidates(
                opt_str(params, "parse_id"),
                opt_bool(params, "reviewed"),
            ),
            _ => unsupported(self.id(), command, command),
        }
    }
}

impl PluginAdapter for PtOverlayAdapter {
    fn id(&self) -> &'static str {
        PLUGIN_PT_OVERLAY
    }
    fn capabilities(&self) -> &'static [&'static str] {
        &["reads_user_library", "connects_ptsl"]
    }
    fn dispatch(&self, command: &'static str, _params: &Value) -> PluginResult {
        match command {
            "capabilities" => super::sfx_pt_overlay::plugin_sfx_pt_overlay_capabilities(),
            "status" => super::sfx_pt_overlay::plugin_sfx_pt_overlay_status(),
            _ => unsupported(self.id(), command, command),
        }
    }
}

impl PluginAdapter for PtBouncerAdapter {
    fn id(&self) -> &'static str {
        PLUGIN_PT_BOUNCER
    }
    fn capabilities(&self) -> &'static [&'static str] {
        &["reads_user_library", "connects_ptsl"]
    }
    fn dispatch(&self, command: &'static str, params: &Value) -> PluginResult {
        match command {
            "preview" => super::sfx_pt_bouncer::plugin_sfx_pt_bouncer_preview(
                opt_str(params, "output_dir"),
                opt_str_vec(params, "targets"),
            ),
            "cancel" => {
                super::sfx_pt_bouncer::plugin_sfx_pt_bouncer_cancel(opt_str(params, "job_id"))
            }
            _ => unsupported(self.id(), command, command),
        }
    }
}

const AAF: AafImportAdapter = AafImportAdapter;
const OVERLAY: PtOverlayAdapter = PtOverlayAdapter;
const BOUNCER: PtBouncerAdapter = PtBouncerAdapter;

fn adapter_for(plugin_id: &str) -> Option<&'static dyn PluginAdapter> {
    match plugin_id {
        PLUGIN_AAF_IMPORT => Some(&AAF),
        PLUGIN_PT_OVERLAY => Some(&OVERLAY),
        PLUGIN_PT_BOUNCER => Some(&BOUNCER),
        _ => None,
    }
}

/// 命令表。required 是执行该命令所需的能力；写入类命令统一使用未声明的写入能力，
/// 因此恒被阻断；即使模块状态已为 read_only，仍需人类授权与真实测试工程前置。
struct CommandSpec {
    name: &'static str,
    read_only: bool,
    required: &'static str,
}

fn command_spec(plugin_id: &str, command: &str) -> Option<&'static CommandSpec> {
    const AAF_PARSE: CommandSpec = CommandSpec {
        name: "parse",
        read_only: true,
        required: "reads_external_files",
    };
    const AAF_CANDIDATES: CommandSpec = CommandSpec {
        name: "candidates",
        read_only: true,
        required: "reads_user_library",
    };
    const AAF_TO_SESSION: CommandSpec = CommandSpec {
        name: "to_session",
        read_only: false,
        required: "writes_pt_session",
    };
    const OVERLAY_CAPABILITIES: CommandSpec = CommandSpec {
        name: "capabilities",
        read_only: true,
        required: "connects_ptsl",
    };
    const OVERLAY_SPOT: CommandSpec = CommandSpec {
        name: "spot",
        read_only: false,
        required: "writes_pt_session",
    };
    const OVERLAY_STATUS: CommandSpec = CommandSpec {
        name: "status",
        read_only: true,
        required: "connects_ptsl",
    };
    const BOUNCER_PREVIEW: CommandSpec = CommandSpec {
        name: "preview",
        read_only: true,
        required: "reads_user_library",
    };
    const BOUNCER_RUN: CommandSpec = CommandSpec {
        name: "run",
        read_only: false,
        required: "writes_audio_files",
    };
    const BOUNCER_CANCEL: CommandSpec = CommandSpec {
        name: "cancel",
        read_only: true,
        required: "reads_user_library",
    };
    match (plugin_id, command) {
        (PLUGIN_AAF_IMPORT, "parse") => Some(&AAF_PARSE),
        (PLUGIN_AAF_IMPORT, "candidates") => Some(&AAF_CANDIDATES),
        (PLUGIN_AAF_IMPORT, "to_session") => Some(&AAF_TO_SESSION),
        (PLUGIN_PT_OVERLAY, "capabilities") => Some(&OVERLAY_CAPABILITIES),
        (PLUGIN_PT_OVERLAY, "spot") => Some(&OVERLAY_SPOT),
        (PLUGIN_PT_OVERLAY, "status") => Some(&OVERLAY_STATUS),
        (PLUGIN_PT_BOUNCER, "preview") => Some(&BOUNCER_PREVIEW),
        (PLUGIN_PT_BOUNCER, "run") => Some(&BOUNCER_RUN),
        (PLUGIN_PT_BOUNCER, "cancel") => Some(&BOUNCER_CANCEL),
        _ => None,
    }
}

fn descriptor(adapter: &'static dyn PluginAdapter) -> AdapterDescriptor {
    let id = adapter.id();
    let (feature, requires_pt, read_only, write) = match id {
        PLUGIN_AAF_IMPORT => (
            "plugin-aaf-import",
            false,
            &["parse", "candidates"][..],
            &["to_session"][..],
        ),
        PLUGIN_PT_OVERLAY => (
            "plugin-pt-overlay",
            true,
            &["capabilities", "status"][..],
            &["spot"][..],
        ),
        _ => (
            "plugin-pt-bouncer",
            true,
            &["preview", "cancel"][..],
            &["run"][..],
        ),
    };
    AdapterDescriptor {
        plugin_id: id,
        kind: "in_process_module",
        status: "read_only",
        feature,
        compiled: enabled(id),
        runtime_enabled: runtime_enabled(id),
        requires_pt,
        capabilities: adapter.capabilities(),
        read_only_commands: read_only,
        write_commands: write,
        adapter_epoch: ADAPTER_EPOCH,
    }
}

pub fn describe() -> Vec<AdapterDescriptor> {
    vec![descriptor(&AAF), descriptor(&OVERLAY), descriptor(&BOUNCER)]
}

fn adapter_error(
    plugin_id: &'static str,
    command: &'static str,
    code: &'static str,
    message: &str,
    data: Value,
    warnings: &[&str],
) -> PluginResult {
    PluginResult {
        plugin_id,
        command,
        ok: false,
        code,
        message: message.to_string(),
        component_epoch: COMPONENT_EPOCH,
        data,
        evidence: "none",
        warnings: warnings.iter().map(|item| item.to_string()).collect(),
    }
}

fn canonical(plugin_id: &str) -> Option<&'static str> {
    match plugin_id {
        PLUGIN_AAF_IMPORT => Some(PLUGIN_AAF_IMPORT),
        PLUGIN_PT_OVERLAY => Some(PLUGIN_PT_OVERLAY),
        PLUGIN_PT_BOUNCER => Some(PLUGIN_PT_BOUNCER),
        _ => None,
    }
}

/// 纯函数入口：偏好开关作为显式入参，便于无共享状态的确定性单测。
pub fn invoke_with(
    plugin_id: &str,
    command: &str,
    params: &Value,
    allow_write: bool,
    preference_enabled: bool,
) -> PluginResult {
    let id = match canonical(plugin_id) {
        Some(item) => item,
        None => {
            return adapter_error(
                "unknown",
                "unknown",
                "ADAPTER_NOT_FOUND",
                "适配器注册表中不存在该模块",
                json!({ "requested": plugin_id }),
                &["可在 plugin_adapter_describe 中查询已注册模块"],
            )
        }
    };
    // 闸门 1：编译层 feature。
    if !enabled(id) {
        return disabled(id, "invoke");
    }
    // 闸门 2：偏好层运行时开关。此前偏好只影响前端展示，此处补成后端真闸门。
    if !preference_enabled {
        return adapter_error(
            id,
            "invoke",
            "PLUGIN_DISABLED",
            "设置页开关未启用：适配器层拒绝执行",
            Value::Null,
            &["Cargo feature 与设置页开关需同时启用"],
        );
    }
    // 闸门 3：版本门槛。
    if !version_at_least(APP_VERSION, MIN_APP_VERSION) {
        return adapter_error(
            id,
            "invoke",
            "MIN_APP_VERSION_UNMET",
            "主程序版本低于 min_app_version",
            Value::Null,
            &["请升级主程序后重试"],
        );
    }
    let spec = match command_spec(id, command) {
        Some(item) => item,
        None => return unsupported(id, "invoke", command),
    };
    // 闸门 4：写入类命令恒阻断，allow_write 为 true 也不放行。
    if !spec.read_only {
        let _ = allow_write;
        return blocked(id, spec.name);
    }
    // 闸门 5：能力白名单。未声明的能力即使代码里存在也应被拒绝。
    let adapter = adapter_for(id).expect("canonical id always has an adapter");
    if !adapter.capabilities().contains(&spec.required) {
        return adapter_error(
            id,
            spec.name,
            "CAPABILITY_DENIED",
            "模块未声明该命令所需能力",
            json!({ "required": spec.required, "declared": adapter.capabilities() }),
            &["能力白名单默认全关，不猜测未声明能力"],
        );
    }
    adapter.dispatch(spec.name, params)
}

/// 统一入口：偏好开关从运行时状态读取，不由单次调用自报。
pub fn invoke(plugin_id: &str, command: &str, params: &Value, allow_write: bool) -> PluginResult {
    invoke_with(
        plugin_id,
        command,
        params,
        allow_write,
        runtime_enabled(plugin_id),
    )
}

#[tauri::command]
pub fn plugin_adapter_describe() -> Vec<AdapterDescriptor> {
    describe()
}

#[tauri::command]
pub fn plugin_adapter_set_flags(
    aaf: bool,
    overlay: bool,
    bouncer: bool,
) -> Vec<AdapterDescriptor> {
    set_runtime_flags(aaf, overlay, bouncer);
    describe()
}

#[tauri::command]
pub fn plugin_adapter_invoke(
    plugin_id: String,
    command: String,
    params: Option<Value>,
    allow_write: Option<bool>,
) -> PluginResult {
    invoke(
        &plugin_id,
        &command,
        &params.unwrap_or(Value::Null),
        allow_write.unwrap_or(false),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_cover_three_read_only_modules() {
        let all = describe();
        assert_eq!(all.len(), 3);
        for item in all {
            assert_eq!(item.status, "read_only");
            assert_eq!(item.kind, "in_process_module");
            assert_eq!(item.adapter_epoch, ADAPTER_EPOCH);
            assert_eq!(item.compiled, enabled(item.plugin_id));
            assert_eq!(item.read_only_commands.len(), 2);
            assert_eq!(item.write_commands.len(), 1);
        }
        assert_eq!(describe_all().len(), 3);
    }

    #[test]
    fn capability_declarations_match_plugin_toml() {
        for (adapter, meta) in [
            (
                &AAF as &'static dyn PluginAdapter,
                super::super::sfx_aaf_import::META,
            ),
            (&OVERLAY, super::super::sfx_pt_overlay::META),
            (&BOUNCER, super::super::sfx_pt_bouncer::META),
        ] {
            for capability in adapter.capabilities() {
                assert!(
                    meta.contains(capability),
                    "{} 的能力 {} 未在 plugin.toml 声明",
                    adapter.id(),
                    capability
                );
            }
        }
    }

    #[test]
    fn unknown_module_is_not_found() {
        let item = invoke_with("not_a_plugin", "parse", &Value::Null, false, true);
        assert!(!item.ok);
        assert_eq!(item.code, "ADAPTER_NOT_FOUND");
        assert_eq!(item.evidence, "none");
    }

    #[test]
    fn unknown_command_is_rejected() {
        let item = invoke_with(
            PLUGIN_AAF_IMPORT,
            "not_a_command",
            &Value::Null,
            false,
            true,
        );
        assert!(!item.ok);
        assert!(item.code == "PLUGIN_DISABLED" || item.code == "INPUT_UNSUPPORTED");
    }

    #[test]
    fn preference_flag_is_a_backend_gate() {
        // 编译层放行时（feature 开启），偏好层关闭仍必须拒绝。
        if enabled(PLUGIN_AAF_IMPORT) {
            let item = invoke_with(PLUGIN_AAF_IMPORT, "parse", &Value::Null, false, false);
            assert!(!item.ok);
            assert_eq!(item.code, "PLUGIN_DISABLED");
            assert!(item.message.contains("设置页开关"));
        }
        // 无论 feature 状态如何，偏好开启都不会绕过编译层。
        let off = invoke_with("not_a_plugin", "parse", &Value::Null, false, true);
        assert_eq!(off.code, "ADAPTER_NOT_FOUND");
    }

    #[test]
    fn write_commands_never_ok_even_with_allow_write() {
        for (id, command) in [
            (PLUGIN_AAF_IMPORT, "to_session"),
            (PLUGIN_PT_OVERLAY, "spot"),
            (PLUGIN_PT_BOUNCER, "run"),
        ] {
            let item = invoke_with(id, command, &Value::Null, true, true);
            assert!(!item.ok, "{} 不应成功", command);
            assert!(
                item.code == "PLUGIN_DISABLED" || item.code == "WRITE_BLOCKED",
                "{} 返回了 {}",
                command,
                item.code
            );
            assert_eq!(item.evidence, "none");
        }
    }

    #[test]
    fn read_only_results_never_pretend_success_without_warnings() {
        for (id, command) in [
            (PLUGIN_AAF_IMPORT, "parse"),
            (PLUGIN_AAF_IMPORT, "candidates"),
            (PLUGIN_PT_BOUNCER, "preview"),
            (PLUGIN_PT_BOUNCER, "cancel"),
        ] {
            let item = invoke_with(id, command, &Value::Null, false, true);
            if item.ok {
                assert_ne!(item.evidence, "none");
                assert!(!item.warnings.is_empty());
            } else {
                assert_eq!(item.evidence, "none");
            }
        }
    }

    #[test]
    fn runtime_flags_default_closed_and_can_be_synced() {
        // 本文件内唯一写全局状态的用例；其余用例走纯函数 invoke_with。
        set_runtime_flags(false, false, false);
        for id in [PLUGIN_AAF_IMPORT, PLUGIN_PT_OVERLAY, PLUGIN_PT_BOUNCER] {
            assert!(!runtime_enabled(id));
            let item = invoke(id, "parse", &Value::Null, false);
            assert!(!item.ok);
            assert!(
                item.code == "PLUGIN_DISABLED" || item.code == "MIN_APP_VERSION_UNMET",
                "{}",
                item.code
            );
        }
        set_runtime_flags(true, false, false);
        assert!(runtime_enabled(PLUGIN_AAF_IMPORT));
        assert!(!runtime_enabled(PLUGIN_PT_OVERLAY));
        assert!(!runtime_enabled(PLUGIN_PT_BOUNCER));
        set_runtime_flags(false, false, false);
    }
}
