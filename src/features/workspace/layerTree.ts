import type { LayerSummary } from '../../shared/api/generated';

/** 展开、搜索和滚动只属于视图；选中图层来自核心快照。 */
export interface LayerTreeView {
  collapsed: number[];
  search: string;
  scroll: number;
}
export const initialTree = (): LayerTreeView => ({ collapsed: [], search: '', scroll: 0 });

export function treeRows(layers: LayerSummary[], view: LayerTreeView) {
  const byId = new Map(layers.map((layer) => [layer.id, layer]));
  const query = view.search.trim().toLocaleLowerCase();
  const matches = new Set<number>();
  if (query)
    for (const layer of layers)
      if (layer.name.toLocaleLowerCase().includes(query)) {
        let parent: LayerSummary | undefined = layer;
        while (parent && !matches.has(parent.id)) {
          matches.add(parent.id);
          parent = parent.parentId === null ? undefined : byId.get(parent.parentId);
        }
      }
  const collapsed = new Set(view.collapsed),
    hidden = new Set<number>(),
    depths = new Map<number, number>();
  return layers.flatMap((layer) => {
    const depth = layer.parentId === null ? 0 : (depths.get(layer.parentId) ?? 0) + 1;
    depths.set(layer.id, depth);
    if (
      !query &&
      layer.parentId !== null &&
      (hidden.has(layer.parentId) || collapsed.has(layer.parentId))
    )
      hidden.add(layer.id);
    return (query ? matches.has(layer.id) : !hidden.has(layer.id)) ? [{ layer, depth }] : [];
  });
}

export const layerLabels = {
  group: '组',
  bitmap: '位图',
  text: '文字',
  shape: '形状',
  smartObject: '智能对象',
  adjustment: '调整',
  unknown: '未知',
};
