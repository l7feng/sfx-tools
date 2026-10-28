//! 显式源文件操作。先列明影响并签发短期令牌，再在导入互斥锁下重新核验。
//! 不覆盖目标，不跟随重解析点，删除只交给 Windows 回收站。
use super::*;
use std::{collections::HashMap, sync::OnceLock, time::Instant};

#[derive(Debug,Serialize,Clone)]
pub struct SourcePreview {
    pub token:String,pub action:String,pub path:String,pub destination:Option<String>,
    pub files:u64,pub bytes:u64,pub catalog_items:u64,pub unindexed_files:u64,pub warning:String,
}
#[derive(Debug,Serialize)]
pub struct SourceResult {pub message:String,pub path:Option<String>}
#[derive(Clone)]
struct Plan {db:PathBuf,action:String,id:Option<String>,folder:Option<String>,value:Option<String>,library:String,fingerprint:String,preview:SourcePreview,created:Instant}
static PLANS:OnceLock<Mutex<HashMap<String,Plan>>>=OnceLock::new();
fn plans()->&'static Mutex<HashMap<String,Plan>>{PLANS.get_or_init(||Mutex::new(HashMap::new()))}
fn scoped(path:&Path,root:&Path)->bool{path==root||path.starts_with(root)}
fn remap(path:&Path,old:&Path,new:&Path)->Result<PathBuf,String>{if path==old{Ok(new.to_path_buf())}else{Ok(new.join(path.strip_prefix(old).map_err(err)?))}}
fn existing_path(raw:&str)->Result<PathBuf,String>{
    let input=Path::new(raw);
    if !input.is_absolute(){return Err("必须提供绝对路径。".into());}
    // 逐层拒绝联接：只检查末端会漏掉父目录的重定向。
    for ancestor in input.ancestors(){
        let metadata=fs::symlink_metadata(ancestor).map_err(err)?;
        if is_reparse(&metadata){return Err("不操作符号链接、目录联接或其他重解析点。".into());}
    }
    fs::canonicalize(input).map_err(err)
}
fn target_path(conn:&Connection,id:Option<&str>,folder:Option<&str>)->Result<PathBuf,String>{
    match (id,folder){
        (Some(id),None)=>{
            let stored:Option<String>=conn.query_row("SELECT path FROM assets WHERE id=?1",[id],|r|r.get(0)).optional().map_err(err)?;
            let path=existing_path(&stored.ok_or("素材不存在，请刷新。")?)?;
            if !path.is_file(){return Err("所选素材不是普通文件。".into());}Ok(path)
        },
        (None,Some(folder))=>{
            let path=existing_path(folder)?;
            if !path.is_dir(){return Err("所选对象不是目录。".into());}
            // 目录树中的盘符/祖先是虚拟导航，不能视作磁盘操作授权。
            let roots=conn.prepare("SELECT root FROM jobs UNION SELECT path FROM import_history").map_err(err)?.query_map([],|r|r.get::<_,String>(0)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
            if !roots.iter().filter_map(|p|fs::canonicalize(p).ok()).any(|root|scoped(&path,&root)){
                return Err("该目录只是导航祖先或未登记来源；请先添加该目录后再操作。".into());
            }Ok(path)
        },_=>Err("请选择一个素材或一个目录。".into())
    }
}
fn protect(path:&Path,db:&Path)->Result<(),String>{
    if path.parent().is_none()||path.file_name().is_none(){return Err("不能操作磁盘根或共享根。".into());}
    let mut protected=vec![db.to_path_buf()];
    if let Ok(path)=std::env::current_dir(){protected.push(path);}
    if let Ok(path)=std::env::current_exe(){protected.push(path);}
    for name in ["SystemRoot","WINDIR","ProgramFiles","ProgramFiles(x86)","USERPROFILE"]{
        if let Some(path)=std::env::var_os(name){protected.push(PathBuf::from(path));}
    }
    if protected.iter().filter_map(|p|fs::canonicalize(p).ok()).any(|p|scoped(&p,path)){
        return Err("该目录包含程序、数据库、系统或用户根目录，不能由素材菜单操作。".into());
    }Ok(())
}
fn leaf_name(value:&str)->Result<&str,String>{
    let name=value.trim();
    if name.is_empty()||name=="."||name==".."||name.len()>240||name.ends_with('.')||name.ends_with(' ')||name.chars().any(|c|c<' '||"<>:\"/\\|?*".contains(c)){
        return Err("请输入有效的单个文件或文件夹名称，不能包含路径分隔符或保留字符。".into());
    }
    let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if ["CON","PRN","AUX","NUL"].contains(&stem.as_str())||(stem.len()==4&&(stem.starts_with("COM")||stem.starts_with("LPT"))&&matches!(stem.as_bytes()[3],b'1'..=b'9')){return Err("名称是Windows保留设备名。".into());}
    Ok(name)
}
fn catalog_rows(conn:&Connection,path:&Path)->Result<Vec<(String,String)>,String>{
    let rows=conn.prepare("SELECT id,path FROM assets ORDER BY id").map_err(err)?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
    Ok(rows.into_iter().filter(|(_,p)|scoped(Path::new(p),path)).collect())
}
fn tree_snapshot(path:&Path,hash:&mut Sha256,files:&mut u64,bytes:&mut u64,entries:&mut usize)->Result<(),String>{
    *entries+=1;if *entries>500_000{return Err("目录超过50万项，当前安全预检不支持；未执行操作。".into());}
    if path.components().count()>512{return Err("目录层级过深，当前安全预检不支持。".into());}
    let metadata=fs::symlink_metadata(path).map_err(err)?;
    if is_reparse(&metadata){return Err("目录内包含链接或联接，未执行整目录操作。".into());}
    hash.update(path_string(path)?.as_bytes());hash.update(metadata_stamp(&metadata)?.as_bytes());hash.update(metadata.len().to_le_bytes());
    if metadata.is_file(){*files+=1;*bytes=bytes.saturating_add(metadata.len());}
    else if metadata.is_dir(){
        let mut children=fs::read_dir(path).map_err(err)?.map(|v|v.map(|v|v.path())).collect::<Result<Vec<_>,_>>().map_err(err)?;children.sort();
        for child in children{tree_snapshot(&child,hash,files,bytes,entries)?;}
    }else{return Err("目录内存在非普通文件，未执行操作。".into());}Ok(())
}
fn build_plan(db:&Path,action:&str,id:Option<String>,folder:Option<String>,value:Option<String>,library:&str)->Result<Plan,String>{
    let conn=connection(db)?;build_plan_on_conn(&conn,db,action,id,folder,value,library)
}
fn build_plan_on_conn(conn:&Connection,db:&Path,action:&str,id:Option<String>,folder:Option<String>,value:Option<String>,library:&str)->Result<Plan,String>{
    validate_library(library)?;
    if !["rename","move","mkdir","trash"].contains(&action){return Err("不支持该源文件操作。".into());}
    let path=target_path(conn,id.as_deref(),folder.as_deref())?;protect(&path,db)?;
    if action=="mkdir"&&!path.is_dir(){return Err("只能在文件夹中新建子文件夹。".into());}
    if action=="trash"&&!path.is_dir(){return Err("磁盘删除仅限明确选择的整个目录；文件菜单只提供出库。".into());}
    let destination=match action{
        "rename"|"mkdir"=>{
            let name=leaf_name(value.as_deref().ok_or("请输入名称。")?)?;
            if action=="rename"&&path.is_file(){
                let new=Path::new(name);if new.extension().map(|v|v.to_ascii_lowercase())!=path.extension().map(|v|v.to_ascii_lowercase()){return Err("重命名必须保留原音频扩展名。".into());}
            }
            Some(if action=="mkdir"{path.join(name)}else{path.parent().ok_or("没有父目录")?.join(name)})
        },
        "move"=>{
            let parent=existing_path(value.as_deref().ok_or("请选择目标文件夹。")?)?;
            if !parent.is_dir(){return Err("移动目标必须是文件夹。".into());}
            let destination=parent.join(path.file_name().ok_or("没有对象名称")?);
            #[cfg(windows)] if path.components().next()!=destination.components().next(){return Err("当前仅支持同一磁盘卷内移动，跨盘请先在资源管理器移动再重新定位。".into());}
            Some(destination)
        },_=>None
    };
    if let Some(target)=&destination{
        if target.try_exists().map_err(err)?{return Err("目标已存在，未覆盖任何文件。".into());}
        if action!="mkdir"&&scoped(target,&path){return Err("不能将目录移入自身或其子目录。".into());}
        protect(target,db)?;
    }
    let rows=if action=="mkdir"{vec![]}else{catalog_rows(conn,&path)?};
    if action!="mkdir"{if let Some(target)=&destination{
        for (id,old) in &rows{
            let next=remap(Path::new(old),&path,target)?;let key=path_string(&next)?;
            if conn.query_row("SELECT EXISTS(SELECT 1 FROM assets WHERE path_key=?1 AND id<>?2)",params![key,id],|r|r.get::<_,bool>(0)).map_err(err)?{return Err("目标位置已有其他素材索引，未合并或覆盖记录。".into());}
        }
    }}
    let mut hash=Sha256::new();let (mut files,mut bytes,mut entries)=(0,0,0);
    if action=="mkdir"{hash.update(path_string(&path)?.as_bytes());hash.update(metadata_stamp(&fs::metadata(&path).map_err(err)?)?.as_bytes());}
    else{tree_snapshot(&path,&mut hash,&mut files,&mut bytes,&mut entries)?;}
    for (id,p) in &rows{
        hash.update(id.as_bytes());hash.update(p.as_bytes());
        let metadata:Option<(i64,String,String)>=conn.query_row("SELECT favorite,tags,tags_search FROM user_metadata WHERE asset_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(err)?;
        hash.update(format!("{metadata:?}").as_bytes());
        // 后增的关联也参与确认快照，避免确认后新增播放列表关联被静默清除。
        let memberships=conn.prepare("SELECT playlist_id,added FROM playlist_items WHERE asset_id=?1 ORDER BY playlist_id").map_err(err)?.query_map([id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
        hash.update(serde_json::to_vec(&memberships).map_err(err)?);
        let played:Option<String>=conn.query_row("SELECT played_at FROM play_history WHERE asset_id=?1",[id],|r|r.get(0)).optional().map_err(err)?;
        hash.update(serde_json::to_vec(&played).map_err(err)?);
    }
    if let Some(target)=&destination{hash.update(path_string(target)?.as_bytes());}
    let fingerprint=format!("{:x}",hash.finalize());
    let warning=if action=="trash"{"整个磁盘文件夹及未入库文件将移入回收站，同时清除两库索引、收藏、标签、备注、历史及播放列表关联。不能使用系统回收站时停止，不永久删除。"}else if action=="mkdir"{"将在原磁盘位置创建空子文件夹。"}else{"会修改真实磁盘位置并更新两库全部引用，保留素材ID、收藏、标签与播放列表；不覆盖目标。"}.into();
    let preview=SourcePreview{token:Uuid::new_v4().to_string(),action:action.into(),path:path_string(&path)?,destination:destination.as_deref().map(path_string).transpose()?,files,bytes,catalog_items:rows.len() as u64,unindexed_files:files.saturating_sub(rows.len() as u64),warning};
    Ok(Plan{db:db.into(),action:action.into(),id,folder,value,library:library.into(),fingerprint,preview,created:Instant::now()})
}
fn preview(db:&Path,action:&str,id:Option<String>,folder:Option<String>,value:Option<String>,library:&str)->Result<SourcePreview,String>{
    let plan=build_plan(db,action,id,folder,value,library)?;let result=plan.preview.clone();let mut pending=plans().lock().map_err(|_|"操作预览锁不可用")?;
    pending.retain(|_,p|p.created.elapsed()<Duration::from_secs(300));if pending.len()>=16{pending.clear();}pending.insert(result.token.clone(),plan);Ok(result)
}
fn update_paths(conn:&Connection,old:&Path,new:&Path,rows:&[(String,String)])->Result<(),String>{
    for (id,path) in rows{
        let next=remap(Path::new(path),old,new)?;let next=path_string(&next)?;let name=Path::new(&next).file_name().and_then(|s|s.to_str()).ok_or("名称无效")?;
        conn.execute("UPDATE assets SET path=?1,path_key=?1,name=?2 WHERE id=?3",params![next,name,id]).map_err(err)?;
    }
    for (table,column) in [("import_history","path"),("pinned_paths","path"),("catalog_directories","path"),("jobs","root")]{
        let present:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",[table],|r|r.get(0)).map_err(err)?;
        if !present{continue;}
        let values=conn.prepare(&format!("SELECT DISTINCT {column} FROM {table}")).map_err(err)?.query_map([],|r|r.get::<_,String>(0)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
        for value in values{if scoped(Path::new(&value),old){let target=path_string(&remap(Path::new(&value),old,new)?)?;conn.execute(&format!("UPDATE OR REPLACE {table} SET {column}=?1 WHERE {column}=?2"),params![target,value]).map_err(err)?;}}
    }Ok(())
}
fn commit(db:&Path,token:&str)->Result<SourceResult,String>{
    let plan=plans().lock().map_err(|_|"操作预览锁不可用")?.remove(token).ok_or("预览已失效，请重新检查后确认。")?;
    if plan.db!=db||plan.created.elapsed()>Duration::from_secs(300){return Err("操作预览已过期，请重新检查。".into());}
    let mut conn=connection(db)?;let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    // 先锁定SQLite写事务再复核，阻止标签、历史、播放列表在确认核对与提交间变化。
    let current=build_plan_on_conn(&tx,db,&plan.action,plan.id.clone(),plan.folder.clone(),plan.value.clone(),&plan.library)?;
    if current.fingerprint!=plan.fingerprint{return Err("文件或关联在确认后发生变化，未执行，请重新检查。".into());}
    let path=Path::new(&plan.preview.path);let destination=plan.preview.destination.as_deref().map(Path::new);
    let rows=catalog_rows(&tx,path)?;
    if plan.action=="mkdir"{
        let target=destination.ok_or("没有目标路径")?;
        tx.execute("INSERT OR IGNORE INTO catalog_directories(path,library) VALUES(?1,?2)",params![path_string(target)?,plan.library]).map_err(err)?;
        fs::create_dir(target).map_err(err)?;
        if let Err(e)=tx.commit(){let rollback=fs::remove_dir(target);return Err(format!("数据库提交失败：{e}；空目录撤销：{rollback:?}"));}
        return Ok(SourceResult{message:"已创建子文件夹并登记到当前素材库目录树。".into(),path:Some(path_string(target)?)});
    }
    if plan.action=="trash"{
        for (id,_) in &rows{tx.execute("DELETE FROM assets WHERE id=?1",[id]).map_err(err)?;}
        // 删除常用/最近路径引用；历史任务保留便于审计，不把任务当文件恢复副本。
        for table in ["pinned_paths","import_history","catalog_directories"]{let sql=format!("DELETE FROM {table} WHERE path=?1 OR substr(path,1,?2)=?3");let prefix=format!("{}{}",path_string(path)?,std::path::MAIN_SEPARATOR);tx.execute(&sql,params![path_string(path)?,prefix.chars().count(),prefix]).map_err(err)?;}
        recycle(path)?;
        tx.commit().map_err(|e|format!("目录已交系统回收站，但索引提交失败：{e}。请刷新并检查失联记录。"))?;
        return Ok(SourceResult{message:"整个目录已移入系统回收站，已清除相应素材关联。".into(),path:None});
    }
    let target=destination.ok_or("没有目标路径")?;
    update_paths(&tx,path,target,&rows)?;
    rename_without_replace(path,target).map_err(|e|format!("磁盘操作失败，索引未提交：{e}"))?;
    if let Err(e)=tx.commit(){let rollback=rename_without_replace(target,path);return Err(format!("索引提交失败：{e}；磁盘回退结果：{rollback:?}。请核对实际位置。"));}
    Ok(SourceResult{message:"已更新磁盘位置及所有素材引用，标注保持。".into(),path:Some(path_string(target)?)})
}

#[tauri::command]
pub async fn source_preview(action:String,id:Option<String>,folder:Option<String>,value:Option<String>,library:Option<String>,state:tauri::State<'_,CatalogState>)->Result<SourcePreview,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|preview(db,&action,id,folder,value,library.as_deref().unwrap_or("sound")))).await.map_err(err)?}
#[tauri::command]
pub async fn source_commit(token:String,state:tauri::State<'_,CatalogState>)->Result<SourceResult,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|commit(db,&token))).await.map_err(err)?}
#[tauri::command]
pub async fn source_reveal(id:Option<String>,folder:Option<String>,state:tauri::State<'_,CatalogState>)->Result<(),String>{
    let db=state.database_path();let path=tauri::async_runtime::spawn_blocking(move||target_path(&connection(&db)?,id.as_deref(),folder.as_deref())).await.map_err(err)??;
    std::process::Command::new("explorer.exe").arg(format!("/select,{}",visible(&path))).spawn().map_err(err)?;Ok(())
}
fn visible(path:&Path)->String{let value=path.display().to_string();if let Some(rest)=value.strip_prefix(r"\\?\UNC\"){format!(r"\\{rest}")}else{value.trim_start_matches(r"\\?\").into()}}
fn rename_without_replace(from:&Path,to:&Path)->Result<(),String>{
    #[cfg(windows)] unsafe{
        use windows::{core::HSTRING,Win32::Storage::FileSystem::{MoveFileExW,MOVE_FILE_FLAGS}};
        MoveFileExW(&HSTRING::from(path_string(from)?),&HSTRING::from(path_string(to)?),MOVE_FILE_FLAGS(0)).map_err(err)
    }
    #[cfg(not(windows))]{if to.try_exists().map_err(err)?{return Err("目标已存在，未覆盖。".into());}fs::rename(from,to).map_err(err)}
}
#[tauri::command]
pub async fn source_copy(window:tauri::Window,ids:Option<Vec<String>>,folder:Option<String>,names_only:Option<bool>,state:tauri::State<'_,CatalogState>)->Result<usize,String>{
    let db=state.database_path();let names_only=names_only.unwrap_or(false);
    let paths=tauri::async_runtime::spawn_blocking(move||{
        let conn=connection(&db)?;
        if let Some(folder)=folder{
            if ids.as_ref().is_some_and(|v|!v.is_empty()){return Err("请选择文件夹或文件其中一种。".into());}
            let registered=folder_nodes_in_library(&db,None)?.iter().any(|node|node.path==folder);
            if !registered{return Err("目录不在已登记目录树中，请刷新。".into());}
            return Ok(vec![PathBuf::from(folder)]);
        }
        let ids=ids.unwrap_or_default();if ids.is_empty()||ids.len()>100{return Err("请选择1至100条素材。".into());}
        ids.iter().map(|id|{
            // 复制路径也适用于离线记录，不用resolve_asset的可播放限制。
            let stored:String=conn.query_row("SELECT path FROM assets WHERE id=?1",[id],|r|r.get(0)).map_err(err)?;Ok(PathBuf::from(stored))
        }).collect::<Result<Vec<_>,String>>()
    }).await.map_err(err)??;
    let text=paths.iter().map(|p|if names_only{p.file_name().unwrap_or_default().to_string_lossy().into_owned()}else{visible(p)}).collect::<Vec<_>>().join("\r\n");
    #[cfg(windows)] unsafe{
        use windows::Win32::{Foundation::HANDLE,System::{DataExchange::*,Memory::*}};
        let data:Vec<u16>=text.encode_utf16().chain(Some(0)).collect();
        OpenClipboard(Some(window.hwnd().map_err(err)?)).map_err(|e|format!("剪贴板被占用：{e}"))?;
        let result=(||{
            let memory=GlobalAlloc(GMEM_MOVEABLE,data.len()*2).map_err(err)?;let dest=GlobalLock(memory);
            if dest.is_null(){let _=windows::Win32::Foundation::GlobalFree(Some(memory));return Err("无法分配剪贴板内存".into());}
            std::ptr::copy_nonoverlapping(data.as_ptr(),dest.cast::<u16>(),data.len());let _=GlobalUnlock(memory);
            if let Err(e)=EmptyClipboard().and_then(|_|SetClipboardData(13,Some(HANDLE(memory.0))).map(|_|())){let _=windows::Win32::Foundation::GlobalFree(Some(memory));return Err(e.to_string());}Ok::<(),String>(())
        })();let _=CloseClipboard();result?;
    }
    #[cfg(not(windows))]{let _=(window,text);return Err("目前仅支持Windows剪贴板。".into());}
    Ok(paths.len())
}

#[cfg(windows)]
fn recycle(path:&Path)->Result<(),String>{
    use windows::{core::HSTRING,Win32::{System::Com::*,UI::Shell::*,Storage::FileSystem::{GetDriveTypeW,GetVolumeInformationW}}};
    // 网络共享和非盘符目标无法预先确认系统回收站，宁可拒绝，不降级永久删除。
    let display=visible(path);let bytes=display.as_bytes();
    if bytes.len()<3||bytes[1]!=b':'||bytes[2]!=b'\\'{return Err("此位置无法保证使用Windows回收站，未删除。".into());}
    unsafe{
        CoInitializeEx(None,COINIT_APARTMENTTHREADED).ok().map_err(err)?;
        struct ComGuard;impl Drop for ComGuard{fn drop(&mut self){unsafe{CoUninitialize();}}}let _guard=ComGuard;
        let root=HSTRING::from(&display[..3]);
        if GetDriveTypeW(&root)!=3{return Err("当前仅支持本地固定磁盘的回收站；该位置未删除。".into());}
        let mut filesystem=[0u16;32];GetVolumeInformationW(&root,None,None,None,None,Some(&mut filesystem)).map_err(err)?;
        if String::from_utf16_lossy(&filesystem[..filesystem.iter().position(|v|*v==0).unwrap_or(filesystem.len())])!="NTFS"{return Err("当前仅支持本地NTFS回收站；该位置未删除。".into());}
        let mut info=SHQUERYRBINFO{cbSize:std::mem::size_of::<SHQUERYRBINFO>() as u32,..Default::default()};
        SHQueryRecycleBinW(&root,&mut info).map_err(|e|format!("该磁盘回收站不可用，未删除：{e}"))?;
        // 用“回收”专用接口而不是DeleteItem：不能回收时没有永久删除后备分支。
        // https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-itransfersource-recycleitem
        let item:IShellItem=SHCreateItemFromParsingName(&HSTRING::from(display),None).map_err(err)?;
        let parent=item.GetParent().map_err(err)?;
        let transfer:ITransferSource=parent.BindToHandler(None,&BHID_Transfer).map_err(|e|format!("此目录不支持系统回收接口，未删除：{e}"))?;
        let bin:IShellItem=SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder,KF_FLAG_DEFAULT,None).map_err(err)?;
        let _recycled=transfer.RecycleItem(&item,&bin,0).map_err(|e|format!("系统未完成回收，索引未提交。系统可能已回收部分内容，请核对回收站与原目录；未改用永久删除：{e}"))?;
        if path.exists(){return Err("系统回收未完成，索引未提交。系统可能已回收部分内容，请核对回收站与原目录。".into());}
    }Ok(())
}
#[cfg(not(windows))]fn recycle(_: &Path)->Result<(),String>{Err("当前平台不支持安全回收站操作，未删除。".into())}

#[cfg(test)]
mod tests{
    use super::*;
    struct Fixture{root:PathBuf,db:PathBuf,audio:PathBuf,id:String,state:Option<CatalogState>}
    impl Fixture{
        fn new()->Self{
            let root=std::env::temp_dir().join(format!("sound-workbench-source-{}",Uuid::new_v4()));fs::create_dir_all(root.join("audio")).unwrap();let root=fs::canonicalize(root).unwrap();let db=root.join("catalog.sqlite");let audio=root.join("audio");
            let state=CatalogState::new(db.clone()).unwrap();let source=audio.join("音效.wav");fs::write(&source,b"test audio bytes, not actually played").unwrap();let conn=connection(&db).unwrap();let mut job=ImportJob::default();record_file(&conn,&source,"WAV",&mut job).unwrap();remember_path(&db,&path_string(&audio).unwrap()).unwrap();
            let id=conn.query_row("SELECT id FROM assets",[],|r|r.get(0)).unwrap();Self{root,db,audio,id,state:Some(state)}
        }
        fn folder(&self)->String{path_string(&self.audio).unwrap()}
    }
    impl Drop for Fixture{fn drop(&mut self){self.state.take();let _=fs::remove_dir_all(&self.root);}}
    #[test]
    fn source_file_rename_preserves_id_annotations_and_bytes(){
        let f=Fixture::new();let conn=connection(&f.db).unwrap();set_favorite(&f.db,&f.id,true).unwrap();set_tags(&f.db,&f.id,vec!["保留".into()]).unwrap();let before=fs::read(f.audio.join("音效.wav")).unwrap();
        let p=preview(&f.db,"rename",Some(f.id.clone()),None,Some("新名.wav".into()),"sound").unwrap();assert_eq!(p.files,1);assert_eq!(p.catalog_items,1);commit(&f.db,&p.token).unwrap();
        assert!(!f.audio.join("音效.wav").exists());assert_eq!(fs::read(f.audio.join("新名.wav")).unwrap(),before);
        let (id,name,favorite,tags):(String,String,bool,String)=conn.query_row("SELECT a.id,a.name,m.favorite,m.tags FROM assets a JOIN user_metadata m ON m.asset_id=a.id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(id,f.id);assert_eq!(name,"新名.wav");assert!(favorite);assert!(tags.contains("保留"));assert!(commit(&f.db,&p.token).is_err());
    }
    #[test]
    fn source_folder_move_updates_descendants_pins_history_and_empty_directories(){
        let f=Fixture::new();let conn=connection(&f.db).unwrap();let nested=f.audio.join("子目录");fs::create_dir(&nested).unwrap();let source=nested.join("二.wav");fs::write(&source,b"second sound").unwrap();record_file(&conn,&source,"WAV",&mut ImportJob::default()).unwrap();
        let empty=f.audio.join("空目录");fs::create_dir(&empty).unwrap();conn.execute("INSERT INTO catalog_directories(path,library) VALUES(?1,'music')",[path_string(&empty).unwrap()]).unwrap();conn.execute("INSERT INTO pinned_paths(library,path,created) VALUES('sound',?1,'0')",[f.folder()]).unwrap();let parent=f.root.join("目标");fs::create_dir(&parent).unwrap();
        let p=preview(&f.db,"move",None,Some(f.folder()),Some(path_string(&parent).unwrap()),"sound").unwrap();assert_eq!(p.files,2);commit(&f.db,&p.token).unwrap();let moved=parent.join("audio");assert!(!f.audio.exists());assert!(moved.join("子目录/二.wav").exists());assert_eq!(fs::read(moved.join("音效.wav")).unwrap(),b"test audio bytes, not actually played");
        assert_eq!(conn.query_row("SELECT path FROM assets WHERE id=?1",[&f.id],|r|r.get::<_,String>(0)).unwrap(),path_string(&moved.join("音效.wav")).unwrap());
        for table in ["pinned_paths","import_history","catalog_directories"]{let path=conn.query_row(&format!("SELECT path FROM {table} LIMIT 1"),[],|r|r.get::<_,String>(0)).unwrap();assert!(Path::new(&path).starts_with(&moved),"{table}");}
    }
    #[test]
    fn source_preview_rejects_changed_source_and_conflicting_destination_without_overwrite(){
        let f=Fixture::new();let target=f.audio.join("占用.wav");fs::write(&target,b"keep target").unwrap();assert!(preview(&f.db,"rename",Some(f.id.clone()),None,Some("占用.wav".into()),"sound").is_err());
        assert!(rename_without_replace(&f.audio.join("音效.wav"),&target).is_err());assert_eq!(fs::read(&target).unwrap(),b"keep target");
        let p=preview(&f.db,"rename",Some(f.id.clone()),None,Some("未执行.wav".into()),"sound").unwrap();fs::write(f.audio.join("音效.wav"),b"source changed").unwrap();assert!(commit(&f.db,&p.token).unwrap_err().contains("变化"));assert!(!f.audio.join("未执行.wav").exists());assert_eq!(fs::read(f.audio.join("音效.wav")).unwrap(),b"source changed");
    }
    #[test]
    fn source_mkdir_is_visible_and_name_boundary_rules_reject_escape(){
        let f=Fixture::new();let p=preview(&f.db,"mkdir",None,Some(f.folder()),Some("新子目录".into()),"music").unwrap();assert_eq!(p.files,0);assert_eq!(p.catalog_items,0);commit(&f.db,&p.token).unwrap();let child=f.audio.join("新子目录");assert!(child.is_dir());
        let nodes=folder_nodes_in_library(&f.db,Some("music")).unwrap();assert!(nodes.iter().any(|n|n.path==path_string(&child).unwrap()&&n.count==0));
        for name in ["../逃逸","..","CON","AUX.wav","COM1.txt","bad:name","bad."]{assert!(leaf_name(name).is_err(),"{name}");}
        assert!(preview(&f.db,"rename",Some(f.id.clone()),None,Some("改格式.mp3".into()),"sound").is_err());
        assert!(preview(&f.db,"move",None,Some(f.folder()),Some(f.folder()),"sound").is_err());
        assert!(preview(&f.db,"rename",None,Some(path_string(&f.root).unwrap()),Some("越界".into()),"sound").is_err());
    }
    #[test]
    fn source_confirmation_tracks_actual_playlist_and_history_rows_not_just_counts(){
        let f=Fixture::new();let conn=connection(&f.db).unwrap();
        for id in ["p1","p2"]{conn.execute("INSERT INTO playlists(id,library,name,created) VALUES(?1,'sound',?1,'0')",[id]).unwrap();}
        conn.execute("INSERT INTO playlist_items(playlist_id,asset_id,added) VALUES('p1',?1,'1')",[&f.id]).unwrap();
        let p=preview(&f.db,"rename",Some(f.id.clone()),None,Some("new.wav".into()),"sound").unwrap();
        conn.execute("UPDATE playlist_items SET playlist_id='p2' WHERE asset_id=?1",[&f.id]).unwrap();
        assert!(commit(&f.db,&p.token).unwrap_err().contains("变化"));assert!(f.audio.join("音效.wav").exists());
        conn.execute("INSERT INTO play_history(asset_id,played_at) VALUES(?1,'1')",[&f.id]).unwrap();
        let p=preview(&f.db,"rename",Some(f.id.clone()),None,Some("new.wav".into()),"sound").unwrap();
        conn.execute("UPDATE play_history SET played_at='2' WHERE asset_id=?1",[&f.id]).unwrap();
        assert!(commit(&f.db,&p.token).unwrap_err().contains("变化"));assert!(!f.audio.join("new.wav").exists());
    }
    #[test]
    fn source_database_commit_failure_rolls_disk_rename_back(){
        let f=Fixture::new();let conn=connection(&f.db).unwrap();
        conn.execute_batch("CREATE TABLE injected_deferred(pid TEXT REFERENCES assets(id) DEFERRABLE INITIALLY DEFERRED);
            CREATE TRIGGER injected_failure AFTER UPDATE OF path ON assets BEGIN INSERT INTO injected_deferred(pid) VALUES('does-not-exist'); END;").unwrap();
        let p=preview(&f.db,"rename",Some(f.id.clone()),None,Some("回退.wav".into()),"sound").unwrap();let error=commit(&f.db,&p.token).unwrap_err();assert!(error.contains("回退"));
        assert!(f.audio.join("音效.wav").exists());assert!(!f.audio.join("回退.wav").exists());
        assert_eq!(conn.query_row("SELECT path FROM assets WHERE id=?1",[&f.id],|r|r.get::<_,String>(0)).unwrap(),path_string(&f.audio.join("音效.wav")).unwrap());
    }
    #[cfg(windows)]
    #[test]
    fn source_parent_junction_and_descendant_junction_are_refused(){
        use std::os::windows::process::CommandExt;
        let f=Fixture::new();let link=f.audio.join("链接");let outside=f.root.join("outside");fs::create_dir(&outside).unwrap();
        let output=std::process::Command::new("cmd.exe").args(["/C","mklink","/J"]).arg(&link).arg(&outside).creation_flags(0x08000000).output().unwrap();assert!(output.status.success(),"junction setup failed: {}",String::from_utf8_lossy(&output.stderr));
        assert!(existing_path(&path_string(&link).unwrap()).is_err());let p=preview(&f.db,"rename",None,Some(f.folder()),Some("不执行".into()),"sound");assert!(p.unwrap_err().contains("链接"));
        fs::remove_dir(&link).unwrap();assert!(outside.exists());
    }
}
