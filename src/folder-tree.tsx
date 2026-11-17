import React,{useMemo,useState} from 'react';
import type {FolderNode} from './catalog-client';
import {displayPath} from './catalog-client';

/** 目录图标：展开时画成开口文件夹，收起时用闭合轮廓。 */
export function FolderGlyph({open}:{open:boolean}){
 return <svg className="folder-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
  {open
   ?<><path d="M3 9V7a2 2 0 0 1 2-2h3.6a2 2 0 0 1 1.4.6L11.4 7H19a2 2 0 0 1 2 2v1"/><path d="M3 9h17.2a1.6 1.6 0 0 1 1.56 1.96l-1.5 7.2A2 2 0 0 1 18.3 19.8H5.7a2 2 0 0 1-1.96-1.64L3 9z"/></>
   :<path d="M3 7a2 2 0 0 1 2-2h3.6a2 2 0 0 1 1.4.6L11.4 7H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/>}
 </svg>;
}

/** 文件图标：叶子目录直接挂素材，用「卷角文件 + 波形」表示，与文件夹区分。 */
export function FileGlyph(){
 return <svg className="folder-icon file-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
  <path d="M6 3h7l5 5v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/><path d="M13 3v5h5"/><path d="M8 15.5v1M10.4 13.5v5M12.8 14.5v3M15.2 15.2v1"/>
 </svg>;
}

export function FolderTree({nodes,selected,onSelect,onContext}:{nodes:FolderNode[];selected:string|null;onSelect:(path:string)=>void;onContext?:(path:string,event:React.MouseEvent)=>void}){
 const [expanded,setExpanded]=useState(new Set<string>()),[collapsed,setCollapsed]=useState(new Set<string>());
 const {children,roots}=useMemo(()=>{
  const known=new Set(nodes.map(n=>n.path));const children=new Map<string,FolderNode[]>();const roots:FolderNode[]=[];
  for(const n of nodes){if(!n.parent||!known.has(n.parent))roots.push(n);else children.set(n.parent,[...(children.get(n.parent)??[]),n]);}
  return {children,roots};
 },[nodes]);
 function toggle(path:string,open:boolean){
  setExpanded(old=>{const next=new Set(old);open?next.delete(path):next.add(path);return next;});
  setCollapsed(old=>{const next=new Set(old);open?next.add(path):next.delete(path);return next;});
 }
 function render(node:FolderNode,depth:number):React.ReactNode{
  const nested=children.get(node.path)??[];const open=!collapsed.has(node.path)&&(depth<2||expanded.has(node.path));
  const hasChild=!!nested.length;
  return <li key={node.path}><button type="button" className={`folder-row${selected===node.path?' selected':''}${hasChild&&open?' is-open':''}`} style={{paddingLeft:Math.min(depth,8)*12}} aria-current={selected===node.path?'location':undefined} aria-expanded={hasChild?open:undefined} title={displayPath(node.path)} onContextMenu={event=>{if(onContext){event.preventDefault();event.stopPropagation();onContext(node.path,event);}}} onClick={()=>{if(hasChild)toggle(node.path,open);onSelect(node.path);}}>
   {hasChild?<FolderGlyph open={open}/>:<FileGlyph/>}
   <span className="folder-text">{node.name}</span>
   <small>{node.count}</small>
  </button>{open&&hasChild&&<ul>{nested.map(child=>render(child,depth+1))}</ul>}</li>;
 }
 return <nav className="folder-tree" aria-label="原路径文件树">{roots.length?<ul>{roots.map(n=>render(n,0))}</ul>:<p className="tree-empty">导入后按原始目录显示<br/>不会整理或移动文件</p>}</nav>;
}
