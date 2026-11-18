/** 收藏 / 标签两个「页面」的展示件。
 *  0.1.34 起不再用右上角弹出小面板：收藏和标签都变成和历史一样的整页视图，
 *  - 收藏页：顶部一排分组标签（全部 / 未分组 / 各分组），下面是列表；
 *  - 标签页：把库里的标签铺成网格，点一个就筛到该标签，再点「全部标签」回到索引。
 *  两件都只吃 props，不碰数据请求，换分页/换库时零成本。 */

export type TagCount = {name: string; count: number};

/** 收藏页顶部的分组条。groups 来自库概览；未分组与「全部」是固定项。 */
export function FavoriteChips({groups, active, ungrouped, onPick}: {
  groups: TagCount[];
  active: string | null;
  ungrouped: boolean;
  onPick: (group: string | null, ungrouped: boolean) => void;
}) {
  const total = groups.reduce((sum, item) => sum + item.count, 0);
  return <div className="saved-chips" role="group" aria-label="收藏分组">
    <button type="button" className={`saved-chip${!active && !ungrouped ? ' is-active' : ''}`} aria-pressed={!active && !ungrouped} onClick={() => onPick(null, false)}>全部收藏<b>{total}</b></button>
    <button type="button" className={`saved-chip${ungrouped ? ' is-active' : ''}`} aria-pressed={ungrouped} onClick={() => onPick(null, true)}>未分组</button>
    {groups.map(group => <button key={group.name} type="button" className={`saved-chip${active === group.name ? ' is-active' : ''}`} aria-pressed={active === group.name} onClick={() => onPick(group.name, false)}>{group.name}<b>{group.count}</b></button>)}
  </div>;
}

/** 标签索引页：标签按数量从多到少铺排，点一个就筛到该标签。 */
export function TagsIndex({tags, active, busy, onPick}: {
  tags: TagCount[];
  active: string | null;
  busy: boolean;
  onPick: (tag: string | null) => void;
}) {
  if (!tags.length) {
    return <section className="tag-index is-empty" aria-label="标签索引">
      <p className="tag-index-empty">还没有标签。在右侧详情栏给素材加标签后，这里会按使用次数列出全部标签。</p>
    </section>;
  }
  return <section className="tag-index" aria-label="标签索引">
    <header className="tag-index-head">
      <span>{tags.length} 个标签 · 共 {tags.reduce((sum, item) => sum + item.count, 0)} 条引用</span>
      {active && <button type="button" className="tag-index-clear" disabled={busy} onClick={() => onPick(null)}>显示全部素材</button>}
    </header>
    <div className="tag-grid" role="list">
      {tags.map(tag => <button key={tag.name} type="button" role="listitem" disabled={busy}
        className={`tag-card${active === tag.name ? ' is-active' : ''}`}
        aria-pressed={active === tag.name}
        title={`按「${tag.name}」筛选`}
        onClick={() => onPick(active === tag.name ? null : tag.name)}>
        <span className="tag-card-name">{tag.name}</span>
        <b className="tag-card-count">{tag.count}</b>
      </button>)}
    </div>
  </section>;
}
