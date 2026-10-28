//! 本地开发账号/发行端适配与离线签名验证；不包含发行私钥，不锁用户基础素材功能。
use serde::{Serialize,Deserialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs::{self,File,OpenOptions},io::{Read,Write},path::{Path,PathBuf},net::{TcpStream,SocketAddr,IpAddr,Ipv4Addr},sync::{Arc,Mutex},time::{Duration,SystemTime,UNIX_EPOCH}};
const LIMIT:usize=128*1024;
const ISSUER:&str="sound-workbench-dev-v1";
fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn now()->u64{SystemTime::now().duration_since(UNIX_EPOCH).map(|v|v.as_secs()).unwrap_or(0)}
#[derive(Clone)]pub struct LicenseState{root:PathBuf,memory:Arc<Mutex<Memory>>}
#[derive(Default)]struct Memory{session:Option<String>}
#[derive(Serialize,Deserialize)]struct Config{endpoint:String,public_key_sha256:String}
#[derive(Debug,Serialize,Deserialize)]#[serde(deny_unknown_fields)]
struct OfflinePayload{issuer:String,version:u32,license_id:String,subject:String,order_id:String,device_id:String,issued_at:u64,expires_at:Option<u64>,features:Vec<String>}
fn ensure_root(root:&Path)->Result<(),String>{
 if !root.is_absolute(){return Err("账号配置目录必须是绝对路径。".into());}
 for path in root.ancestors(){if let Ok(metadata)=fs::symlink_metadata(path){if metadata.file_type().is_symlink(){return Err("账号配置不接受链接目录。".into());}#[cfg(windows)]{use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Err("账号配置不接受目录联接。".into());}}}}
 fs::create_dir_all(root).map_err(err)
}
fn read(path:&Path,limit:usize)->Result<Vec<u8>,String>{let metadata=fs::symlink_metadata(path).map_err(err)?;if !metadata.is_file()||metadata.file_type().is_symlink(){return Err("配置不是普通文件。".into());}#[cfg(windows)]{use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Err("配置不能是重解析点。".into());}}let mut bytes=Vec::new();File::open(path).map_err(err)?.take(limit as u64+1).read_to_end(&mut bytes).map_err(err)?;if bytes.len()>limit{return Err("账号数据超过大小上限。".into());}Ok(bytes)}
fn write(root:&Path,name:&str,data:&[u8])->Result<(),String>{ensure_root(root)?;let temporary=root.join(format!("{}.partial",uuid::Uuid::new_v4()));let result=(||{let mut file=OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(err)?;file.write_all(data).map_err(err)?;file.sync_all().map_err(err)?;drop(file);fs::rename(&temporary,root.join(name)).map_err(err)})();if result.is_err(){let _=fs::remove_file(&temporary);}result}
fn endpoint(value:&str)->Result<u16,String>{let clean=value.trim().trim_end_matches('/');let port=clean.strip_prefix("http://127.0.0.1:").ok_or("开发服务仅允许http://127.0.0.1:端口，不连接公网。")?.parse::<u16>().map_err(|_|"本地服务端口无效。")?;if port<1024{return Err("开发端口应为1024至65535。".into());}Ok(port)}
fn decode_hex(value:&str,max:usize)->Result<Vec<u8>,String>{if value.len()%2!=0||value.len()>max*2{return Err("凭证编码或长度无效。".into());}value.as_bytes().chunks_exact(2).map(|pair|{let s=std::str::from_utf8(pair).map_err(err)?;u8::from_str_radix(s,16).map_err(|_|"凭证不是有效十六进制。".into())}).collect()}
fn validate_public(bytes:&[u8])->Result<(),String>{
 if bytes.len()<24||bytes.len()>1024{return Err("仅接受开发发行端RSA2048公开BLOB。".into());}
 let values:Vec<u32>=bytes[..24].chunks_exact(4).map(|v|u32::from_le_bytes(v.try_into().unwrap())).collect();
 if values[0]!=0x31415352||values[1]!=2048||values[2]!=3||values[3]!=256||values[4]!=0||values[5]!=0||bytes.len()!=283||bytes[24..27]!=[1,0,1]{return Err("公开BLOB必须是RSA2048/65537，不能导入私钥。".into());}Ok(())
}
#[cfg(windows)]fn verify_signature(public:&[u8],payload:&[u8],signature:&[u8])->Result<(),String>{
 use windows::Win32::Security::Cryptography::*;
 validate_public(public)?;
 unsafe{
  struct Handles{algorithm:BCRYPT_ALG_HANDLE,key:BCRYPT_KEY_HANDLE}impl Drop for Handles{fn drop(&mut self){unsafe{if !self.key.0.is_null(){let _=BCryptDestroyKey(self.key);}if !self.algorithm.0.is_null(){let _=BCryptCloseAlgorithmProvider(self.algorithm,0);}}}}
  let mut handles=Handles{algorithm:Default::default(),key:Default::default()};
  BCryptOpenAlgorithmProvider(&mut handles.algorithm,BCRYPT_RSA_ALGORITHM,None,BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0)).ok().map_err(|_|"无法打开系统RSA验证器。")?;
  BCryptImportKeyPair(handles.algorithm,None,BCRYPT_RSAPUBLIC_BLOB,&mut handles.key,public,0).ok().map_err(|_|"受信公钥无法导入。")?;
  let padding=BCRYPT_PKCS1_PADDING_INFO{pszAlgId:BCRYPT_SHA256_ALGORITHM};let digest=Sha256::digest(payload);
  BCryptVerifySignature(handles.key,Some((&padding as *const BCRYPT_PKCS1_PADDING_INFO).cast()),&digest,signature,BCRYPT_PAD_PKCS1).ok().map_err(|_|"凭证签名无效或不是受信发行端。".to_string())
 }
}
#[cfg(not(windows))]fn verify_signature(_: &[u8],_: &[u8],_: &[u8])->Result<(),String>{Err("此开发验证适配仅支持Windows CNG。".into())}
fn verify_token(token:&str,public:&[u8],device:&str,current:u64)->Result<OfflinePayload,String>{
 if token.len()>32768{return Err("离线凭证过大。".into());}let (payload,signature)=token.trim().split_once('.').ok_or("离线凭证格式无效。")?;let payload=decode_hex(payload,8192)?;let signature=decode_hex(signature,256)?;if signature.len()!=256{return Err("签名长度无效。".into());}verify_signature(public,&payload,&signature)?;
 let value:OfflinePayload=serde_json::from_slice(&payload).map_err(|_|"已签名凭证内容格式无效。")?;
 if value.issuer!=ISSUER||value.version!=1||value.device_id!=device||value.issued_at>current.saturating_add(300)||value.expires_at.is_some_and(|expiry|expiry<=current)||value.license_id.is_empty()||value.subject.is_empty()||value.order_id.is_empty()||value.features!=["buyout-local"]{return Err("凭证设备、发行信息、权限或有效期无效。".into());}Ok(value)
}
fn http(port:u16,action:&str,payload:&Value,session:Option<&str>)->Result<Value,String>{
 let body=serde_json::to_vec(payload).map_err(err)?;if body.len()>64*1024{return Err("账号请求超过64KiB。".into());}
 let address=SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST),port);let mut stream=TcpStream::connect_timeout(&address,Duration::from_secs(2)).map_err(|_|"本地开发服务未运行；未自动启动或连接公网。")?;stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(err)?;stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(err)?;
 let authorization=if let Some(token)=session{if token.is_empty()||token.len()>256||!token.bytes().all(|v|v.is_ascii_alphanumeric()||v==b'_'||v==b'-'){return Err("登录状态格式无效，请重新登录。".into());}format!("Authorization: Bearer {token}\r\n")}else{String::new()};
 let header=format!("POST /v1/{action} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{authorization}Connection: close\r\n\r\n",body.len());stream.write_all(header.as_bytes()).and_then(|_|stream.write_all(&body)).map_err(|_|"开发请求发送失败。")?;
 let mut response=Vec::new();stream.take(LIMIT as u64+1).read_to_end(&mut response).map_err(|_|"开发服务响应超时或不可读。")?;if response.len()>LIMIT{return Err("开发响应超过128KiB。".into());}let boundary=response.windows(4).position(|v|v==b"\r\n\r\n").ok_or("开发响应不是HTTP报文。")?;
 let head=std::str::from_utf8(&response[..boundary]).map_err(err)?;if !head.starts_with("HTTP/1.0 ")&&!head.starts_with("HTTP/1.1 "){return Err("开发响应协议无效。".into());}
 let length=head.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length:").map(|v|v.trim().parse::<usize>())).ok_or("开发响应缺少长度。")?.map_err(err)?;
 let body=&response[boundary+4..];if body.len()!=length{return Err("开发响应长度不一致。".into());}let value:Value=serde_json::from_slice(body).map_err(|_|"开发响应JSON无效。")?;if value.get("ok").and_then(Value::as_bool)!=Some(true){return Err(value.get("error").and_then(Value::as_str).unwrap_or("开发账号请求失败。").chars().take(400).collect());}Ok(value.get("result").cloned().unwrap_or(Value::Null))
}
impl LicenseState{
 pub fn new(root:PathBuf)->Self{Self{root,memory:Arc::new(Mutex::new(Default::default()))}}
 fn device(&self)->Result<String,String>{ensure_root(&self.root)?;let path=self.root.join("device-id.txt");if !path.exists(){let id=uuid::Uuid::new_v4().to_string();match OpenOptions::new().write(true).create_new(true).open(&path){Ok(mut file)=>file.write_all(id.as_bytes()).map_err(err)?,Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(e)=>return Err(err(e))}}let id=String::from_utf8(read(&path,128)?).map_err(err)?;uuid::Uuid::parse_str(&id).map_err(|_|"设备标识损坏，未重置以绕过设备上限。")?;Ok(id)}
 fn config(&self)->Result<Config,String>{let config:Config=serde_json::from_slice(&read(&self.root.join("development.json"),4096)?).map_err(err)?;endpoint(&config.endpoint)?;let public=self.public()?;if format!("{:x}",Sha256::digest(public))!=config.public_key_sha256{return Err("受信公钥与配置不一致，未验证凭证。".into());}Ok(config)}
 fn public(&self)->Result<Vec<u8>,String>{let public=read(&self.root.join("issuer-public.blob"),1024)?;validate_public(&public)?;Ok(public)}
 fn offline(&self)->Result<OfflinePayload,String>{let config=self.config()?;let _=config;let token=String::from_utf8(read(&self.root.join("offline-token.txt"),32768)?).map_err(err)?;verify_token(&token,&self.public()?,&self.device()?,now())}
 fn status(&self)->Result<Value,String>{let device=self.device()?;let config=self.config();let offline=if self.root.join("offline-token.txt").exists(){match self.offline(){Ok(value)=>json!({"state":"verified_development","license_id":value.license_id,"subject":value.subject,"order_id":value.order_id,"expires_at":value.expires_at,"features":value.features}),Err(e)=>json!({"state":"invalid","error":e})}}else{json!({"state":"none"})};let memory=self.memory.lock().map_err(|_|"账号状态锁不可用")?;Ok(json!({"development":true,"production_configured":false,"basic_features_allowed":true,"device_id":device,"configured":config.is_ok(),"endpoint":config.as_ref().map(|v|v.endpoint.clone()).ok(),"public_key_sha256":config.as_ref().map(|v|v.public_key_sha256.clone()).ok(),"logged_in":memory.session.is_some(),"offline":offline,"notice":"仅本地开发账号与模拟订单；没有真实购买。解绑/退款在联网核验后生效，永久离线凭证不能即时撤销。"}))}
 fn configure(&self,url:&str,key_path:&str)->Result<Value,String>{let port=endpoint(url)?;let path=Path::new(key_path);if !path.is_absolute(){return Err("受信公钥必须是绝对文件路径。".into());}let public=read(path,1024)?;validate_public(&public)?;let fingerprint=format!("{:x}",Sha256::digest(&public));write(&self.root,"issuer-public.blob",&public)?;write(&self.root,"development.json",&serde_json::to_vec(&Config{endpoint:format!("http://127.0.0.1:{port}"),public_key_sha256:fingerprint}).map_err(err)?)?;self.memory.lock().map_err(|_|"账号状态锁不可用")?.session=None;self.status()}
 fn import(&self,token:&str)->Result<Value,String>{self.config()?;let value=verify_token(token,&self.public()?,&self.device()?,now())?;write(&self.root,"offline-token.txt",token.trim().as_bytes())?;Ok(json!({"verified":true,"development":true,"license_id":value.license_id,"expires_at":value.expires_at}))}
 fn remove_local(&self){let _=fs::remove_file(self.root.join("offline-token.txt"));}
 fn request(&self,action:&str,mut payload:Value)->Result<Value,String>{
  if !["register","login","logout","account","create_order","test_paid","activate","deactivate","refund"].contains(&action){return Err("未支持此开发账号动作。".into());}if !payload.is_object(){return Err("请求必须是JSON对象。".into());}let config=self.config()?;let device=self.device()?;
  if ["account","activate","deactivate"].contains(&action)&&payload.get("device_id").is_none(){payload["device_id"]=json!(device);}
  if ["test_paid","refund"].contains(&action)&&payload.get("event_id").is_none(){payload["event_id"]=json!(uuid::Uuid::new_v4().to_string());}
  let session=self.memory.lock().map_err(|_|"账号状态锁不可用")?.session.clone();let mut result=http(endpoint(&config.endpoint)?,action,&payload,session.as_deref())?;
  if action=="login"{let token=result.get("session").and_then(Value::as_str).ok_or("开发登录缺少会话。")?;if token.len()>256||!token.bytes().all(|v|v.is_ascii_alphanumeric()||v==b'_'||v==b'-'){return Err("开发会话格式无效。".into());}self.memory.lock().map_err(|_|"账号状态锁不可用")?.session=Some(token.into());result.as_object_mut().ok_or("开发登录响应无效。")?.remove("session");}
  if action=="logout"{self.memory.lock().map_err(|_|"账号状态锁不可用")?.session=None;}
  if action=="activate"{let token=result.get("offline_token").and_then(Value::as_str).ok_or("发行端没有提供离线凭证。")?;self.import(token)?;result.as_object_mut().ok_or("开发激活响应无效。")?.remove("offline_token");}
  if action=="deactivate"&&payload.get("device_id").and_then(Value::as_str)==Some(device.as_str()){self.remove_local();}
  if action=="refund"{if self.offline().ok().is_some_and(|v|payload.get("order_id").and_then(Value::as_str)==Some(v.order_id.as_str())){self.remove_local();}}
  if action=="account"{let current=&result["current_device"];if current.is_null()||current.get("active").and_then(Value::as_i64)!=Some(1)||current.get("order_state").and_then(Value::as_str)!=Some("paid"){self.remove_local();}}
  Ok(result)
 }
}
#[tauri::command]pub async fn licensing_status(state:tauri::State<'_,LicenseState>)->Result<Value,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.status()).await.map_err(err)?}
#[tauri::command]pub async fn licensing_configure_dev(endpoint:String,public_key_path:String,state:tauri::State<'_,LicenseState>)->Result<Value,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.configure(&endpoint,&public_key_path)).await.map_err(err)?}
#[tauri::command]pub async fn licensing_request(action:String,payload:Value,state:tauri::State<'_,LicenseState>)->Result<Value,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.request(&action,payload)).await.map_err(err)?}
#[tauri::command]pub async fn licensing_import_offline(token:String,state:tauri::State<'_,LicenseState>)->Result<Value,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.import(&token)).await.map_err(err)?}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn licensing_endpoint_and_key_boundary(){for value in ["https://example.com","http://localhost:48761","http://127.0.0.1:80","http://127.0.0.1:48761/path","http://127.0.0.1:48761@evil"]{assert!(endpoint(value).is_err(),"{value}");}assert_eq!(endpoint("http://127.0.0.1:48761/").unwrap(),48761);assert!(validate_public(&[0;283]).is_err());assert!(decode_hex("xyz",20).is_err());assert!(decode_hex("00",0).is_err());}
 #[test]fn licensing_state_never_blocks_basic_features(){let root=std::env::temp_dir().join(format!("sound-workbench-license-rust-{}",uuid::Uuid::new_v4()));let state=LicenseState::new(root.clone());let first=state.status().unwrap();let second=state.status().unwrap();assert_eq!(first["device_id"],second["device_id"]);assert_eq!(first["basic_features_allowed"],true);assert_eq!(first["production_configured"],false);assert_eq!(first["configured"],false);assert!(state.import("not-a-token").is_err());assert!(state.request("payment",json!({})).is_err());fs::remove_dir_all(root).unwrap();}

 #[cfg(windows)]
 #[test]fn licensing_python_cng_interop_tamper_device_and_expiry(){
  use std::{process::{Command,Stdio},thread,time::Instant};
  use std::os::windows::process::CommandExt;
  let root=std::env::temp_dir().join(format!("sound-workbench-license-interop-{}",uuid::Uuid::new_v4()));
  let state=LicenseState::new(root.clone());let device=state.device().unwrap();let current=now();
  let payload=json!({"issuer":ISSUER,"version":1,"license_id":uuid::Uuid::new_v4().to_string(),"subject":"local-test-subject","order_id":"local-test-order","device_id":device,"issued_at":current,"expires_at":null,"features":["buyout-local"]});
  let service=Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/license-dev-service.py");
  let python=PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Programs/Python/Python312/python.exe");
  let script=r#"import importlib.util,json,sys
spec=importlib.util.spec_from_file_location('issuer',sys.argv[1]);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
r=m.Rsa();p=json.loads(sys.argv[2]);out={}
for name,patch in [('valid',{}),('expired',{'expires_at':p['issued_at']-1}),('future',{'issued_at':p['issued_at']+1000}),('other_device',{'device_id':'00000000-0000-0000-0000-000000000000'}),('unknown_issuer',{'issuer':'unknown'})]:
 value={**p,**patch};raw=m.encode(value);out[name]=raw.hex()+'.'+r.sign(raw).hex()
other=m.Rsa();out['untrusted_public']=other.export().hex();out['public']=r.export().hex();print(json.dumps(out));other.close();r.close()
"#;
  let mut child=Command::new(python).args(["-B","-X","utf8","-c",script]).arg(service).arg(payload.to_string()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(0x08000000).spawn().unwrap();
  let start=Instant::now();loop{if child.try_wait().unwrap().is_some(){break;}if start.elapsed()>Duration::from_secs(15){let _=child.kill();let _=child.wait();panic!("development CNG fixture timed out");}thread::sleep(Duration::from_millis(20));}
  let output=child.wait_with_output().unwrap();assert!(output.status.success(),"fixture generation failed");assert!(output.stdout.len()<32768);
  let result:Value=serde_json::from_slice(&output.stdout).unwrap();let public=decode_hex(result["public"].as_str().unwrap(),1024).unwrap();
  let token=result["valid"].as_str().unwrap();assert_eq!(verify_token(token,&public,&device,current).unwrap().order_id,"local-test-order");
  for name in ["expired","future","other_device","unknown_issuer"]{assert!(verify_token(result[name].as_str().unwrap(),&public,&device,current).is_err(),"{name}");}
  let other=decode_hex(result["untrusted_public"].as_str().unwrap(),1024).unwrap();assert!(verify_token(token,&other,&device,current).is_err());
  let mut tampered=token.as_bytes().to_vec();let last=tampered.last_mut().unwrap();*last=if *last==b'0'{b'1'}else{b'0'};assert!(verify_token(std::str::from_utf8(&tampered).unwrap(),&public,&device,current).is_err());
  let input=root.join("input-public.blob");fs::write(&input,&public).unwrap();state.configure("http://127.0.0.1:48761",input.to_str().unwrap()).unwrap();
  state.import(token).unwrap();assert_eq!(state.status().unwrap()["offline"]["state"],"verified_development");
  fs::write(root.join("issuer-public.blob"),&other).unwrap();assert_eq!(state.status().unwrap()["offline"]["state"],"invalid");assert_eq!(state.status().unwrap()["basic_features_allowed"],true);
  drop(state);fs::remove_dir_all(root).unwrap();
 }

 #[cfg(windows)]
 #[test]fn licensing_loopback_account_activation_and_refund_roundtrip(){
  use std::{process::{Command,Stdio},thread,time::Instant};
  use std::os::windows::process::CommandExt;
  let root=std::env::temp_dir().join(format!("sound-workbench-license-http-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
  let listener=std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).unwrap();let port=listener.local_addr().unwrap().port();drop(listener);
  let python=PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Programs/Python/Python312/python.exe");let service=Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/license-dev-service.py");
  let child=Command::new(python).args(["-B","-X","utf8"]).arg(service).arg("--data-dir").arg(root.join("issuer")).arg("--port").arg(port.to_string()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().unwrap();
  struct Service(std::process::Child);impl Drop for Service{fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
  let mut process=Service(child);let deadline=Instant::now();loop{if TcpStream::connect_timeout(&SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST),port),Duration::from_millis(100)).is_ok(){break;}assert!(process.0.try_wait().unwrap().is_none(),"development service quit early");assert!(deadline.elapsed()<Duration::from_secs(10),"development service startup timed out");thread::sleep(Duration::from_millis(20));}
  let state=LicenseState::new(root.join("client"));state.configure(&format!("http://127.0.0.1:{port}"),root.join("issuer/issuer-public.blob").to_str().unwrap()).unwrap();
  let credentials=json!({"email":"native-client@local.invalid","password":"test-password-for-native-client"});state.request("register",credentials.clone()).unwrap();let login=state.request("login",credentials).unwrap();assert!(login.get("session").is_none());assert_eq!(state.status().unwrap()["logged_in"],true);
  let order=state.request("create_order",json!({})).unwrap()["order_id"].as_str().unwrap().to_string();let event=json!({"order_id":order,"event_id":"native-repeated-test-event"});state.request("test_paid",event.clone()).unwrap();assert_eq!(state.request("test_paid",event).unwrap()["idempotent"],true);
  let activate=state.request("activate",json!({"order_id":order})).unwrap();assert!(activate.get("offline_token").is_none());assert_eq!(state.status().unwrap()["offline"]["state"],"verified_development");
  state.request("account",json!({})).unwrap();assert_eq!(state.status().unwrap()["offline"]["state"],"verified_development");
  state.request("deactivate",json!({})).unwrap();assert_eq!(state.status().unwrap()["offline"]["state"],"none");state.request("activate",json!({"order_id":order})).unwrap();
  state.request("refund",json!({"order_id":order})).unwrap();assert_eq!(state.status().unwrap()["offline"]["state"],"none");state.request("logout",json!({})).unwrap();assert_eq!(state.status().unwrap()["logged_in"],false);assert_eq!(state.status().unwrap()["basic_features_allowed"],true);
  drop(state);drop(process);fs::remove_dir_all(root).unwrap();
 }
}
