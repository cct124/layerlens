import { expect, it } from 'vitest';
import type { LayerSummary } from '../../shared/api/generated';
import { inspectionLayers, hitLayers } from './canvasInspection';
import { revealLayer, treeRows } from './layerTree';

const layer = (id: number, extra: Partial<LayerSummary> = {}): LayerSummary => ({
  id,
  parentId: null,
  name: `layer${id}`,
  nameTruncated: false,
  kind: 'bitmap',
  bounds: { x: 10, y: 20, width: 40, height: 30 },
  visible: true,
  effectiveVisible: true,
  opacity: 255,
  ...extra,
});
it('完整摘要按堆叠命中叶子，排除透明、隐藏祖先、零面积及画布外位置', () => {
  const layers = [
    layer(1, { kind: 'group', visible: false }),
    layer(2, { parentId: 1 }),
    layer(3),
    layer(4),
    layer(5, { opacity: 0 }),
    layer(6, { bounds: { x: 10, y: 20, width: 0, height: 30 } }),
  ];
  expect(inspectionLayers(null)).toBeNull();
  const index = inspectionLayers(layers)!;
  expect(hitLayers(index, { x: 20, y: 30 }, 100, 100)).toEqual([3, 4]);
  expect(hitLayers(index, { x: 50, y: 30 }, 100, 100)).toEqual([]);
  expect(hitLayers(index, { x: -1, y: 30 }, 100, 100)).toEqual([]);
});
it('树过滤和折叠不改变候选；确认后可显露被过滤和折叠的选中项', () => {
  const layers = [layer(1, { kind: 'group' }), layer(2, { parentId: 1 }), layer(3)];
  const view = { collapsed: [1], search: 'absent', scroll: 300 };
  expect(treeRows(layers, view)).toEqual([]);
  expect(hitLayers(inspectionLayers(layers)!, { x: 20, y: 30 }, 100, 100)).toEqual([2, 3]);
  const revealed = revealLayer(layers, view, 2);
  expect(revealed.search).toBe('');
  expect(treeRows(layers, revealed).map((row) => row.layer.id)).toContain(2);
});
