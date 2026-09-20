import { flushPromises, mount } from '@vue/test-utils';
import type { VueWrapper } from '@vue/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import WorkspacePanels from './WorkspacePanels.vue';
import { PANEL_LAYOUT_KEY } from './panelLayout';

let wrapper: VueWrapper<InstanceType<typeof WorkspacePanels>> | undefined;
let width = 1200;
let height = 700;
const observers = new Set<TestObserver>();
class TestObserver implements ResizeObserver {
  targets = new Set<Element>();
  constructor(readonly callback: ResizeObserverCallback) {
    observers.add(this);
  }
  observe(target: Element) {
    this.targets.add(target);
  }
  unobserve(target: Element) {
    this.targets.delete(target);
  }
  disconnect() {
    this.targets.clear();
    observers.delete(this);
  }
}
function percent(id: string) {
  return Number(wrapper?.get<HTMLElement>(`#${id}`).element.style.flexGrow ?? 0);
}
function sidebarWidth() {
  return (percent('workspace-sidebar') / 100) * (width - 8);
}
function expectSidebar(expected: number) {
  // Reka 的 CSS flex-grow 使用三位有效数字，当前测试视口存在不足 1 CSS px 的舍入。
  expect(Math.abs(sidebarWidth() - expected)).toBeLessThan(1);
}
function topHeight() {
  return (percent('workspace-inspector') / 100) * (height - 28 - 8);
}
function rectangle(element: HTMLElement) {
  if (
    element.classList.contains('workspace-panels') ||
    (element.dataset.panelGroupId === 'workspace-width' && element.hasAttribute('data-panel-group'))
  )
    return new DOMRect(0, 0, width, height);
  const side = wrapper ? sidebarWidth() : 320;
  if (
    element.classList.contains('sidebar-stack') ||
    (element.hasAttribute('data-panel-group') &&
      element.dataset.panelGroupId === 'workspace-height')
  )
    return new DOMRect(width - side, 0, side, height - 28);
  if (element.id === 'sidebar-width-handle') return new DOMRect(width - side - 8, 0, 8, height);
  if (element.id === 'sidebar-height-handle')
    return new DOMRect(width - side, wrapper ? topHeight() : 240, side, 8);
  return new DOMRect(0, 0, width, height);
}
async function measure() {
  for (const observer of observers) {
    observer.callback(
      [...observer.targets].map((target) => ({
        target,
        contentRect: target.getBoundingClientRect(),
        borderBoxSize: [],
        contentBoxSize: [],
        devicePixelContentBoxSize: [],
      })),
      observer,
    );
  }
  await flushPromises();
}
async function setup() {
  wrapper = mount(WorkspacePanels, {
    attachTo: document.body,
    slots: {
      canvas: '<div>画布</div>',
      inspector: '<div>属性</div>',
      resources: '<input aria-label="任务名称" />',
      footer: '<div>只读</div>',
    },
  });
  await flushPromises();
  await measure();
  return wrapper;
}
async function down(id: string) {
  const element = wrapper!.get(`#${id}`).element;
  const rect = element.getBoundingClientRect();
  const point = { clientX: rect.x + rect.width / 2, clientY: rect.y + rect.height / 2 };
  element.dispatchEvent(
    new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      buttons: 1,
      button: 0,
      ...point,
    }),
  );
  await flushPromises();
  return point;
}
async function move(point: { clientX: number; clientY: number }) {
  document.body.dispatchEvent(
    new MouseEvent('mousemove', { bubbles: true, cancelable: true, buttons: 1, ...point }),
  );
  await flushPromises();
}
async function up(point: { clientX: number; clientY: number }) {
  window.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, cancelable: true, ...point }));
  await flushPromises();
}
beforeEach(() => {
  width = 1200;
  height = 700;
  localStorage.clear();
  vi.stubGlobal('ResizeObserver', TestObserver);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    return rectangle(this);
  });
});
afterEach(() => {
  wrapper?.unmount();
  wrapper = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('真实 Reka 分割面板集成', () => {
  it('默认布局、分隔条语义与焦点；初始化不保存偏好', async () => {
    const bar = await setup();
    expectSidebar(320);
    expect(percent('workspace-inspector')).toBeCloseTo(36, 1);
    const handles = bar.findAll('[role="separator"]');
    expect(handles).toHaveLength(2);
    expect(handles[0]!.attributes('aria-orientation')).toBe('vertical');
    expect(handles[1]!.attributes('aria-orientation')).toBe('horizontal');
    expect(handles[0]!.attributes('tabindex')).toBe('0');
    expect(handles[0]!.attributes('aria-controls')).toBe('workspace-canvas');
    expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBeNull();
    await bar.get('#sidebar-width-handle').trigger('keydown', { key: 'Escape' });
    await flushPromises();
    expectSidebar(320);
  });
  it('左右和上下拖动实时调整，松开才保存；嵌套面板内容不重建', async () => {
    const bar = await setup();
    const input = bar.get<HTMLInputElement>('input').element;
    await bar.get('input').setValue('我的任务');
    const point = await down('sidebar-width-handle');
    await move({ ...point, clientX: point.clientX - 100 });
    expect(sidebarWidth()).toBeGreaterThan(410);
    expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBeNull();
    await up(point);
    expect(JSON.parse(localStorage.getItem(PANEL_LAYOUT_KEY)!).sidebarWidth).toBeGreaterThan(410);
    const top = topHeight();
    const vertical = await down('sidebar-height-handle');
    await move({ ...vertical, clientY: vertical.clientY + 70 });
    await up(vertical);
    expect(topHeight()).toBeGreaterThan(top + 60);
    expect(bar.get('input').element).toBe(input);
    expect(input.value).toBe('我的任务');
  });
  it('Esc 取消并真正结束鼠标手势，不保存、不冒泡；后续拖动仍正常', async () => {
    await setup();
    const outerKey = vi.fn();
    window.addEventListener('keydown', outerKey);
    const point = await down('sidebar-width-handle');
    await move({ ...point, clientX: point.clientX - 100 });
    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    );
    await flushPromises();
    expectSidebar(320);
    expect(outerKey).not.toHaveBeenCalled();
    await move({ ...point, clientX: point.clientX - 150 });
    expectSidebar(320);
    expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBeNull();
    expect(wrapper!.get<HTMLElement>('#workspace-canvas').element.style.pointerEvents).not.toBe(
      'none',
    );
    await up(point);
    const next = await down('sidebar-width-handle');
    await move({ ...next, clientX: next.clientX - 60 });
    await up(next);
    expect(sidebarWidth()).toBeGreaterThan(370);
    window.removeEventListener('keydown', outerKey);
  });
  it('失焦取消上下调整，卸载期间结束手势且释放 ResizeObserver', async () => {
    const bar = await setup();
    const point = await down('sidebar-height-handle');
    await move({ ...point, clientY: point.clientY + 70 });
    window.dispatchEvent(new Event('blur'));
    await flushPromises();
    expect(percent('workspace-inspector')).toBeCloseTo(36, 1);
    await down('sidebar-width-handle');
    bar.unmount();
    wrapper = undefined;
    await flushPromises();
    expect(observers.size).toBe(0);
    expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBeNull();
    await setup();
    const next = await down('sidebar-width-handle');
    await move({ ...next, clientX: next.clientX - 60 });
    await up(next);
    expect(sidebarWidth()).toBeGreaterThan(370);
  });
  it('拖动期间拒绝键盘混入，外部尺寸变化取消草稿；非左键不会调整', async () => {
    const bar = await setup();
    const point = await down('sidebar-width-handle');
    await move({ ...point, clientX: point.clientX - 80 });
    const beforeKey = sidebarWidth();
    await bar.get('#sidebar-width-handle').trigger('keydown', { key: 'ArrowLeft' });
    await flushPromises();
    expect(sidebarWidth()).toBe(beforeKey);
    expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBeNull();
    width = 900;
    await measure();
    expectSidebar(320);
    expect(localStorage.getItem(PANEL_LAYOUT_KEY)).toBeNull();
    await up(point);
    const handle = bar.get('#sidebar-width-handle').element;
    const rect = handle.getBoundingClientRect();
    const middle = { clientX: rect.x + 4, clientY: 20 };
    handle.dispatchEvent(
      new MouseEvent('mousedown', { bubbles: true, button: 1, buttons: 4, ...middle }),
    );
    await move({ ...middle, clientX: middle.clientX - 90 });
    await up(middle);
    expectSidebar(320);
  });
  it('方向键与 Home/End 受约束，双击只重置对应方向，重置入口恢复两轴', async () => {
    const bar = await setup();
    const handle = bar.get('#sidebar-width-handle');
    await handle.trigger('keydown', { key: 'ArrowLeft' });
    await flushPromises();
    expect(sidebarWidth()).toBeGreaterThan(320);
    expect(localStorage.getItem(PANEL_LAYOUT_KEY)).not.toBeNull();
    await handle.trigger('keydown', { key: 'Home' });
    await flushPromises();
    expectSidebar(560);
    await handle.trigger('keydown', { key: 'End' });
    await flushPromises();
    expectSidebar(260);
    await bar.get('#sidebar-height-handle').trigger('keydown', { key: 'ArrowDown' });
    await flushPromises();
    const top = percent('workspace-inspector');
    expect(top).toBeGreaterThan(36);
    await handle.trigger('dblclick');
    await flushPromises();
    expectSidebar(320);
    expect(percent('workspace-inspector')).toBeCloseTo(top, 1);
    bar.vm.reset();
    await flushPromises();
    expect(percent('workspace-inspector')).toBeCloseTo(36, 1);
  });
  it('保存后重挂载恢复；窗口缩小仅约束展示，不改写偏好，放大恢复', async () => {
    localStorage.setItem(
      PANEL_LAYOUT_KEY,
      JSON.stringify({ version: 1, sidebarWidth: 550, inspectorPercent: 55 }),
    );
    const bar = await setup();
    expectSidebar(550);
    width = 900;
    await measure();
    expectSidebar(492);
    const point = await down('sidebar-width-handle');
    await up(point);
    expect(JSON.parse(localStorage.getItem(PANEL_LAYOUT_KEY)!).sidebarWidth).toBe(550);
    width = 1200;
    await measure();
    expectSidebar(550);
    bar.unmount();
    wrapper = undefined;
    await setup();
    expect(percent('workspace-inspector')).toBeCloseTo(55, 1);
  });
  it('损坏或拒绝存储可恢复，失败明确提示且不阻止本次调整', async () => {
    localStorage.setItem(PANEL_LAYOUT_KEY, '{');
    const bar = await setup();
    expect(bar.get('[role="status"]').text()).toContain('无法恢复面板布局');
    expectSidebar(320);
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('存储不可用');
    });
    await bar.get('#sidebar-width-handle').trigger('keydown', { key: 'ArrowLeft' });
    await flushPromises();
    expect(sidebarWidth()).toBeGreaterThan(320);
    expect(bar.get('[role="status"]').text()).toContain('无法保存：存储不可用');
  });
});
