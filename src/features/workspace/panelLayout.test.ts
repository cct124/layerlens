import { describe, expect, it } from 'vitest';
import {
  defaultPanelPreferences,
  inspectorLayout,
  parsePanelPreferences,
  sidebarLayout,
} from './panelLayout';

describe('面板布局边界', () => {
  it('只接受带版本的有限尺寸偏好，空记录使用默认值', () => {
    expect(parsePanelPreferences(null)).toEqual(defaultPanelPreferences());
    expect(parsePanelPreferences('{"version":1,"sidebarWidth":480,"inspectorPercent":42}')).toEqual(
      { version: 1, sidebarWidth: 480, inspectorPercent: 42 },
    );
    for (const value of [
      '{',
      'null',
      '[]',
      '{}',
      '{"version":2,"sidebarWidth":320,"inspectorPercent":36}',
      '{"version":1,"sidebarWidth":99999,"inspectorPercent":36}',
      '{"version":1,"sidebarWidth":"320","inspectorPercent":36}',
      '{"version":1,"sidebarWidth":320,"inspectorPercent":0}',
      '{"version":1,"sidebarWidth":320,"inspectorPercent":100}',
    ]) {
      expect(() => parsePanelPreferences(value)).toThrow();
    }
  });
  it('侧栏按 CSS 像素限制在 260–560，保留 400 像素画布并扣除分隔条', () => {
    const normal = sidebarLayout(1200, 320);
    expect((normal.percent / 100) * normal.available).toBeCloseTo(320);
    expect((normal.minPercent / 100) * normal.available).toBeCloseTo(260);
    expect((normal.maxPercent / 100) * normal.available).toBeCloseTo(560);
    const small = sidebarLayout(800, 560);
    expect((small.percent / 100) * small.available).toBeCloseTo(392);
    expect(small.available * (1 - small.maxPercent / 100)).toBeCloseTo(400);
  });
  it('上下保留 140／180 像素，狭小预览尺寸仍有可满足的约束', () => {
    expect(inspectorLayout(608, 1).percent).toBeCloseTo((140 / 600) * 100);
    expect(inspectorLayout(608, 99).percent).toBeCloseTo((420 / 600) * 100);
    for (const size of [0, 100, 320, 900]) {
      for (const layout of [sidebarLayout(size, 560), inspectorLayout(size, 99)]) {
        expect(Number.isFinite(layout.percent)).toBe(true);
        expect(layout.minPercent).toBeLessThanOrEqual(layout.maxPercent);
        expect(layout.percent).toBeGreaterThanOrEqual(layout.minPercent);
        expect(layout.percent).toBeLessThanOrEqual(layout.maxPercent);
      }
    }
  });
});
