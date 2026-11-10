export type DurationRange={min:number|null;max:number|null};
export type MusicDimension={key:string;label:string;presets:string[]};
export type MusicFilterTemplates=Record<string,string[]>;
/**
 * 音乐库筛选维度的默认预置值。设置页可改写每个维度的预置值，改动存 preferences；
 * 这里只是默认模板与恢复默认的来源。
 */
export const MUSIC_DIMENSIONS:MusicDimension[]=[
 {key:'流派',label:'曲风',presets:['电影配乐','古典','电子','流行','摇滚','爵士','氛围']},
 {key:'情绪',label:'情绪',presets:['紧张','悬疑','温暖','悲伤','欢快','史诗']},
 {key:'用途',label:'用途 / 场景',presets:['预告片','对白铺底','转场','片头','片尾']},
 {key:'乐器',label:'乐器',presets:['钢琴','弦乐','吉他','打击乐','合成器']},
 {key:'人声',label:'人声',presets:['男声','女声','合唱','无人声']},
 {key:'BPM',label:'BPM',presets:[]},
 {key:'调性',label:'调性',presets:[]}
];
export const DURATION_PRESETS:{label:string;min:number|null;max:number|null}[]=[
 {label:'15 秒以内',min:null,max:15},
 {label:'15 秒 – 1 分',min:15,max:60},
 {label:'1 – 3 分',min:60,max:180},
 {label:'3 分以上',min:180,max:null}
];
export const defaultMusicFilterTemplates=():MusicFilterTemplates=>Object.fromEntries(MUSIC_DIMENSIONS.map(item=>[item.key,[...item.presets]]));
export const TEMPLATE_VALUE_LIMIT=40;
const clampValue=(value:string)=>value.trim().slice(0,30);
/** 设置页输入用「，」分隔；也接受逗号、顿号、换行。空值表示该维度不显示预置。 */
export function parseTemplateValues(text:string):string[]{
 const seen=new Set<string>();const values:string[]=[];
 for(const part of text.split(/[,，、\n\r]+/)){
  const value=clampValue(part);
  if(!value||seen.has(value))continue;
  seen.add(value);values.push(value);
  if(values.length>=TEMPLATE_VALUE_LIMIT)break;
 }
 return values;
}
export function safeMusicFilterTemplates(raw:unknown):MusicFilterTemplates{
 const defaults=defaultMusicFilterTemplates();
 if(typeof raw!=='object'||!raw)return defaults;
 const source=raw as Record<string,unknown>;
 const result:MusicFilterTemplates={};
 for(const dimension of MUSIC_DIMENSIONS){
  const candidate=source[dimension.key];
  if(!Array.isArray(candidate)){result[dimension.key]=defaults[dimension.key];continue;}
  result[dimension.key]=parseTemplateValues(candidate.filter(item=>typeof item==='string').join('\n'));
 }
 return result;
}
