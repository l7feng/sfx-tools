export const toolPages = [
  {id:'ucs',label:'UCS 分类建议',description:'按描述或所选素材名称、标签提出分类建议。',keywords:'分类 标签 命名 整理'},
  {id:'duplicates',label:'重复素材',description:'检查当前库：字节相同或波形与时长一致的重复素材，可合并目录记录。',keywords:'相同 SHA256 波形 时长 合并 清理 整理'},
  {id:'convert',label:'批量导出与转换',description:'原件副本导出、SoXR 重采样、保调变速。',keywords:'批量 导出 高质量 采样率 速度 制作'},
  {id:'plugins',label:'插件与制作连接',description:'扫描插件候选；查看 AAF/OMF 与宿主的开发状态。',keywords:'VST3 DLL AAF OMF 插件 扫描 宿主'},
  {id:'layer',label:'叠层编排',description:'多素材编排、散布、混音与导出。',keywords:'叠层 散布 时间轴 渲染 制作'},
  {id:'pt',label:'Pro Tools 导入预览',description:'检测连接与导入安排，不执行真实工程写入。',keywords:'PT Pro Tools 第三期 制作 导入'},
  {id:'account',label:'账号与授权',description:'本地开发账号、双设备、离线凭证与模拟订单。',keywords:'买断 支付 退款 交易 设备 激活'},
  {id:'maintenance',label:'素材库维护',description:'检查失联素材与导入任务。',keywords:'库 失联 路径 检测 导入 任务'},
  {id:'info',label:'音频信息',description:'查看所选素材的格式、采样率、声道与时长。',keywords:'音频 规格 文件 时长 信息'},
] as const;
export type ToolPage = typeof toolPages[number]['id'];
export function findTools(query:string){
  const words=query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return toolPages.filter(tool=>words.every(word=>`${tool.label} ${tool.description} ${tool.keywords}`.toLowerCase().includes(word)));
}

export function toolArea(id:ToolPage){return (['convert','plugins','pt'] as string[]).includes(id)?'production':'library';}
