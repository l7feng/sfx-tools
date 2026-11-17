/** 目录树的点击语义。抽成独立模块的原因：
 *  1) 这是纯规则、无 React 依赖，能被 node 的测试直接跑；
 *  2) 规则散在 onClick 里时，改一次手势要读整段 JSX 才知道会怎样。
 *
 * 0.1.33 定的规则是「单击选中、再点标题收起」：
 *  - 叶子目录（挂着素材、没有子层）：点就只是选中；
 *  - 点别的行：选中它，并把它展开（一次点击就能看到下一层）；
 *  - 点已选中的行：只收起/展开它自己，选中保持不变——避免连续点子目录时每点都抖一下。
 */
export type TreeClickAction = 'select' | 'select-open' | 'toggle';

export function treeClick(node: {hasChild: boolean; open: boolean; selected: boolean}): TreeClickAction {
  if (!node.hasChild) return 'select';
  if (node.selected) return 'toggle';
  return node.open ? 'select' : 'select-open';
}
