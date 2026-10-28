export type SearchScope='global'|'folder';
export function searchFolder(folder:string|null,query:string,scope:SearchScope){return query.trim()&&scope==='global'?null:folder;}
