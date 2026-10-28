export type CatalogAsset={id:string;path:string;name:string;format:string;size:number;modified:string;rate:number|null;channels:number|null;duration:number|null;status:string;error:string|null;favorite:boolean;tags:string[]};
export type CatalogPage={items:CatalogAsset[];total:number};
export type FolderNode={path:string;parent:string|null;name:string;count:number};
export function displayPath(path:string){return path.startsWith('\\\\?\\UNC\\')?'\\\\'+path.slice(8):path.startsWith('\\\\?\\')?path.slice(4):path;}
export type ImportJob={id:string;root:string;status:string;discovered:number;processed:number;added:number;updated:number;skipped:number;failed:number;error:string|null;library?:'sound'|'music'};
export const jobLabels:Record<string,string>={queued:'等待中',running:'扫描中',completed:'完成',completed_with_errors:'完成（有失败项）',cancelled:'已取消',interrupted:'重启中断',failed:'失败',cancelling:'正在取消'};
export const resumable=(job:ImportJob)=>['cancelled','interrupted','failed','completed_with_errors'].includes(job.status);
export function filterPage(items:CatalogAsset[],query:string,format:string){
 const words=query.trim().toLowerCase().split(/\s+/).filter(Boolean);
 return items.filter(a=>(format==='全部格式'||a.format.toUpperCase()===format)&&words.every(w=>`${a.name} ${a.path}`.toLowerCase().includes(w)));
}
export function formatBytes(size:number){return size<1024?`${size} B`:size<1048576?`${(size/1024).toFixed(1)} KB`:`${(size/1048576).toFixed(1)} MB`;}
