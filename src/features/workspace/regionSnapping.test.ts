import { expect, it } from 'vitest';
import type { LayerSummary } from '../../shared/api/generated';
import { beginRegion, finishRegion } from './regionDraft';
import type { RegionAction } from './regionDraft';
import { buildSnapIndex, snapRegion } from './regionSnapping';

const none = { x: null, y: null };
const layer = (id: number, x = 300, y = 250, width = 200, height = 100): LayerSummary => ({
  id,
  parentId: null,
  name: `图层 ${id}`,
  nameTruncated: false,
  kind: 'bitmap',
  bounds: { x, y, width, height },
  visible: true,
  effectiveVisible: true,
  opacity: 255,
});
const create = () => beginRegion('create', { x: 100, y: 120 }, null);

it('索引只包含当前摘要的可见非组正面积边界，同坐标优先画布及较小图层 ID', () => {
  const index = buildSnapIndex(1000, 800, [
    layer(3),
    layer(1),
    { ...layer(4, 310), effectiveVisible: false },
    { ...layer(5, 320), kind: 'group' },
    layer(6, 330, 250, 0),
    layer(7, 340, 250, 20, 0),
    layer(8, 350, 800),
    layer(9, 1000),
    layer(10, NaN),
    layer(11, -40, 20, 60, 80),
    layer(12, 0, 0),
  ]);
  expect(index.layersReady).toBe(true);
  expect(index.x.map((edge) => edge.value)).toEqual([0, 20, 200, 300, 500, 1000]);
  expect(index.x.find((edge) => edge.value === 0)?.layerId).toBeNull();
  expect(index.x.find((edge) => edge.value === 300)?.layerId).toBe(1);
  expect(index.x.find((edge) => edge.value === 300)?.label).toContain('几何左边');
  expect(buildSnapIndex(1000, 800).layersReady).toBe(false);
  expect(buildSnapIndex(1000, 800, []).layersReady).toBe(true);
});

it.each([0.1, 0.43, 1, 1.25, 2, 8])(
  '缩放 %s 时用 6 CSS px 进入、10 CSS px 脱离，不使用设备像素比',
  (zoom) => {
    const index = buildSnapIndex(2000, 1600, [layer(1, 1000, 600)]);
    const initial = beginRegion('create', { x: 100, y: 100 }, null);
    const snapped = snapRegion(
      initial,
      { x: 1000 - 5.9 / zoom, y: 400 },
      2000,
      1600,
      zoom,
      index,
      none,
    );
    expect(snapped.draft.bounds.x + snapped.draft.bounds.width).toBe(1000);
    const held = snapRegion(
      snapped.draft,
      { x: 1000 - 9.9 / zoom, y: 400 },
      2000,
      1600,
      zoom,
      index,
      snapped.guides,
    );
    expect(held.guides.x?.target.value).toBe(1000);
    const released = snapRegion(
      held.draft,
      { x: 1000 - 10.1 / zoom, y: 400 },
      2000,
      1600,
      zoom,
      index,
      held.guides,
    );
    expect(released.guides.x).toBeNull();
    expect(released.draft.bounds.width).toBeCloseTo(900 - 10.1 / zoom);
    expect(
      snapRegion(initial, { x: 1000 - 6.1 / zoom, y: 400 }, 2000, 1600, zoom, index, none).guides.x,
    ).toBeNull();
  },
);

it('新建保持起点、吸附拖动终点，反向拖动和两轴独立生效', () => {
  const index = buildSnapIndex(1000, 800, [layer(1)]);
  const result = snapRegion(create(), { x: 297, y: 347 }, 1000, 800, 1, index, none);
  expect(result.draft.bounds).toEqual({ x: 100, y: 120, width: 200, height: 230 });
  const reversed = snapRegion(
    beginRegion('create', { x: 600, y: 500 }, null),
    { x: 503, y: 253 },
    1000,
    800,
    1,
    index,
    none,
  );
  expect(reversed.draft.bounds).toEqual({ x: 500, y: 250, width: 100, height: 250 });
});

it('同距优先画布，再按稳定 ID 决定；吸附锁保持至脱离，不被更近的边抢走', () => {
  const index = buildSnapIndex(1000, 800, [layer(1, 10), layer(2, 290), layer(3, 300)]);
  const tie = snapRegion(create(), { x: 5, y: 100 }, 1000, 800, 1, index, none);
  expect(tie.guides.x?.target.layerId).toBeNull();
  const layersTie = snapRegion(create(), { x: 295, y: 100 }, 1000, 800, 1, index, none);
  expect(layersTie.guides.x?.target.layerId).toBe(2);
  const enter = snapRegion(create(), { x: 297, y: 100 }, 1000, 800, 1, index, none);
  const held = snapRegion(enter.draft, { x: 291, y: 100 }, 1000, 800, 1, index, enter.guides);
  expect(held.guides.x?.target.value).toBe(300);
  const leave = snapRegion(held.draft, { x: 289, y: 100 }, 1000, 800, 1, index, held.guides);
  expect(leave.guides.x?.target.value).toBe(290);
});

it('开关或 Alt 禁用从原始指针恢复，重新启用不复用脱离阈值', () => {
  const index = buildSnapIndex(1000, 800, [layer(1)]);
  const enter = snapRegion(create(), { x: 297, y: 347 }, 1000, 800, 1, index, none);
  const free = snapRegion(
    enter.draft,
    { x: 292, y: 347 },
    1000,
    800,
    1,
    index,
    enter.guides,
    false,
  );
  expect(free.draft.bounds).toEqual({ x: 100, y: 120, width: 192, height: 227 });
  expect(free.guides).toEqual(none);
  expect(
    snapRegion(free.draft, { x: 292, y: 347 }, 1000, 800, 1, index, free.guides).guides.x,
  ).toBeNull();
});

it('移动比较两条边，保持尺寸；不选择会使另一边越出画布的目标', () => {
  const original = { x: 100, y: 200, width: 200, height: 100 };
  const draft = beginRegion('move', { x: 150, y: 250 }, original);
  const index = buildSnapIndex(1000, 800, [layer(1, 310, 200)]);
  const result = snapRegion(draft, { x: 157, y: 267 }, 1000, 800, 1, index, none);
  expect(result.draft.bounds).toEqual({ x: 110, y: 217, width: 200, height: 100 });
  const boundary = snapRegion(
    draft,
    { x: -200, y: 250 },
    1000,
    800,
    1,
    buildSnapIndex(1000, 800, [layer(1, 195)]),
    none,
  );
  expect(boundary.draft.bounds.x).toBe(0);
  expect(boundary.guides.x?.target.layerId).toBeNull();
  expect(
    finishRegion(
      snapRegion(draft, draft.start, 1000, 800, 1, buildSnapIndex(1000, 800), none).draft,
      50,
      1000,
      800,
    ),
  ).toBeNull();
});

it.each(['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw'] as const)(
  '%s 控制点只吸附被拖动边，不移动固定对边',
  (action) => {
    const original = { x: 200, y: 200, width: 200, height: 200 };
    const draft = beginRegion(action, { x: 200, y: 200 }, original);
    const index = buildSnapIndex(1000, 800, [layer(1, 210, 210, 200, 200)]);
    const result = snapRegion(draft, { x: 207, y: 207 }, 1000, 800, 1, index, none).draft.bounds;
    expect(result.x).toBe(action.includes('w') ? 210 : 200);
    expect(result.y).toBe(action.includes('n') ? 210 : 200);
    expect(result.x + result.width).toBe(action.includes('e') ? 410 : 400);
    expect(result.y + result.height).toBe(action.includes('s') ? 410 : 400);
  },
);

it('控制点越过对边仍固定原对边，不吸成零面积，也不跳过下一个有效目标', () => {
  const draft = beginRegion('w', { x: 200, y: 300 }, { x: 200, y: 200, width: 200, height: 200 });
  const result = snapRegion(
    draft,
    { x: 408, y: 300 },
    1000,
    800,
    1,
    buildSnapIndex(1000, 800, [layer(1, 400, 200, 10, 200)]),
    none,
  );
  expect(result.draft.bounds).toEqual({ x: 400, y: 200, width: 10, height: 200 });
  const nearFixed = snapRegion(
    draft,
    { x: 399, y: 300 },
    1000,
    800,
    1,
    buildSnapIndex(1000, 800, [layer(1, 400, 200, 3, 200)]),
    none,
  );
  expect(nearFixed.guides.x?.target.value).toBe(403);
});

it('4096 层重叠目标去重且稳定，输入顺序不影响结果', () => {
  const layers = Array.from({ length: 4096 }, (_, id) => layer(id));
  const first = buildSnapIndex(1000, 800, layers);
  const second = buildSnapIndex(1000, 800, [...layers].reverse());
  expect(first).toEqual(second);
  expect(first.x).toHaveLength(4);
  expect(first.x[1]?.layerId).toBe(0);
});

it.each(['create', 'move', 'e'] as const)(
  '%s 向画布外拖动可脱离靠边图层的锁定，再贴合画布',
  (action) => {
    const index = buildSnapIndex(1000, 800, [layer(1, 795, 250, 200, 100)]);
    const draft = beginRegion(
      action,
      { x: 900, y: 300 },
      { x: 700, y: 200, width: 200, height: 200 },
    );
    const enter = snapRegion(draft, { x: 994, y: 300 }, 1000, 800, 1, index, none);
    expect(enter.guides.x?.target.layerId).toBe(1);
    const outside = snapRegion(enter.draft, { x: 1100, y: 300 }, 1000, 800, 1, index, enter.guides);
    expect(outside.draft.bounds.x + outside.draft.bounds.width).toBe(1000);
    expect(outside.guides.x?.target.layerId).toBeNull();
  },
);

it('有界拖动组合始终保持有效范围，且不修改原始草稿或索引', () => {
  const index = buildSnapIndex(1000, 800, [layer(1), layer(2, 790, 595)]);
  const saved = structuredClone(index);
  const actions: RegionAction[] = ['create', 'move', 'n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw'];
  for (const action of actions) {
    const draft = beginRegion(
      action,
      { x: 200, y: 200 },
      { x: 200, y: 200, width: 200, height: 200 },
    );
    const initial = structuredClone(draft);
    for (const x of [-500, 0, 5, 297, 394, 500, 796, 1000, 2000]) {
      for (const y of [-500, 0, 247, 347, 405, 800, 2000]) {
        const b = snapRegion(draft, { x, y }, 1000, 800, 1, index, none).draft.bounds;
        expect(b.x).toBeGreaterThanOrEqual(0);
        expect(b.y).toBeGreaterThanOrEqual(0);
        expect(b.x + b.width).toBeLessThanOrEqual(1000);
        expect(b.y + b.height).toBeLessThanOrEqual(800);
        if (action === 'move') expect([b.width, b.height]).toEqual([200, 200]);
      }
    }
    expect(draft).toEqual(initial);
  }
  expect(index).toEqual(saved);
});
