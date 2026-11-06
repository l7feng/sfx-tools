/** 常用路径的颜色标记：值存库，色值只管显示，换主题不用迁移数据。 */
export const PIN_COLORS=[
 {value:'red',label:'红'},
 {value:'orange',label:'橙'},
 {value:'yellow',label:'黄'},
 {value:'green',label:'绿'},
 {value:'blue',label:'蓝'},
 {value:'purple',label:'紫'},
]as const;
export type PinColor=typeof PIN_COLORS[number]['value'];
export type Pin={path:string;color:string|null};
export const pinColorLabel=(value:string|null|undefined)=>PIN_COLORS.find(item=>item.value===value)?.label??'无';
export const isPinColor=(value:unknown):value is PinColor=>PIN_COLORS.some(item=>item.value===value);
