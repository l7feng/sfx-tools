//! Only connection inspection and local import previews; no import execution endpoint.
use crate::catalog::CatalogState;
use serde_json::{json, Value};
use std::{collections::HashSet, io::{Read, Write}, process::{Command, Stdio}, time::{Duration, Instant}};

fn bridge(request: Value) -> Result<Value, String> {
    let interpreter = std::env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA 不可用")?;
    let python = std::path::Path::new(&interpreter).join("Programs/Python/Python312/python.exe");
    if !python.is_file() { return Err("缺少现有 Python 3.12；未自动安装或启动 Pro Tools。".into()); }
    let mut command = Command::new(python);
    command.args(["-B", "-X", "utf8", "-c", include_str!("pt_bridge.py")])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("Python 输入管道不可用")?;
    let input = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    if let Err(e) = stdin.write_all(&input) { let _=child.kill(); let _=child.wait(); return Err(e.to_string()); }
    drop(stdin);
    // Drain pipes concurrently so even a verbose failure cannot deadlock the timeout.
    let stdout = child.stdout.take().ok_or("Python 输出管道不可用")?;
    let stderr = child.stderr.take().ok_or("Python 错误管道不可用")?;
    let out_thread = std::thread::spawn(move || { let mut s=String::new(); let _=stdout.take(2_000_000).read_to_string(&mut s); s });
    let err_thread = std::thread::spawn(move || { let mut s=String::new(); let _=stderr.take(64_000).read_to_string(&mut s); s });
    let start=Instant::now();
    let success=loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if start.elapsed() < Duration::from_secs(12) => std::thread::sleep(Duration::from_millis(30)),
            _ => { let _=child.kill(); let _=child.wait(); let _=out_thread.join(); let _=err_thread.join(); return Err("Pro Tools 检测超时或进程异常；未执行导入。".into()); }
        }
    };
    let out=out_thread.join().map_err(|_| "读取检测输出失败")?;
    let err=err_thread.join().unwrap_or_default();
    let value:Value=serde_json::from_str(&out).map_err(|_| format!("PTSL Python 环境不可用：{err}"))?;
    if !success { return Err(value["error"].as_str().unwrap_or("PTSL 检测失败").to_string()); }
    Ok(value)
}

#[tauri::command]
pub async fn protools_status() -> Result<Value,String> {
    tauri::async_runtime::spawn_blocking(||bridge(json!({"action":"status"}))).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn protools_import_preview(ids:Vec<String>,destination:String,location:String,state:tauri::State<'_,CatalogState>) -> Result<Value,String> {
    if ids.is_empty() || ids.len()>100 {return Err("请选择 1–100 条已登记素材。".into());}
    if !["new_track","clip_list"].contains(&destination.as_str()) || !["session_start","selection"].contains(&location.as_str()) {return Err("不支持的导入预览选项。".into());}
    let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut seen=HashSet::new(); let mut assets=Vec::new();
        for id in ids {
            if !seen.insert(id.clone()) {continue;}
            let path=state.resolve_asset(&id)?;
            let size=std::fs::metadata(&path).map_err(|e|e.to_string())?.len();
            assets.push(json!({"id":id,"path":path.to_str().ok_or("素材路径无法表示")?,"size":size}));
        }
        bridge(json!({"action":"preview","assets":assets,"destination":destination,"location":location}))
    }).await.map_err(|e|e.to_string())?
}
