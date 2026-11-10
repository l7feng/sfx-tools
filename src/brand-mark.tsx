import React from 'react';
/** 顶部品牌标：与 EXE 图标同一枚图形，深灰外框 + 黑色圆角方块 + 蓝色声波柱。 */
export function BrandMark({size=26}:{size?:number}){
 return <svg className="brand-mark" width={size} height={size} viewBox="0 0 32 32" role="img" aria-label="SFX TOOLS 标志">
  <defs><linearGradient id="sfx-brand-blue" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="#cfe6fb"/><stop offset="1" stopColor="#3d7dbf"/></linearGradient></defs>
  <rect x="0.5" y="0.5" width="31" height="31" rx="9" fill="#3d3d3d"/>
  <rect x="2.2" y="2.2" width="27.6" height="27.6" rx="7.5" fill="#1b1b1b"/>
  {[6,11,17,13,8].map((height,index)=><rect key={index} x={5+index*4.5} y={16-height/2} width="3.1" height={height} rx="1.55" fill="url(#sfx-brand-blue)"/>)}
 </svg>;
}
