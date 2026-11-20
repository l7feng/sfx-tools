export type ToolArea='library'|'production'|'info'|'extension'|'account';
/** local=本程序内置｜link=网页外链｜app=外部程序（本机 exe，只负责启动）。 */
export type ToolKind='local'|'link'|'app';
/** ready=可用｜need=需先选素材｜dev=开发中｜link=外链，统一驱动卡片状态标签配色。 */
export type ToolStatus='ready'|'need'|'dev'|'link';
export type ToolEntry={
 id:string;label:string;description:string;keywords:string;
 area:ToolArea;kind:ToolKind;status:ToolStatus;
 /** 外链工具：完整 URL 与打开方式（系统浏览器 / 内嵌 webview）。 */
 href?:string;openIn?:'browser'|'webview';
 /** 外部程序：可执行文件名，供自动定位（有界遍历按名字找）。 */
 exe?:string;
};

/**
 * 工具注册表：左侧导航、右侧卡片网格、搜索都读这一份数据。
 * 新增外链工具只加一条配置，不改界面代码。
 */
export const toolPages = [
 {id:'ucs',label:'UCS 分类建议',description:'按描述或所选素材的名称、标签推断 CatID 与子类，逐条确认后写入标签，不自动改名。',keywords:'分类 标签 命名 整理 UCS',area:'library',kind:'local',status:'ready'},
 {id:'duplicates',label:'重复素材',description:'字节相同或波形与时长一致的重复项；只合并目录记录，源文件不移动、不删除。',keywords:'相同 SHA256 波形 时长 合并 清理 整理',area:'library',kind:'local',status:'ready'},
 {id:'layer',label:'叠层编排',description:'多素材叠加、散布与混音导出，单次最多 12 层，时长未知的片段自动跳过。',keywords:'叠层 散布 时间轴 渲染 制作',area:'library',kind:'local',status:'need'},
 {id:'maintenance',label:'库维护',description:'检查已登记文件的失联、变化与不可访问状态，保留标注，不自动删除记录。',keywords:'库 失联 路径 检测 导入 任务',area:'library',kind:'local',status:'ready'},
 {id:'convert',label:'批量导出与转换',description:'原件副本导出、SoXR 重采样、保调变速；默认不覆盖同名文件。',keywords:'批量 导出 高质量 采样率 速度 制作',area:'production',kind:'local',status:'need'},
 {id:'plugins',label:'插件与制作连接',description:'扫描 VST3 候选；AAF / OMF 与宿主连接仍为只读骨架。',keywords:'VST3 DLL AAF OMF 插件 扫描 宿主',area:'production',kind:'local',status:'dev'},
 {id:'pt',label:'Pro Tools 导入预览',description:'检测连接与落点安排，不写工程、不批量渲染。',keywords:'PT Pro Tools 第三期 制作 导入',area:'production',kind:'local',status:'dev'},
 {id:'info',label:'音频信息',description:'查看所选素材的格式、采样率、声道、时长与文件大小。',keywords:'音频 规格 文件 时长 信息',area:'info',kind:'local',status:'need'},
 {id:'ucs-namer',label:'UCS Namer',description:'批量重命名与 CatID 规范化，命名清单确认后再落盘；独立程序，本程序只负责启动。',keywords:'UCS 批量 命名 CatID 外部程序',area:'extension',kind:'app',status:'ready',exe:'UCS Namer.exe'},
 {id:'mus-prompt',label:'MUS Prompt',description:'配乐提示词与风格模板库，按项目复用提示词；独立程序，本程序只负责启动。',keywords:'MUS 提示词 prompt 配乐 外部程序',area:'extension',kind:'app',status:'ready',exe:'MUS Prompt.exe'},
 // 外部程序：本程序只负责把它送到你面前，扫描/比对/隔离全在它自己的界面里做。
 {id:'dedup',label:'Audio Dedup',description:'重复音频检测与隔离，独立程序；本程序只负责启动，处置在它自己的界面里逐次确认。',keywords:'去重 重复 Audio Dedup 隔离 清理 外部程序',area:'extension',kind:'app',status:'ready',exe:'audio-dedup-gui.exe'},
 {id:'account',label:'账号与授权',description:'GitHub / Google 登录、手机绑定、设备与离线凭证。',keywords:'买断 支付 退款 交易 设备 激活 登录',area:'account',kind:'local',status:'ready'},
] as const satisfies readonly ToolEntry[];
export type ToolPage = typeof toolPages[number]['id'];
/** 供运行时读取可选字段数组化后的条目（as const 会让可选字段形成联合类型）。 */
export const toolEntries = toolPages as readonly ToolEntry[];

export const toolGroups:{area:ToolArea;label:string}[]=[
 {area:'library',label:'音效库'},
 {area:'production',label:'制作'},
 {area:'info',label:'信息'},
 {area:'extension',label:'扩展 · 链接'},
 {area:'account',label:'账号'},
];

export function findTools(query:string){
 const words=query.trim().toLowerCase().split(/\s+/).filter(Boolean);
 return toolEntries.filter(tool=>words.every(word=>`${tool.label} ${tool.description} ${tool.keywords}`.toLowerCase().includes(word)));
}
export function toolsOf(area:ToolArea){return toolEntries.filter(tool=>tool.area===area);}
export function findTool(id:ToolPage){return toolEntries.find(tool=>tool.id===id)??null;}
/** 兼容旧调用：素材库类工具在列表内直接生效，其余走制作流程。 */
export function toolArea(id:ToolPage){return id==='convert'||id==='plugins'||id==='pt'?'production':'library';}
