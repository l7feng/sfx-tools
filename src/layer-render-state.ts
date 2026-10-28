export type RenderClip={assetId:string;offset:number;gain:number;duration:number};
export type RenderSnapshot={job_id:string|null;status:string;progress:number;rendered_seconds:number;duration:number;output_path:string|null;bytes:number;peak:number;clipped_samples:number;error:string|null;clips:RenderClip[];source_stale?:boolean};
export function renderRecipe(clips:readonly RenderClip[]):RenderClip[]{return clips.map(({assetId,offset,gain,duration})=>({assetId,offset,gain,duration}));}
export function renderRecipeKey(clips:readonly RenderClip[]){return JSON.stringify(renderRecipe(clips));}
export function renderIsActive(snapshot:Pick<RenderSnapshot,'status'>|null){return !!snapshot&&['queued','preparing','running','cancelling'].includes(snapshot.status);}
export function renderMatches(snapshot:RenderSnapshot|null,clips:readonly RenderClip[]){return !!snapshot&&!!snapshot.job_id&&snapshot.clips.length>0&&renderRecipeKey(snapshot.clips)===renderRecipeKey(clips);}
export function renderReady(snapshot:RenderSnapshot|null,clips:readonly RenderClip[]){return !!snapshot&&snapshot.status==='completed'&&!!snapshot.output_path&&snapshot.source_stale!==true&&renderMatches(snapshot,clips);}
export function renderProgress(value:number){return Number.isFinite(value)?Math.max(0,Math.min(1,value)):0;}
