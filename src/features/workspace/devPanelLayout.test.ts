import { afterEach, expect, it, vi } from 'vitest';
import { observePanels } from './devPanelLayout';
import { PANEL_LAYOUT_KEY } from './panelLayout';

afterEach(() => {
  document.body.replaceChildren();
  localStorage.clear();
  vi.restoreAllMocks();
});
function setup() {
  document.body.innerHTML =
    '<aside class="sidebar"></aside><div class="sidebar-stack"></div><div data-panel-id="workspace-inspector"></div><div data-dev-target="panel:width"></div><div data-dev-target="panel:height"></div><section class="layer-tree" data-layer-count="4096" data-loading="false"><div data-dev-target="panel:tree"><div class="tree-row"><button class="tree-name" data-dev-target="layer:0:select"></button></div></div></section>';
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    return {
      x: 10,
      y: 20,
      width: 320,
      height: this.matches('[data-panel-id]') ? 216 : 608,
      top: 20,
      left: 10,
      bottom: 628,
      right: 330,
      toJSON: () => ({}),
    };
  });
  vi.spyOn(HTMLElement.prototype, 'getClientRects').mockReturnValue([
    { x: 10, y: 20, width: 320, height: 608 },
  ] as unknown as DOMRectList);
  const resources = document.createElement('div');
  resources.dataset.panelId = 'workspace-resources';
  document.body.append(resources);
}
it('缺少布局时不可用；读取只计算真实行数，不把图层总数当渲染节点数', () => {
  expect(observePanels()).toBeNull();
  setup();
  const p = observePanels()!;
  expect(p.treeCount).toBe(4096);
  expect(p.tree?.renderedRows).toBe(1);
  expect(p.tree?.lastLayerId).toBe(0);
  expect(p.savedSidebarWidth).toBeNull();
  expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBeNull();
});
it('隐藏列表没有可见滚动证据；损坏偏好明确标记，不覆盖存储', () => {
  setup();
  vi.mocked(HTMLElement.prototype.getClientRects).mockReturnValue([] as unknown as DOMRectList);
  localStorage.setItem(PANEL_LAYOUT_KEY, 'broken');
  const p = observePanels()!;
  expect(p.tree).toBeNull();
  expect(p.preferenceError).toBe(true);
  expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBe('broken');
});
it('拖动状态与保存偏好分开观测', () => {
  setup();
  document.querySelector<HTMLElement>(
    '[data-dev-target="panel:width"]',
  )!.dataset.resizeHandleState = 'drag';
  localStorage.setItem(
    PANEL_LAYOUT_KEY,
    JSON.stringify({ version: 1, sidebarWidth: 420, inspectorPercent: 48 }),
  );
  const p = observePanels()!;
  expect(p.dragging).toBe(true);
  expect(p.sidebarWidth).toBe(320);
  expect(p.savedSidebarWidth).toBe(420);
  expect(p.savedInspectorPercent).toBe(48);
});

it('高度和分隔条焦点来自实际元素，不以拖动标志推断键盘焦点', () => {
  setup();
  const handle = document.querySelector<HTMLElement>('[data-dev-target="panel:height"]')!;
  handle.tabIndex = 0;
  handle.focus();
  const p = observePanels()!;
  expect(p.focusedHandle).toBe('height');
  expect(p.dragging).toBe(false);
  expect(p.inspectorHeight).toBe(216);
  expect(p.resourcesHeight).toBe(216);
});
