import { expect, it } from 'vitest';
import { beginRegion, documentPoint, finishRegion, screenPoint, updateRegion } from './regionDraft';
import type { RegionHandle } from './regionDraft';

it('缩放和平移下 CSS 像素与文档像素往返一致，不混入设备像素比', () => {
  for (const zoom of [0.01, 0.43, 1, 1.25, 2, 8]) {
    const canvas = {
      width: 1920,
      height: 1080,
      viewportWidth: 917,
      viewportHeight: 637,
      view: { mode: 'manual' as const, zoom, x: 37.5, y: -28.25 },
    };
    const point = { x: 103.25, y: 824.125 };
    const screen = screenPoint(point, canvas);
    const back = documentPoint(screen, canvas);
    expect(back.x).toBeCloseTo(point.x, 9);
    expect(back.y).toBeCloseTo(point.y, 9);
    expect(documentPoint({ x: screen.x + zoom, y: screen.y }, canvas).x).toBeCloseTo(
      point.x + 1,
      9,
    );
  }
});

it('反向新建限制在文档内，点击、零面积和返回原范围不提交', () => {
  const initial = beginRegion('create', { x: 70.5, y: 80.25 }, null);
  const updated = updateRegion(initial, { x: -200, y: 25.5 }, 100, 100);
  expect(updated.bounds).toEqual({ x: 0, y: 25.5, width: 70.5, height: 54.75 });
  expect(finishRegion(updated, 80, 100, 100)).toEqual(updated.bounds);
  expect(finishRegion(updated, 2, 100, 100)).toBeNull();
  expect(finishRegion(initial, 20, 100, 100)).toBeNull();
  const old = { x: 10, y: 20, width: 30, height: 40 };
  expect(finishRegion(beginRegion('move', { x: 20, y: 30 }, old), 20, 100, 100)).toBeNull();
  expect(finishRegion(updateRegion(initial, { x: NaN, y: 5 }, 100, 100), 20, 100, 100)).toBeNull();
});

it('移动到画布外保持尺寸，返回时始终相对初始范围计算', () => {
  const initial = beginRegion('move', { x: 30, y: 30 }, { x: 10, y: 20, width: 50, height: 40 });
  const outside = updateRegion(initial, { x: 300, y: -300 }, 100, 100);
  expect(outside.bounds).toEqual({ x: 50, y: 0, width: 50, height: 40 });
  expect(updateRegion(outside, { x: 35, y: 35 }, 100, 100).bounds).toEqual({
    x: 15,
    y: 25,
    width: 50,
    height: 40,
  });
});

it('八个控制点仅改变拖动边，支持越过对边且点击偏移不使边框跳动', () => {
  const original = { x: 20, y: 30, width: 40, height: 50 };
  const handles: RegionHandle[] = ['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw'];
  for (const action of handles) {
    const draft = beginRegion(action, { x: 55, y: 65 }, original);
    expect(updateRegion(draft, draft.start, 200, 200).bounds).toEqual(original);
    const bounds = updateRegion(draft, { x: 65, y: 70 }, 200, 200).bounds;
    expect(bounds.x).toBe(action.includes('w') ? 30 : 20);
    expect(bounds.y).toBe(action.includes('n') ? 35 : 30);
    expect(bounds.x + bounds.width).toBe(action.includes('e') ? 70 : 60);
    expect(bounds.y + bounds.height).toBe(action.includes('s') ? 85 : 80);
  }
  const crossed = updateRegion(
    beginRegion('nw', { x: 20, y: 30 }, original),
    { x: 90, y: 110 },
    100,
    100,
  );
  expect(crossed.bounds).toEqual({ x: 60, y: 80, width: 30, height: 20 });
});
