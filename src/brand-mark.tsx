import React from 'react';
import brandUrl from './assets/brand.png';
/** 顶部品牌字标：两行反白 SFX TOOLS。 */
export function BrandMark({size=26}:{size?:number}){
 return <img className="brand-mark" src={brandUrl} alt="SFX Tools" style={{height:size,width:'auto'}} />;
}
