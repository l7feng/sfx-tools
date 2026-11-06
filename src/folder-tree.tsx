import React,{useMemo,useState} from 'react';
import type {FolderNode} from './catalog-client';
import {displayPath} from './catalog-client';
export function FolderTree({nodes,selected,onSelect,onContext}:{nodes:FolderNode[];selected:string|null;onSelect:(path:string)=>void;onContext?:(path:string,event:React.MouseEvent)=>void}){
 const [expanded,setExpanded]=useState(new Set<string>()),[collapsed,setCollapsed]=useState(new Set<string>());
 const {children,roots}=useMemo(()=>{
  const known=new Set(nodes.map(n=>n.path));const children=new Map<string,FolderNode[]>();const roots:FolderNode[]=[];
  for(const n of nodes){if(!n.parent||!known.has(n.parent))roots.push(n);else children.set(n.parent,[...(children.get(n.parent)??[]),n]);}
  return {children,roots};
 },[nodes]);
 function render(node:FolderNode,depth:number):React.ReactNode{
  const nested=children.get(node.path)??[];const open=!collapsed.has(node.path)&&(depth<2||expanded.has(node.path));
  return <li key={node.path}><div className={'folder-row '+(selected===node.path?'selected':'')} style={{paddingLeft:Math.min(depth,8)*12}} onContextMenu={event=>{if(onContext){event.preventDefault();event.stopPropagation();onContext(node.path,event);}}}>
   <button className="folder-expand" disabled={!nested.length} aria-label={`${open?'折叠':'展开'} ${displayPath(node.path)}`} aria-expanded={nested.length?open:undefined} onClick={()=>{setExpanded(old=>{const n=new Set(old);if(!open)n.add(node.path);return n;});setCollapsed(old=>{const n=new Set(old);open?n.add(node.path):n.delete(node.path);return n;});}}>{nested.length?(open?'⌄':'›'):'·'}</button>
   <button className="folder-select" title={displayPath(node.path)} aria-current={selected===node.path?'location':undefined} onClick={()=>onSelect(node.path)}><span>▱ {node.name}</span><small>{node.count}</small></button>
  </div>{open&&nested.length>0&&<ul>{nested.map(child=>render(child,depth+1))}</ul>}</li>;
 }
 return <nav className="folder-tree" aria-label="原路径文件树">{roots.length?<ul>{roots.map(n=>render(n,0))}</ul>:<p className="tree-empty">导入后按原始目录显示<br/>不会整理或移动文件</p>}</nav>;
}
