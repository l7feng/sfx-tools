import React,{useMemo,useState} from 'react';
import type {FolderNode} from './catalog-client';
import {displayPath} from './catalog-client';

/** 统一三角符号：展开朝下、折叠朝右；叶子目录用同形但压暗，不再分文件夹/文件图标。 */
export function FolderGlyph({open}:{open:boolean}){
 return <svg className={`folder-icon tree-triangle${open?' is-open':''}`} viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true"><path d="M9 5.6 17.2 12 9 18.4z"/></svg>;
}

/** 无子目录的叶子：同一个三角，只是不旋转、颜色更淡。 */
export function FileGlyph(){
 return <svg className="folder-icon tree-triangle is-leaf" viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true"><path d="M9 5.6 17.2 12 9 18.4z"/></svg>;
}

import {treeClick} from './tree-click';

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
  const action=treeClick({hasChild,open,selected:selected===node.path});
  const onClick=()=>{
   if(action==='select'){onSelect(node.path);return;}
   if(action==='select-open'){onSelect(node.path);toggle(node.path,true);return;}
   toggle(node.path,open); // 已选中：再点标题＝收起/展开，选中不动
  };
  return <li key={node.path}><button type="button" className={`folder-row${selected===node.path?' selected':''}${hasChild&&open?' is-open':''}`} style={{paddingLeft:Math.min(depth,8)*12}} aria-current={selected===node.path?'location':undefined} aria-expanded={hasChild?open:undefined} title={displayPath(node.path)} onContextMenu={event=>{if(onContext){event.preventDefault();event.stopPropagation();onContext(node.path,event);}}} onClick={onClick}>
   {hasChild?<FolderGlyph open={open}/>:<FileGlyph/>}
   <span className="folder-text">{node.name}</span>
   <small>{node.count}</small>
  </button>{open&&hasChild&&<ul>{nested.map(child=>render(child,depth+1))}</ul>}</li>;
 }
 return <nav className="folder-tree" aria-label="原路径文件树">{roots.length?<ul>{roots.map(n=>render(n,0))}</ul>:<p className="tree-empty">导入后按原始目录显示<br/>不会整理或移动文件</p>}</nav>;
}
