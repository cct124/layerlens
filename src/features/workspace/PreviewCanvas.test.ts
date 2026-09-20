import { mount } from '@vue/test-utils';
import { nextTick } from 'vue';
import type { VueWrapper } from '@vue/test-utils';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import PreviewCanvas from './PreviewCanvas.vue';
import type { CanvasTool } from './regionDraft';
import type { SelectionBounds } from '../../shared/api/generated';

let wrapper: VueWrapper<InstanceType<typeof PreviewCanvas>>;
const original = { x: 200, y: 200, width: 300, height: 200 };
let resizeViewport: (width: number, height: number) => void;
beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(private callback: ResizeObserverCallback) {}
      observe(target: Element) {
        resizeViewport = (width, height) =>
          this.callback(
            [
              {
                target,
                contentRect: new DOMRect(0, 0, width, height),
                borderBoxSize: [],
                contentBoxSize: [],
                devicePixelContentBoxSize: [],
              },
            ],
            this,
          );
      }
      unobserve() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  wrapper?.unmount();
  vi.unstubAllGlobals();
});
function setup(region: SelectionBounds | null = original, tool: CanvasTool = 'region') {
  wrapper = mount(PreviewCanvas, {
    props: {
      width: 1000,
      height: 800,
      url: 'blob:preview',
      name: 'sample',
      bounds: null,
      view: { mode: 'manual', zoom: 1, x: 0, y: 0 },
      tool,
      region,
      contextKey: 'session/doc/revision/selection',
      disabled: false,
    },
  });
  const viewport = wrapper.get<HTMLElement>('.preview-viewport');
  vi.spyOn(viewport.element, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 1000, 800));
  let captured = false;
  viewport.element.setPointerCapture = () => {
    captured = true;
  };
  viewport.element.hasPointerCapture = () => captured;
  viewport.element.releasePointerCapture = () => {
    captured = false;
  };
  return viewport;
}
async function pointer(
  target: { element: Element },
  type: string,
  x: number,
  y: number,
  id = 1,
  button = 0,
  buttons = type === 'pointerup' ? 0 : 1,
) {
  // jsdom 的 PointerEvent 继承只读 MouseEvent 属性，直接构造事件避免 trigger 的属性回填。
  target.element.dispatchEvent(
    new PointerEvent(type, {
      bubbles: true,
      cancelable: true,
      button,
      buttons,
      pointerId: id,
      clientX: x,
      clientY: y,
    }),
  );
  await nextTick();
}

it('新建草稿随拖动显示，松开只提交一次且等待权威选区确认', async () => {
  const viewport = setup();
  await pointer(viewport, 'pointerdown', 100, 120);
  await pointer(viewport, 'pointermove', 180, 170);
  expect(wrapper.get('.region-draft').attributes('style')).toContain('width: 80px');
  expect(wrapper.emitted('region')).toBeUndefined();
  await pointer(viewport, 'pointerup', 190, 180);
  expect(wrapper.emitted('region')).toEqual([
    [{ x: 100, y: 120, width: 90, height: 60 }, 'session/doc/revision/selection'],
  ]);
  expect(wrapper.get('.region-boundary').attributes('style')).toContain('width: 300px');
  await pointer(viewport, 'lostpointercapture', 190, 180);
  expect(wrapper.emitted('region')).toHaveLength(1);
  await wrapper.setProps({ region: { x: 100, y: 120, width: 90, height: 60 }, contextKey: 'next' });
  expect(wrapper.get('.region-boundary').attributes('style')).toContain('width: 90px');
});

it('框内移动保持尺寸，控制点调整尺寸，重新框选支持从原范围内部新建', async () => {
  const viewport = setup();
  await pointer(wrapper.get('.region-boundary'), 'pointerdown', 300, 300);
  await pointer(viewport, 'pointerup', 320, 310);
  expect(wrapper.emitted('region')?.[0]?.[0]).toEqual({ x: 220, y: 210, width: 300, height: 200 });
  await pointer(wrapper.get('[data-region-handle="se"]'), 'pointerdown', 500, 400);
  await pointer(viewport, 'pointerup', 570, 480);
  expect(wrapper.emitted('region')?.[1]?.[0]).toEqual({ x: 200, y: 200, width: 370, height: 280 });
  await wrapper.get('.region-controls button').trigger('click');
  expect(wrapper.find('.region-editable').exists()).toBe(false);
  await pointer(viewport, 'pointerdown', 250, 250);
  await pointer(viewport, 'pointerup', 350, 350);
  expect(wrapper.emitted('region')?.[2]?.[0]).toEqual({ x: 250, y: 250, width: 100, height: 100 });
});

it.each(['viewport', 'region', 'handle'])(
  '从 %s 按中键临时平移，松开恢复框选并保留原范围',
  async (target) => {
    const viewport = setup();
    const start =
      target === 'viewport'
        ? viewport
        : wrapper.get(target === 'region' ? '.region-boundary' : '[data-region-handle="se"]');
    const down = new PointerEvent('pointerdown', {
      bubbles: true,
      cancelable: true,
      button: 1,
      buttons: 4,
      pointerId: 1,
      clientX: 500,
      clientY: 400,
    });
    start.element.dispatchEvent(down);
    await nextTick();
    expect(down.defaultPrevented).toBe(true);
    expect(wrapper.find('.panning').exists()).toBe(true);
    await pointer(viewport, 'pointermove', 550, 430, 1, -1, 4);
    expect(wrapper.emitted('update:view')?.[0]?.[0]).toEqual({
      mode: 'manual',
      zoom: 1,
      x: 50,
      y: 30,
    });
    await wrapper.setProps({ view: { mode: 'manual', zoom: 1, x: 50, y: 30 } });
    expect(wrapper.props('region')).toEqual(original);
    expect(wrapper.get('.region-boundary').attributes('style')).toContain('width: 300px');
    await pointer(viewport, 'pointerup', 550, 430, 1, 1, 0);
    expect(wrapper.find('.panning').exists()).toBe(false);
    expect(wrapper.props('tool')).toBe('region');
    expect(wrapper.emitted('region')).toBeUndefined();
    await pointer(viewport, 'pointerdown', 100, 100);
    await pointer(viewport, 'pointerup', 150, 150);
    expect(wrapper.emitted('region')?.[0]?.[0]).toEqual({ x: 50, y: 70, width: 50, height: 50 });
  },
);

it.each(['create', 'move', 'resize'])(
  '左键 %s 中追加中键取消草稿；中键先松开也能退出平移',
  async (action) => {
    const viewport = setup();
    const target =
      action === 'create'
        ? viewport
        : wrapper.get(action === 'move' ? '.region-boundary' : '[data-region-handle="se"]');
    await pointer(target, 'pointerdown', 100, 100);
    await pointer(viewport, 'pointermove', 140, 150);
    expect(wrapper.find('.region-draft').exists()).toBe(true);
    // 实际鼠标组合按键通过 pointermove/buttons=5 表达，不会再发 pointerdown。
    await pointer(viewport, 'pointermove', 140, 150, 1, 1, 5);
    expect(wrapper.find('.region-draft').exists()).toBe(false);
    expect(wrapper.get('.region-boundary').attributes('style')).toContain('width: 300px');
    await pointer(viewport, 'pointermove', 170, 180, 1, -1, 5);
    expect(wrapper.emitted('update:view')?.[0]?.[0]).toMatchObject({ x: 30, y: 30 });
    await pointer(viewport, 'pointermove', 170, 180, 1, 1, 1);
    expect(wrapper.find('.panning').exists()).toBe(false);
    await pointer(viewport, 'pointermove', 190, 190);
    await pointer(viewport, 'pointerup', 190, 190);
    expect(wrapper.emitted('update:view')).toHaveLength(1);
    expect(wrapper.emitted('region')).toBeUndefined();
  },
);

it.each(['blur', 'pointercancel', 'lostpointercapture', 'context'])(
  '中键平移遇到 %s 后不再跟随指针',
  async (reason) => {
    const viewport = setup();
    await pointer(viewport, 'pointerdown', 100, 100, 1, 1, 4);
    if (reason === 'blur') window.dispatchEvent(new Event('blur'));
    else if (reason === 'context') await wrapper.setProps({ contextKey: 'new-document' });
    else await pointer(viewport, reason, 100, 100, 1, 1, 4);
    await pointer(viewport, 'pointermove', 150, 150, 1, -1, 4);
    expect(wrapper.emitted('update:view')).toBeUndefined();
    expect(wrapper.find('.panning').exists()).toBe(false);
    expect(wrapper.props('region')).toEqual(original);
  },
);

it('中键默认滚屏与辅助点击被阻止，右键默认行为保持', () => {
  const viewport = setup();
  for (const type of ['mousedown', 'auxclick']) {
    for (const button of [1, 2]) {
      const event = new MouseEvent(type, { bubbles: true, cancelable: true, button });
      viewport.element.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(button === 1);
    }
  }
});

it('空闲 Esc 请求清空但不切工具；忙碌、移动模式、输入法和按键重复不触发清空', async () => {
  const viewport = setup();
  await viewport.trigger('keydown', { key: 'Escape', repeat: true });
  await viewport.trigger('keydown', { key: 'Escape', isComposing: true });
  await wrapper.setProps({ disabled: true });
  await viewport.trigger('keydown', { key: 'Escape' });
  await wrapper.setProps({ disabled: false, tool: 'pan' });
  await viewport.trigger('keydown', { key: 'Escape' });
  expect(wrapper.emitted('clearSelection')).toBeUndefined();
  await wrapper.setProps({ tool: 'region' });
  await viewport.trigger('keydown', { key: 'Escape' });
  expect(wrapper.emitted('clearSelection')).toEqual([['session/doc/revision/selection']]);
  expect(wrapper.props('tool')).toBe('region');
  expect(wrapper.props('region')).toEqual(original);
});

it.each(['region', 'pan'])(
  '%s 拖动中 Esc 只取消手势，长按不清空，再次单按才清空',
  async (gesture) => {
    const viewport = setup();
    await pointer(
      viewport,
      'pointerdown',
      100,
      100,
      1,
      gesture === 'pan' ? 1 : 0,
      gesture === 'pan' ? 4 : 1,
    );
    await pointer(viewport, 'pointermove', 150, 150, 1, -1, gesture === 'pan' ? 4 : 1);
    await viewport.trigger('keydown', { key: 'Escape' });
    await viewport.trigger('keydown', { key: 'Escape', repeat: true });
    await pointer(viewport, 'pointerup', 150, 150);
    expect(wrapper.emitted('clearSelection')).toBeUndefined();
    expect(wrapper.emitted('region')).toBeUndefined();
    expect(wrapper.find('.region-draft').exists()).toBe(false);
    expect(wrapper.find('.panning').exists()).toBe(false);
    await viewport.trigger('keydown', { key: 'Escape' });
    expect(wrapper.emitted('clearSelection')).toHaveLength(1);
  },
);

it.each([
  'esc',
  'pointercancel',
  'lostpointercapture',
  'tool',
  'context',
  'disabled',
  'blur',
  'zoom',
  'view',
  'explicit',
  'resize',
])('%s 取消草稿，保留已提交范围且晚到 pointerup 不提交', async (kind) => {
  const viewport = setup();
  const before = wrapper.get('.region-boundary').attributes('style');
  await pointer(viewport, 'pointerdown', 100, 100);
  await pointer(viewport, 'pointermove', 160, 170);
  if (kind === 'esc') await viewport.trigger('keydown', { key: 'Escape' });
  else if (kind === 'tool') await wrapper.setProps({ tool: 'pan' });
  else if (kind === 'context')
    await wrapper.setProps({ contextKey: 'another-document-or-revision' });
  else if (kind === 'disabled') await wrapper.setProps({ disabled: true });
  else if (kind === 'blur') window.dispatchEvent(new Event('blur'));
  else if (kind === 'zoom')
    viewport.element.dispatchEvent(
      new WheelEvent('wheel', { deltaY: 30, clientX: 100, clientY: 100 }),
    );
  else if (kind === 'view')
    await wrapper.setProps({ view: { mode: 'manual', zoom: 1, x: 0, y: 0 } });
  else if (kind === 'explicit') wrapper.vm.cancel();
  else if (kind === 'resize') resizeViewport(900, 700);
  else await pointer(viewport, kind, 160, 170);
  await pointer(viewport, 'pointerup', 180, 190);
  expect(wrapper.emitted('region')).toBeUndefined();
  expect(wrapper.find('.region-draft').exists()).toBe(false);
  expect(wrapper.get('.region-boundary').attributes('style')).toBe(before);
});

it('点击、画布外起点、第二指针与无预览不能生成区域；平移工具不改变区域', async () => {
  const viewport = setup(null);
  await pointer(viewport, 'pointerdown', -20, 100);
  await pointer(viewport, 'pointerup', 200, 200);
  await pointer(viewport, 'pointerdown', 100, 100);
  await pointer(viewport, 'pointermove', 300, 300, 2);
  await pointer(viewport, 'pointerup', 300, 300, 2);
  await pointer(viewport, 'pointerup', 101, 101);
  expect(wrapper.emitted('region')).toBeUndefined();
  await wrapper.setProps({ url: null });
  await pointer(viewport, 'pointerdown', 100, 100);
  await pointer(viewport, 'pointerup', 200, 200);
  expect(wrapper.emitted('region')).toBeUndefined();
  await wrapper.setProps({ tool: 'pan', url: 'blob:preview', region: original });
  await pointer(viewport, 'pointerdown', 100, 100);
  await pointer(viewport, 'pointermove', 200, 200);
  await pointer(viewport, 'pointerup', 200, 200);
  expect(wrapper.emitted('update:view')?.[0]?.[0]).toMatchObject({ x: 100, y: 100 });
  expect(wrapper.emitted('region')).toBeUndefined();
});

it('面板改变视口尺寸时保持手动缩放和关注位置，适应窗口模式才自动重算', async () => {
  setup();
  await wrapper.setProps({ view: { mode: 'manual', zoom: 1.25, x: 170, y: -30 } });
  resizeViewport(1000, 800);
  await nextTick();
  const before = wrapper.get('.preview-image').attributes('style');
  resizeViewport(750, 800);
  await nextTick();
  expect(wrapper.get('.preview-image').attributes('style')).toBe(before);
  expect(wrapper.get('[aria-label="当前缩放"]').text()).toBe('125%');
  expect(wrapper.emitted('update:view')).toBeUndefined();
  expect(wrapper.emitted('clearSelection')).toBeUndefined();
  expect(wrapper.emitted('region')).toBeUndefined();
  await wrapper.setProps({ view: { mode: 'fit', zoom: 1, x: 0, y: 0 } });
  const fitBefore = wrapper.get('[aria-label="当前缩放"]').text();
  resizeViewport(450, 800);
  await nextTick();
  expect(wrapper.get('[aria-label="当前缩放"]').text()).not.toBe(fitBefore);
  expect(wrapper.find('.region-boundary').exists()).toBe(true);
});
