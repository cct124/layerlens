import { expect, it } from 'vitest';
import { fitZoom, locateBounds, zoomAround } from './canvasView';
it('适应窗口留边且不放大小图', () => {
  expect(fitZoom(1000, 2000, 864, 1064)).toBe(0.5);
  expect(fitZoom(10, 10, 500, 500)).toBe(1);
});
it('定位保留负文档坐标且中心映射到画布中心', () => {
  const bounds = { x: -20, y: 1300, width: 200, height: 100 };
  const view = locateBounds(1000, 2000, bounds, 864, 664);
  expect(view.zoom).toBe(4);
  expect(view.x + (bounds.x + bounds.width / 2 - 500) * view.zoom).toBe(0);
  expect(view.y + (bounds.y + bounds.height / 2 - 1000) * view.zoom).toBe(0);
});
it('缩放保持指针下原稿坐标不变并限制倍率', () => {
  const view = { mode: 'manual' as const, zoom: 0.5, x: 20, y: -30 };
  const next = zoomAround(view, 1, 120, 70);
  expect((120 - next.x) / next.zoom).toBe((120 - view.x) / view.zoom);
  expect((70 - next.y) / next.zoom).toBe((70 - view.y) / view.zoom);
  expect(zoomAround(view, 99).zoom).toBe(8);
});
