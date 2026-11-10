//! 应用侧常用路径、播放列表、试听历史和出库；不改写源音频。
use super::*;
use std::{collections::{BTreeSet,HashMap},sync::OnceLock,time::Instant};

pub(super) fn initialize(conn:&Connection)->Result<(),String>{
    conn.execute_batch("CREATE TABLE IF NOT EXISTS playlists (
        id TEXT PRIMARY KEY,library TEXT NOT NULL CHECK(library IN ('sound','music')),
        name TEXT NOT NULL,created TEXT NOT NULL,UNIQUE(library,name));
        CREATE TABLE IF NOT EXISTS playlist_items (
        playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
        asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,added TEXT NOT NULL,
        PRIMARY KEY(playlist_id,asset_id));
        CREATE TABLE IF NOT EXISTS play_history (
        asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,played_at TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS pinned_paths (
        library TEXT NOT NULL CHECK(library IN ('sound','music')),path TEXT NOT NULL,created TEXT NOT NULL,
        PRIMARY KEY(library,path));
        CREATE TABLE IF NOT EXISTS catalog_directories (
        path TEXT NOT NULL,library TEXT NOT NULL CHECK(library IN ('sound','music')),PRIMARY KEY(path,library));").map_err(err)?;
    ensure_pin_color(conn)
}
/// 常用路径的颜色标记是后加字段：旧库用 ALTER 补齐，不重建表、不动既有置顶。
fn ensure_pin_color(conn:&Connection)->Result<(),String>{
    let mut stmt=conn.prepare("PRAGMA table_info(pinned_paths)").map_err(err)?;
    let names=stmt.query_map([],|r|r.get::<_,String>(1)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
    drop(stmt);
    if !names.iter().any(|name|name=="color"){conn.execute("ALTER TABLE pinned_paths ADD COLUMN color TEXT",[]).map_err(err)?;}
    Ok(())
}
const PIN_COLORS:[&str;6]=["red","orange","yellow","green","blue","purple"];
#[derive(Serialize)]
pub struct TagCount{pub name:String,pub count:u64}
#[derive(Serialize)]
pub struct PlaylistSummary{pub id:String,pub name:String,pub count:u64}
#[derive(Serialize)]
pub struct PinEntry{pub path:String,pub color:Option<String>}
#[derive(Serialize)]
pub struct FavoriteGroup{pub name:String,pub count:u64}
#[derive(Serialize)]
pub struct LibraryOverview{pub tags:Vec<TagCount>,pub playlists:Vec<PlaylistSummary>,pub pins:Vec<PinEntry>,pub history_count:u64,pub favorite_count:u64,pub favorite_ungrouped:u64,pub favorite_groups:Vec<FavoriteGroup>}
fn overview(db:&Path,library:&str)->Result<LibraryOverview,String>{
    let library=validate_library(library)?;let mut conn=connection(db)?;let tx=conn.transaction().map_err(err)?;
    let mut counts:BTreeMap<String,u64>=BTreeMap::new();
    {
        let mut stmt=tx.prepare("SELECT m.tags FROM user_metadata m JOIN asset_libraries l ON l.asset_id=m.asset_id WHERE l.library=?1").map_err(err)?;
        for raw in stmt.query_map([library],|r|r.get::<_,String>(0)).map_err(err)?{
            let tags:Vec<String>=serde_json::from_str(&raw.map_err(err)?).map_err(err)?;
            for tag in tags.into_iter().collect::<BTreeSet<_>>(){*counts.entry(tag).or_default()+=1;}
        }
    }
    let tags=counts.into_iter().map(|(name,count)|TagCount{name,count}).collect();
    let playlists={let mut stmt=tx.prepare("SELECT p.id,p.name,COUNT(l.asset_id) FROM playlists p LEFT JOIN playlist_items i ON i.playlist_id=p.id LEFT JOIN asset_libraries l ON l.asset_id=i.asset_id AND l.library=p.library WHERE p.library=?1 GROUP BY p.id,p.name ORDER BY p.created,p.id").map_err(err)?;
        let rows=stmt.query_map([library],|r|Ok(PlaylistSummary{id:r.get(0)?,name:r.get(1)?,count:r.get(2)?})).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;rows};
    let pins={let mut stmt=tx.prepare("SELECT path,color FROM pinned_paths WHERE library=?1 ORDER BY created,path").map_err(err)?;let rows=stmt.query_map([library],|r|Ok(PinEntry{path:r.get(0)?,color:r.get(1)?})).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;rows};
    let history_count=tx.query_row("SELECT COUNT(*) FROM play_history h JOIN asset_libraries l ON h.asset_id=l.asset_id WHERE l.library=?1",[library],|r|r.get(0)).map_err(err)?;
    // 收藏分组：空串视作未分组，只统计当前库内仍存在的已收藏素材。
    let mut group_counts:BTreeMap<String,u64>=BTreeMap::new();
    {
        let mut stmt=tx.prepare("SELECT COALESCE(TRIM(m.favorite_group),''),COUNT(*) FROM user_metadata m JOIN asset_libraries l ON l.asset_id=m.asset_id WHERE l.library=?1 AND COALESCE(m.favorite,0)=1 GROUP BY 1").map_err(err)?;
        for row in stmt.query_map([library],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?))).map_err(err)?{let (name,count)=row.map_err(err)?;*group_counts.entry(name).or_default()+=count;}
    }
    let favorite_count=group_counts.values().sum();
    let favorite_ungrouped=group_counts.get("").copied().unwrap_or(0);
    let favorite_groups=group_counts.into_iter().filter(|(name,_)|!name.is_empty()).map(|(name,count)|FavoriteGroup{name,count}).collect();
    tx.commit().map_err(err)?;Ok(LibraryOverview{tags,playlists,pins,history_count,favorite_count,favorite_ungrouped,favorite_groups})
}
fn pin(db:&Path,library:&str,path:&str,pinned:bool)->Result<(),String>{
    let library=validate_library(library)?;let prefix=folder_prefix(Some(path))?.ok_or("请选择素材目录。")?;
    let conn=connection(db)?;
    if pinned{
        let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM assets a JOIN asset_libraries l ON l.asset_id=a.id WHERE l.library=?1 AND substr(a.path,1,length(?2))=?2) OR EXISTS(SELECT 1 FROM catalog_directories WHERE library=?1 AND (path=?3 OR substr(path,1,length(?2))=?2))",params![library,prefix,path],|r|r.get(0)).map_err(err)?;
        if !valid{return Err("该目录没有当前库的登记素材，未添加常用路径。".into());}
        let count:u64=conn.query_row("SELECT COUNT(*) FROM pinned_paths WHERE library=?1",[library],|r|r.get(0)).map_err(err)?;
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pinned_paths WHERE library=?1 AND path=?2)",params![library,path],|r|r.get(0)).map_err(err)?;
        if !exists&&count>=100{return Err("每个库最多保留100个常用路径。".into());}
        conn.execute("INSERT OR IGNORE INTO pinned_paths(library,path,created) VALUES(?1,?2,?3)",params![library,path,now()]).map_err(err)?;
    }else{conn.execute("DELETE FROM pinned_paths WHERE library=?1 AND path=?2",params![library,path]).map_err(err)?;}
    Ok(())
}
/// 只给已置顶的路径上色；颜色是常用路径的属性，不单独存在。
fn pin_color(db:&Path,library:&str,path:&str,color:Option<&str>)->Result<(),String>{
    let library=validate_library(library)?;
    let color=match color.map(str::trim).filter(|value|!value.is_empty()){
        None=>None,
        Some(value)=>{
            if !PIN_COLORS.contains(&value){return Err("颜色标记不在允许范围内。".into());}
            Some(value)
        }
    };
    let conn=connection(db)?;
    let changed=conn.execute("UPDATE pinned_paths SET color=?1 WHERE library=?2 AND path=?3",params![color,library,path]).map_err(err)?;
    if changed==0{return Err("该路径不在常用路径中，未设置颜色。".into());}
    Ok(())
}
fn playlist_create(db:&Path,library:&str,name:&str)->Result<String,String>{
    let library=validate_library(library)?;let name=name.trim();
    if name.is_empty()||name.chars().count()>80||name.chars().any(char::is_control){return Err("播放列表名称须为1至80个可见字符。".into());}
    let mut conn=connection(db)?;let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    if tx.query_row::<u64,_,_>("SELECT COUNT(*) FROM playlists WHERE library=?1",[library],|r|r.get(0)).map_err(err)?>=100{return Err("每个库最多100个播放列表。".into());}
    let id=Uuid::new_v4().to_string();
    tx.execute("INSERT INTO playlists(id,library,name,created) VALUES(?1,?2,?3,?4)",params![id,library,name,now()]).map_err(|e|format!("无法新建播放列表（同库名称不得重复）：{e}"))?;
    tx.commit().map_err(err)?;Ok(id)
}
fn clean_ids(ids:Vec<String>)->Result<Vec<String>,String>{
    if ids.is_empty()||ids.len()>500||ids.iter().any(|id|id.is_empty()||id.len()>128){return Err("请选择1至500个有效素材ID。".into());}
    Ok(ids.into_iter().collect::<BTreeSet<_>>().into_iter().collect())
}
fn playlist_add(db:&Path,playlist_id:&str,ids:Vec<String>)->Result<u64,String>{
    let ids=clean_ids(ids)?;let mut conn=connection(db)?;let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    let library:String=tx.query_row("SELECT library FROM playlists WHERE id=?1",[playlist_id],|r|r.get(0)).optional().map_err(err)?.ok_or("播放列表不存在。")?;
    for id in &ids{
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM asset_libraries WHERE asset_id=?1 AND library=?2)",params![id,library],|r|r.get(0)).map_err(err)?;
        if !valid{return Err("部分素材不属于播放列表所在库，整批未添加。".into());}
    }
    let mut added=0;for id in ids{added+=tx.execute("INSERT OR IGNORE INTO playlist_items(playlist_id,asset_id,added) VALUES(?1,?2,?3)",params![playlist_id,id,now()]).map_err(err)? as u64;}
    tx.commit().map_err(err)?;Ok(added)
}
/// 仅在显式试听成功后调用；仅保留最近500条不同素材。
pub(crate) fn record_history(db:&Path,id:&str)->Result<(),String>{
    let mut conn=connection(db)?;let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    let changed=tx.execute("INSERT INTO play_history(asset_id,played_at) SELECT id,?2 FROM assets WHERE id=?1 ON CONFLICT(asset_id) DO UPDATE SET played_at=excluded.played_at",params![id,now()]).map_err(err)?;
    if changed==0{return Err("素材已出库，未记录试听历史。".into());}
    tx.execute("DELETE FROM play_history WHERE asset_id NOT IN (SELECT asset_id FROM play_history ORDER BY played_at DESC,asset_id LIMIT 500)",[]).map_err(err)?;
    tx.commit().map_err(err)?;Ok(())
}

#[derive(Clone)]
enum RemovalSelector{Ids(Vec<String>),Folder(String)}
#[derive(Clone)]
struct PendingRemoval{db:PathBuf,selector:RemovalSelector,digest:String,created:Instant}
static REMOVALS:OnceLock<Mutex<HashMap<String,PendingRemoval>>>=OnceLock::new();
fn pending()->&'static Mutex<HashMap<String,PendingRemoval>>{REMOVALS.get_or_init(||Mutex::new(HashMap::new()))}
#[derive(Serialize)]
pub struct RemovalPreview{
    pub token:String,pub assets:u64,pub directories:u64,pub bytes:u64,pub favorites:u64,pub tags:u64,pub notes:u64,
    pub playlists:u64,pub history:u64,pub paths:Vec<String>,
}
fn selector(ids:Option<Vec<String>>,folder:Option<String>)->Result<RemovalSelector,String>{
    match(ids,folder){(Some(ids),None)=>Ok(RemovalSelector::Ids(clean_ids(ids)?)),(None,Some(path))=>{folder_prefix(Some(&path))?;if path.is_empty(){return Err("目录不能为空。".into());}Ok(RemovalSelector::Folder(path))},_=>Err("必须且只能选择素材ID或一个目录。".into())}
}
fn selected_ids(conn:&Connection,selector:&RemovalSelector)->Result<Vec<String>,String>{
    match selector{
        RemovalSelector::Ids(ids)=>{for id in ids{let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",[id],|r|r.get(0)).map_err(err)?;if !valid{return Err("所选素材已变化或已出库，请刷新后重试。".into());}}Ok(ids.clone())},
        RemovalSelector::Folder(path)=>{let prefix=folder_prefix(Some(path))?.ok_or("目录不能为空。")?;let mut stmt=conn.prepare("SELECT id FROM assets WHERE substr(path,1,length(?1))=?1 ORDER BY id").map_err(err)?;let rows=stmt.query_map([prefix],|r|r.get(0)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;Ok(rows)}
    }
}
fn removal_snapshot(conn:&Connection,selector:&RemovalSelector)->Result<(RemovalPreview,Vec<String>,String),String>{
    let ids=selected_ids(conn,selector)?;let mut digest=Sha256::new();let mut result=RemovalPreview{token:String::new(),assets:ids.len() as u64,directories:0,bytes:0,favorites:0,tags:0,notes:0,playlists:0,history:0,paths:Vec::new()};
    for id in &ids{
        let (path,size,modified,name,status,favorite,tags):(String,u64,String,String,String,bool,String)=conn.query_row("SELECT a.path,a.size,a.modified,a.name,a.status,COALESCE(m.favorite,0),COALESCE(m.tags,'[]') FROM assets a LEFT JOIN user_metadata m ON m.asset_id=a.id WHERE a.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(err)?;
        let parsed:Vec<String>=serde_json::from_str(&tags).map_err(err)?;
        let associations={let mut stmt=conn.prepare("SELECT playlist_id FROM playlist_items WHERE asset_id=?1 ORDER BY playlist_id").map_err(err)?;let rows=stmt.query_map([id],|r|r.get::<_,String>(0)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;rows};
        let played:Option<String>=conn.query_row("SELECT played_at FROM play_history WHERE asset_id=?1",[id],|r|r.get(0)).optional().map_err(err)?;
        let libraries={let mut stmt=conn.prepare("SELECT library FROM asset_libraries WHERE asset_id=?1 ORDER BY library").map_err(err)?;let rows=stmt.query_map([id],|r|r.get::<_,String>(0)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;rows};
        digest.update(serde_json::to_vec(&(id,&path,size,modified,name,status,favorite,&tags,&associations,&played,libraries)).map_err(err)?);
        result.bytes=result.bytes.saturating_add(size);result.favorites+=favorite as u64;result.tags+=(!parsed.is_empty()) as u64;result.playlists+=associations.len() as u64;result.history+=played.is_some() as u64;
        // 展示量有界，实际数量和指纹覆盖全部选中记录。
        if result.paths.len()<100{result.paths.push(path);}
    }
    if let RemovalSelector::Folder(folder)=selector{
        let prefix=folder_prefix(Some(folder))?.ok_or("目录不能为空。")?;
        let mut stmt=conn.prepare("SELECT path,library FROM catalog_directories WHERE path=?1 OR substr(path,1,length(?2))=?2 ORDER BY path,library").map_err(err)?;
        let mut paths=BTreeSet::new();
        for row in stmt.query_map(params![folder,prefix],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(err)?{let row=row.map_err(err)?;digest.update(serde_json::to_vec(&row).map_err(err)?);paths.insert(row.0);}
        result.directories=paths.len() as u64;
    }
    Ok((result,ids,format!("{:x}",digest.finalize())))
}
fn remove_preview(db:&Path,selector:RemovalSelector)->Result<RemovalPreview,String>{
    let mut conn=connection(db)?;let tx=conn.transaction().map_err(err)?;let (mut preview,_,digest)=removal_snapshot(&tx,&selector)?;tx.commit().map_err(err)?;
    if preview.assets==0&&preview.directories==0{return Err("此范围没有已入库素材或登记目录。".into());}
    let token=Uuid::new_v4().to_string();let mut pending=pending().lock().map_err(|_|"出库预览锁不可用")?;
    pending.retain(|_,value|value.created.elapsed()<Duration::from_secs(300));
    if pending.len()>=32{return Err("待确认的出库预览过多，请稍后重试。".into());}
    pending.insert(token.clone(),PendingRemoval{db:db.to_owned(),selector,digest,created:Instant::now()});preview.token=token;Ok(preview)
}
fn remove_commit(db:&Path,token:&str)->Result<u64,String>{
    let preview=pending().lock().map_err(|_|"出库预览锁不可用")?.get(token).cloned().ok_or("出库预览已失效，请重新预览。")?;
    if preview.db!=db||preview.created.elapsed()>=Duration::from_secs(300){return Err("出库预览已失效，请重新预览。".into());}
    let mut conn=connection(db)?;let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    let (summary,ids,digest)=removal_snapshot(&tx,&preview.selector)?;
    if digest!=preview.digest{return Err("选中素材或关联在预览后发生变化，未出库；请重新预览确认。".into());}
    for id in &ids{tx.execute("DELETE FROM assets WHERE id=?1",[id]).map_err(err)?;}
    if let RemovalSelector::Folder(folder)=&preview.selector{
        let prefix=folder_prefix(Some(folder))?.ok_or("目录不能为空。")?;
        tx.execute("DELETE FROM pinned_paths WHERE path=?1 OR substr(path,1,length(?2))=?2",params![folder,prefix]).map_err(err)?;
        tx.execute("DELETE FROM catalog_directories WHERE path=?1 OR substr(path,1,length(?2))=?2",params![folder,prefix]).map_err(err)?;
    }
    tx.execute("INSERT INTO meta(key,value) VALUES('metadata_revision',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[now()]).map_err(err)?;
    tx.commit().map_err(err)?;pending().lock().map_err(|_|"出库预览锁不可用")?.remove(token);Ok(summary.assets)
}

#[tauri::command]
pub async fn library_overview(library:String,state:tauri::State<'_,CatalogState>)->Result<LibraryOverview,String>{let db=state.database_path();tauri::async_runtime::spawn_blocking(move||overview(&db,&library)).await.map_err(err)?}
#[tauri::command]
pub async fn library_pin(library:String,path:String,pinned:bool,state:tauri::State<'_,CatalogState>)->Result<(),String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|pin(db,&library,&path,pinned))).await.map_err(err)?}
#[tauri::command]
pub async fn library_pin_color(library:String,path:String,color:Option<String>,state:tauri::State<'_,CatalogState>)->Result<(),String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|pin_color(db,&library,&path,color.as_deref()))).await.map_err(err)?}
#[tauri::command]
pub async fn library_playlist_create(library:String,name:String,state:tauri::State<'_,CatalogState>)->Result<String,String>{let db=state.database_path();tauri::async_runtime::spawn_blocking(move||playlist_create(&db,&library,&name)).await.map_err(err)?}
#[tauri::command]
pub async fn library_playlist_add(playlist_id:String,ids:Vec<String>,state:tauri::State<'_,CatalogState>)->Result<u64,String>{let db=state.database_path();tauri::async_runtime::spawn_blocking(move||playlist_add(&db,&playlist_id,ids)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_remove_preview(ids:Option<Vec<String>>,folder:Option<String>,state:tauri::State<'_,CatalogState>)->Result<RemovalPreview,String>{let selector=selector(ids,folder)?;let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|remove_preview(db,selector))).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_remove_commit(token:String,state:tauri::State<'_,CatalogState>)->Result<u64,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|remove_commit(db,&token))).await.map_err(err)?}

#[cfg(test)]
mod tests{
    use super::*;
    struct Fixture{root:PathBuf,db:PathBuf,_state:CatalogState}
    impl Fixture{
        fn new()->Self{let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/gui-context-validation/test-data").join(Uuid::new_v4().to_string());fs::create_dir_all(root.join("a")).unwrap();fs::create_dir_all(root.join("ab")).unwrap();let db=root.join("catalog.sqlite");let state=CatalogState::new(db.clone()).unwrap();Self{root,db,_state:state}}
        fn asset(&self,id:&str,folder:&str,library:&str){let path=self.root.join(folder).join(format!("{id}.wav"));fs::write(&path,b"source remains").unwrap();let conn=connection(&self.db).unwrap();conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,status) VALUES(?1,?2,?2,?3,'WAV',14,'123','ready')",params![id,path_string(&path).unwrap(),format!("{id}.wav")]).unwrap();conn.execute("INSERT INTO asset_libraries(asset_id,library) VALUES(?1,?2)",params![id,library]).unwrap();}
    }
    #[test]
    fn collections_filters_exact_tags_and_library_scoped_playlists_history(){
        let f=Fixture::new();f.asset("one","a","sound");f.asset("two","a","music");set_tags(&f.db,"one",vec!["雨".into(),"雨声".into()]).unwrap();set_tags(&f.db,"two",vec!["雨".into()]).unwrap();
        record_history(&f.db,"two").unwrap();record_history(&f.db,"one").unwrap();record_history(&f.db,"one").unwrap();
        let id=playlist_create(&f.db,"sound","工作用").unwrap();assert_eq!(playlist_add(&f.db,&id,vec!["one".into()]).unwrap(),1);assert_eq!(playlist_add(&f.db,&id,vec!["one".into()]).unwrap(),0);assert!(playlist_add(&f.db,&id,vec!["one".into(),"two".into()]).is_err());
        let sound=overview(&f.db,"sound").unwrap();assert_eq!(sound.history_count,1);assert_eq!(sound.tags[0].name,"雨");assert_eq!(sound.tags[0].count,1);assert_eq!(sound.playlists[0].count,1);assert!(overview(&f.db,"music").unwrap().playlists.is_empty());
        assert_eq!(search_page(&f.db,0,10,SearchFilter{library:Some("sound"),tag:Some("雨"),..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,10,SearchFilter{tag:Some("雨声"),..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,10,SearchFilter{library:Some("music"),playlist_id:Some(&id),..Default::default()}).unwrap().total,0);
        assert_eq!(search_page(&f.db,0,10,SearchFilter{history_only:true,..Default::default()}).unwrap().items[0].id,"one");
        assert!(playlist_create(&f.db,"sound","\n").is_err());assert!(record_history(&f.db,"missing").is_err());
    }
    #[test]
    fn collections_pins_survive_reopen_and_respect_library(){
        let f=Fixture::new();f.asset("one","a","sound");let folder=path_string(&f.root.join("a")).unwrap();pin(&f.db,"sound",&folder,true).unwrap();pin(&f.db,"sound",&folder,true).unwrap();assert!(pin(&f.db,"music",&folder,true).is_err());
        let first=overview(&f.db,"sound").unwrap().pins;assert_eq!(first.len(),1);assert_eq!(first[0].path,folder);assert!(first[0].color.is_none());
        let conn=connection(&f.db).unwrap();initialize(&conn).unwrap();assert_eq!(overview(&f.db,"sound").unwrap().pins.len(),1);
        pin(&f.db,"sound",&folder,false).unwrap();assert!(overview(&f.db,"sound").unwrap().pins.is_empty());
    }
    #[test]
    fn pin_color_is_validated_stored_per_library_and_cleared_with_the_pin(){
        let f=Fixture::new();f.asset("one","a","sound");f.asset("two","ab","sound");let a=path_string(&f.root.join("a")).unwrap();let ab=path_string(&f.root.join("ab")).unwrap();
        assert!(pin_color(&f.db,"sound",&a,Some("red")).is_err());
        pin(&f.db,"sound",&a,true).unwrap();pin(&f.db,"sound",&ab,true).unwrap();
        assert!(pin_color(&f.db,"sound",&a,Some("neon")).is_err());
        assert!(pin_color(&f.db,"music",&a,Some("red")).is_err());
        pin_color(&f.db,"sound",&a,Some("red")).unwrap();pin_color(&f.db,"sound",&ab,Some("blue")).unwrap();
        let colors=overview(&f.db,"sound").unwrap().pins.into_iter().map(|entry|(entry.path,entry.color)).collect::<Vec<_>>();
        assert_eq!(colors,vec![(a.clone(),Some("red".to_string())),(ab,Some("blue".to_string()))]);
        pin_color(&f.db,"sound",&a,Some("  ")).unwrap();
        assert!(overview(&f.db,"sound").unwrap().pins.iter().find(|entry|entry.path==a).unwrap().color.is_none());
        let conn=connection(&f.db).unwrap();initialize(&conn).unwrap();
        assert_eq!(overview(&f.db,"sound").unwrap().pins.iter().filter(|entry|entry.color.is_some()).count(),1);
    }
    #[test]
    fn collections_history_is_bounded_and_replaying_moves_one_item_to_front(){
        let f=Fixture::new();f.asset("latest","a","sound");
        let mut conn=connection(&f.db).unwrap();let tx=conn.transaction().unwrap();
        for i in 0..501{let id=format!("old-{i:03}");tx.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,status) VALUES(?1,?1,?1,?1,'WAV',1,'1','ready')",[&id]).unwrap();tx.execute("INSERT INTO play_history(asset_id,played_at) VALUES(?1,?2)",params![id,format!("{i:020}")]).unwrap();}
        tx.commit().unwrap();record_history(&f.db,"latest").unwrap();record_history(&f.db,"latest").unwrap();
        assert_eq!(conn.query_row::<u64,_,_>("SELECT COUNT(*) FROM play_history",[],|r|r.get(0)).unwrap(),500);
        assert!(!conn.query_row::<bool,_,_>("SELECT EXISTS(SELECT 1 FROM play_history WHERE asset_id='old-000')",[],|r|r.get(0)).unwrap());
        assert_eq!(search_page(&f.db,0,10,SearchFilter{history_only:true,..Default::default()}).unwrap().items[0].id,"latest");
    }
    #[test]
    fn favorite_groups_are_counted_per_library_and_cleared_with_unfavorite(){
        let f=Fixture::new();f.asset("one","a","sound");f.asset("two","ab","sound");f.asset("three","a","music");
        set_favorite_group(&f.db,vec!["one".into(),"two".into(),"three".into()],Some("对白")).unwrap();
        let sound=overview(&f.db,"sound").unwrap();
        assert_eq!((sound.favorite_count,sound.favorite_ungrouped),(2,0));assert_eq!(sound.favorite_groups[0].name,"对白");assert_eq!(sound.favorite_groups[0].count,2);
        assert_eq!(overview(&f.db,"music").unwrap().favorite_count,1);
        set_favorite_group(&f.db,vec!["two".into()],None).unwrap();
        let sound=overview(&f.db,"sound").unwrap();assert_eq!((sound.favorite_count,sound.favorite_ungrouped),(2,1));assert_eq!(sound.favorite_groups[0].count,1);
        set_favorite(&f.db,"one",false).unwrap();
        let sound=overview(&f.db,"sound").unwrap();assert_eq!((sound.favorite_count,sound.favorite_groups.len()),(1,0));assert_eq!(sound.favorite_ungrouped,1);
    }
    #[test]
    fn collections_removal_cascades_both_libraries_and_associations_preserves_disk_and_sibling(){
        let f=Fixture::new();f.asset("one","a","sound");f.asset("two","ab","sound");assign_library(&f.db,vec!["one".into()],"music").unwrap();set_favorite(&f.db,"one",true).unwrap();set_tags(&f.db,"one",vec!["雨".into()]).unwrap();record_history(&f.db,"one").unwrap();let playlist=playlist_create(&f.db,"sound","列表").unwrap();playlist_add(&f.db,&playlist,vec!["one".into()]).unwrap();let folder=path_string(&f.root.join("a")).unwrap();pin(&f.db,"sound",&folder,true).unwrap();
        let preview=remove_preview(&f.db,RemovalSelector::Folder(folder)).unwrap();assert_eq!((preview.assets,preview.favorites,preview.tags,preview.playlists,preview.history),(1,1,1,1,1));assert_eq!(remove_commit(&f.db,&preview.token).unwrap(),1);assert!(remove_commit(&f.db,&preview.token).is_err());
        assert_eq!(search_page(&f.db,0,10,SearchFilter::default()).unwrap().items[0].id,"two");assert_eq!(search_page(&f.db,0,10,SearchFilter{library:Some("music"),..Default::default()}).unwrap().total,0);
        let conn=connection(&f.db).unwrap();for table in ["user_metadata","playlist_items","play_history"]{assert_eq!(conn.query_row::<u64,_,_>(&format!("SELECT COUNT(*) FROM {table}"),[],|r|r.get(0)).unwrap(),0);}assert_eq!(overview(&f.db,"sound").unwrap().playlists[0].count,0);assert!(overview(&f.db,"sound").unwrap().pins.is_empty());assert_eq!(fs::read(f.root.join("a/one.wav")).unwrap(),b"source remains");
    }
    #[test]
    fn collections_removal_rejects_changed_scope_or_annotations_without_partial_deletion(){
        let f=Fixture::new();f.asset("one","a","sound");let folder=path_string(&f.root.join("a")).unwrap();let preview=remove_preview(&f.db,RemovalSelector::Folder(folder)).unwrap();f.asset("two","a","sound");assert!(remove_commit(&f.db,&preview.token).is_err());assert_eq!(search_page(&f.db,0,10,SearchFilter::default()).unwrap().total,2);
        let preview=remove_preview(&f.db,RemovalSelector::Ids(vec!["one".into()])).unwrap();set_tags(&f.db,"one",vec!["刚修改".into()]).unwrap();assert!(remove_commit(&f.db,&preview.token).is_err());assert_eq!(search_page(&f.db,0,10,SearchFilter::default()).unwrap().total,2);
        assert!(selector(None,None).is_err());assert!(selector(Some(vec![]),None).is_err());assert!(selector(Some(vec!["one".into()]),Some(path_string(&f.root).unwrap())).is_err());
    }
    #[test]
    fn collections_explicit_empty_directories_persist_are_scoped_and_can_leave_without_disk_delete(){
        let f=Fixture::new();f.asset("one","ab","sound");let folder=path_string(&f.root.join("a")).unwrap();
        let conn=connection(&f.db).unwrap();conn.execute("INSERT INTO catalog_directories(path,library) VALUES(?1,'music')",[&folder]).unwrap();
        let music=folder_nodes_in_library(&f.db,Some("music")).unwrap();assert!(music.iter().any(|node|node.path==folder&&node.count==0));assert!(music.iter().all(|node|node.count==0));assert!(!folder_nodes_in_library(&f.db,Some("sound")).unwrap().iter().any(|node|node.path==folder));
        initialize(&conn).unwrap();pin(&f.db,"music",&folder,true).unwrap();let preview=remove_preview(&f.db,RemovalSelector::Folder(folder.clone())).unwrap();assert_eq!((preview.assets,preview.directories),(0,1));assert_eq!(remove_commit(&f.db,&preview.token).unwrap(),0);assert!(!folder_nodes_in_library(&f.db,Some("music")).unwrap().iter().any(|node|node.path==folder));assert!(f.root.join("a").is_dir());assert_eq!(search_page(&f.db,0,10,SearchFilter::default()).unwrap().total,1);
    }
}
