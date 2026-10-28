export type Asset = {id:number; name:string; file:string; category:string; format:string; duration:number; rate:number; channels:number; tags:string[]; description:string};
export const assets:Asset[] = [
 {id:1,name:'雨落松林 · 远处雷声',file:'RAIN_PineForest_DistantThunder_01.wav',category:'自然环境',format:'WAV',duration:42.8,rate:96,channels:2,tags:['雨','森林','远雷'],description:'细密的雨落在松针与林间地面上，远处偶有低沉雷声。适合建立安静、有纵深的雨夜环境。'},
 {id:2,name:'林间清晨 · 鸟鸣与微风',file:'AMBI_Forest_MorningBirds_02.wav',category:'自然环境',format:'WAV',duration:68.2,rate:48,channels:2,tags:['森林','鸟鸣','清晨'],description:'清晨林间的稀疏鸟鸣与轻微树叶声。'},
 {id:3,name:'海岸 · 缓慢涌浪',file:'WATR_Coast_SoftWaves_03.flac',category:'自然环境',format:'FLAC',duration:31.5,rate:96,channels:2,tags:['海浪','海岸','舒缓'],description:'平缓的海浪在沙岸上展开，保留自然的呼吸感。'},
 {id:4,name:'雨夜街道 · 车辆驶过',file:'AMBI_City_WetStreet_04.wav',category:'城市生活',format:'WAV',duration:26.4,rate:48,channels:2,tags:['城市','雨','汽车'],description:'湿润街面上的轮胎摩擦与远处交通环境。'},
 {id:5,name:'老木门 · 缓慢推开',file:'DOOR_Wood_CreakOpen_05.aiff',category:'拟音动作',format:'AIFF',duration:4.6,rate:48,channels:1,tags:['木门','吱呀','室内'],description:'老旧木门缓慢打开，铰链发出连续的摩擦声。'},
 {id:6,name:'脚步 · 石板路上行走',file:'FOOT_Stone_Walk_06.wav',category:'拟音动作',format:'WAV',duration:12.3,rate:48,channels:1,tags:['脚步','石板','行走'],description:'硬底鞋在石板路上以自然节奏行走。'},
 {id:7,name:'机械开关 · 金属锁扣',file:'MECH_Metal_Latch_07.wav',category:'机械装置',format:'WAV',duration:2.1,rate:96,channels:1,tags:['金属','开关','机械'],description:'清脆的金属锁扣闭合，带短促机械回弹。'},
 {id:8,name:'城市地铁 · 进站',file:'TRAN_Subway_Arrival_08.flac',category:'城市生活',format:'FLAC',duration:38.7,rate:48,channels:2,tags:['地铁','交通','城市'],description:'列车进站的滚动声逐渐接近并停下。'},
 {id:9,name:'低频冲击 · 深沉尾音',file:'DSGN_Impact_DeepTail_09.wav',category:'设计音效',format:'WAV',duration:6.8,rate:96,channels:2,tags:['冲击','低频','转场'],description:'低频冲击接上平缓衰减的空间尾音。'},
 {id:10,name:'转场 · 空气掠过',file:'DSGN_Whoosh_Air_10.mp3',category:'设计音效',format:'MP3',duration:3.2,rate:44.1,channels:2,tags:['转场','空气','快速'],description:'由远及近的空气掠过声。'},
 {id:11,name:'溪流 · 石间流水',file:'WATR_Stream_Rocks_11.wav',category:'自然环境',format:'WAV',duration:54.1,rate:48,channels:2,tags:['流水','溪流','自然'],description:'溪水绕过石块形成细碎、持续的流动声。'},
 {id:12,name:'电机 · 稳定运转',file:'MECH_Motor_Steady_12.aiff',category:'机械装置',format:'AIFF',duration:18.4,rate:48,channels:1,tags:['电机','持续','机械'],description:'稳定运转的电机，附带轻微机壳共振。'},
];
export function filterAssets(items:Asset[],query:string,scope:string,format:string,favorites:Set<number>) {
 const words=query.trim().toLowerCase().split(/\s+/).filter(Boolean);
 return items.filter(a=>(scope==='全部素材'||(scope==='我的收藏'?favorites.has(a.id):a.category===scope))&&(format==='全部格式'||a.format===format)&&words.every(w=>`${a.name} ${a.file} ${a.tags.join(' ')}`.toLowerCase().includes(w)));
}
export function timeLabel(seconds:number){const ticks=Math.round(Math.max(0,seconds)*10);return `${Math.floor(ticks/600).toString().padStart(2,'0')}:${((ticks%600)/10).toFixed(1).padStart(4,'0')}`;}
