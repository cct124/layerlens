import { flushPromises, mount } from '@vue/test-utils';
import type { VueWrapper } from '@vue/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import WorkspaceTitleBar from './WorkspaceTitleBar.vue';

const api = vi.hoisted(() => ({
  getCurrentWindow: vi.fn(),
  isMaximized: vi.fn<() => Promise<boolean>>(),
  onResized: vi.fn<(callback: () => void) => Promise<() => void>>(),
  minimize: vi.fn<() => Promise<void>>(),
  toggleMaximize: vi.fn<() => Promise<void>>(),
  close: vi.fn<() => Promise<void>>(),
  startDragging: vi.fn<() => Promise<void>>(),
}));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: api.getCurrentWindow }));
let wrapper: VueWrapper | undefined;
let resized: () => void;
const stop = vi.fn();
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
async function mountTitlebar(desktop = true) {
  wrapper = mount(WorkspaceTitleBar, {
    props: { desktop, connected: desktop, picking: false },
    slots: { default: '<nav><button aria-label="文件"><span>文件</span></button></nav>' },
  });
  await flushPromises();
  return wrapper;
}
async function mouseDown(selector: string, options: MouseEventInit = {}) {
  wrapper!.get(selector).element.dispatchEvent(
    new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      button: 0,
      buttons: 1,
      detail: 1,
      ...options,
    }),
  );
  await flushPromises();
}
beforeEach(() => {
  vi.resetAllMocks();
  api.getCurrentWindow.mockReturnValue(api);
  api.isMaximized.mockResolvedValue(false);
  api.onResized.mockImplementation(async (callback) => {
    resized = callback;
    return stop;
  });
  api.minimize.mockResolvedValue();
  api.toggleMaximize.mockResolvedValue();
  api.close.mockResolvedValue();
  api.startDragging.mockResolvedValue();
});
afterEach(() => {
  wrapper?.unmount();
  wrapper = undefined;
});

describe('合并标题栏', () => {
  it('窗口命令调用原生 API，最大化状态由实际窗口决定；关闭不是关闭文档', async () => {
    const bar = await mountTitlebar();
    expect(bar.find('[aria-label="最大化窗口"]').exists()).toBe(true);
    await bar.get('[aria-label="最小化窗口"]').trigger('click');
    await flushPromises();
    expect(api.minimize).toHaveBeenCalledOnce();
    api.isMaximized.mockResolvedValue(true);
    await bar.get('[aria-label="最大化窗口"]').trigger('click');
    await flushPromises();
    expect(api.toggleMaximize).toHaveBeenCalledOnce();
    expect(bar.find('[aria-label="还原窗口"]').exists()).toBe(true);
    await bar.get('[aria-label="关闭窗口"]').trigger('click');
    await flushPromises();
    expect(api.close).toHaveBeenCalledOnce();
  });

  it('空白区和品牌子图标可以拖动；双击只切换一次最大化，菜单和窗口按钮不会误拖动', async () => {
    const bar = await mountTitlebar();
    await mouseDown('.header-spacer');
    await mouseDown('.brand svg');
    expect(api.startDragging).toHaveBeenCalledTimes(2);
    await mouseDown('.header-spacer', { detail: 2 });
    expect(api.toggleMaximize).toHaveBeenCalledOnce();
    await bar.get('.header-spacer').trigger('dblclick');
    await mouseDown('nav span');
    await mouseDown('[aria-label="最小化窗口"] svg');
    await mouseDown('.header-spacer', { button: 2, buttons: 2 });
    await mouseDown('.header-spacer', { buttons: 3 });
    expect(api.startDragging).toHaveBeenCalledTimes(2);
    expect(api.toggleMaximize).toHaveBeenCalledOnce();
  });

  it('系统 resize 和重新获得焦点同步最大化状态', async () => {
    const bar = await mountTitlebar();
    api.isMaximized.mockResolvedValue(true);
    resized();
    await flushPromises();
    expect(bar.find('[aria-label="还原窗口"]').exists()).toBe(true);
    api.isMaximized.mockResolvedValue(false);
    window.dispatchEvent(new Event('focus'));
    await flushPromises();
    expect(bar.find('[aria-label="最大化窗口"]').exists()).toBe(true);
  });

  it('合并密集 resize 查询，过时状态不覆盖后续事件，卸载后不再查询', async () => {
    const first = deferred<boolean>();
    api.isMaximized.mockReturnValueOnce(first.promise);
    const bar = await mountTitlebar();
    for (let i = 0; i < 20; i++) resized();
    expect(api.isMaximized).toHaveBeenCalledOnce();
    api.isMaximized.mockResolvedValue(true);
    first.resolve(false);
    await flushPromises();
    expect(api.isMaximized).toHaveBeenCalledTimes(2);
    expect(bar.find('[aria-label="还原窗口"]').exists()).toBe(true);
    bar.unmount();
    wrapper = undefined;
    expect(stop).toHaveBeenCalledOnce();
    resized();
    window.dispatchEvent(new Event('focus'));
    expect(api.isMaximized).toHaveBeenCalledTimes(2);
  });

  it('卸载期间晚到的订阅立即释放，未发起状态查询', async () => {
    const subscription = deferred<() => void>();
    api.onResized.mockReturnValueOnce(subscription.promise);
    const bar = await mountTitlebar();
    bar.unmount();
    wrapper = undefined;
    subscription.resolve(stop);
    await flushPromises();
    expect(stop).toHaveBeenCalledOnce();
    expect(api.isMaximized).not.toHaveBeenCalled();
  });

  it('等待命令时禁止重复，失败显示原因且可重试，状态读取不会清掉操作错误', async () => {
    const command = deferred<void>();
    api.toggleMaximize.mockReturnValueOnce(command.promise);
    const bar = await mountTitlebar();
    await bar.get('[aria-label="最大化窗口"]').trigger('click');
    expect(bar.get('[aria-label="关闭窗口"]').attributes('disabled')).toBeDefined();
    await mouseDown('.header-spacer', { detail: 2 });
    expect(api.toggleMaximize).toHaveBeenCalledOnce();
    command.resolve();
    await flushPromises();
    api.close.mockRejectedValueOnce(new Error('系统拒绝关闭'));
    await bar.get('[aria-label="关闭窗口"]').trigger('click');
    await flushPromises();
    resized();
    await flushPromises();
    expect(bar.get('[role="alert"]').text()).toContain('关闭窗口失败：系统拒绝关闭');
    expect(bar.get('[aria-label="关闭窗口"]').attributes('disabled')).toBeUndefined();
    await bar.get('[aria-label="关闭窗口"]').trigger('click');
    await flushPromises();
    expect(api.close).toHaveBeenCalledTimes(2);
    expect(bar.find('[role="alert"]').exists()).toBe(false);
  });

  it('查询或订阅失败仍保留可用窗口按钮，焦点重读可恢复状态；拖动失败明确提示', async () => {
    api.onResized.mockRejectedValueOnce('监听被拒绝');
    api.isMaximized.mockRejectedValueOnce('状态被拒绝');
    const bar = await mountTitlebar();
    expect(bar.get('[role="alert"]').text()).toContain('订阅窗口状态失败');
    expect(bar.get('[aria-label="最大化 / 还原窗口"]').attributes('disabled')).toBeUndefined();
    await bar.get('[aria-label="关闭窗口提示"]').trigger('click');
    window.dispatchEvent(new Event('focus'));
    await flushPromises();
    expect(bar.find('[aria-label="最大化窗口"]').exists()).toBe(true);
    api.startDragging.mockRejectedValueOnce('拖动被拒绝');
    await mouseDown('.header-spacer');
    expect(bar.get('[role="alert"]').text()).toContain('拖动窗口失败：拖动被拒绝');
  });

  it('浏览器预览无窗口按钮、不访问 Tauri，仍显示菜单及只读说明', async () => {
    const bar = await mountTitlebar(false);
    expect(bar.find('[aria-label="窗口控制"]').exists()).toBe(false);
    expect(bar.find('[aria-label="文件"]').exists()).toBe(true);
    expect(bar.text()).toContain('浏览器预览');
    await mouseDown('.header-spacer');
    expect(api.getCurrentWindow).not.toHaveBeenCalled();
    expect(api.onResized).not.toHaveBeenCalled();
    expect(api.startDragging).not.toHaveBeenCalled();
  });
});
