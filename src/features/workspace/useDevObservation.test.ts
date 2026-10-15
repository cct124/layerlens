import { defineComponent, h, ref } from 'vue';
import { flushPromises, mount } from '@vue/test-utils';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useDevObservation } from './useDevObservation';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const active: ReturnType<typeof mount>[] = [];
function setup(checkDom = false) {
  const sequence = ref('1');
  const wrapper = mount(
    defineComponent({
      setup() {
        useDevObservation(() => ({
          coreSequence: sequence.value,
          sessionId: 'session',
          activeDocument: checkDom
            ? { id: document.getElementById('dev-sequence')?.textContent ?? '0', revision: '1' }
            : null,
          selectionRevision: '1',
          snapshotId: null,
          region: null,
          tool: 'pan',
          panel: 'layers',
          canvas: null,
          cleanup: {
            opened: false,
            phase: null,
            planId: null,
            taskIds: [],
            syncing: false,
            syncError: false,
            pending: false,
          },
          tasks: [],
          selectionRunning: false,
          content: null,
        }));
        return () =>
          h(
            'button',
            {
              id: 'dev-sequence',
              onPointerdown: () => {
                sequence.value = '2';
              },
            },
            sequence.value,
          );
      },
    }),
    { attachTo: document.body },
  );
  active.push(wrapper);
  return { sequence, wrapper };
}
afterEach(() => {
  for (const wrapper of active.splice(0)) wrapper.unmount();
  delete window.__LAYERLENS_DEV_BOOT__;
  delete window.__LAYERLENS_DEV_VIEW__;
  vi.restoreAllMocks();
  vi.mocked(invoke).mockReset();
});
describe('开发观测生命周期', () => {
  it('捕获阶段触发上报时，同一输入造成的 DOM 更新必须先完成', async () => {
    window.__LAYERLENS_DEV_BOOT__ = { instanceId: 'instance' };
    const domAtSend: (string | null)[] = [];
    vi.mocked(invoke).mockImplementation(async () => {
      domAtSend.push(document.getElementById('dev-sequence')?.textContent ?? null);
    });
    const { wrapper } = setup(true);
    await flushPromises();
    await wrapper.get('button').trigger('pointerdown');
    await flushPromises();
    for (const [index, [, args]] of vi.mocked(invoke).mock.calls.entries()) {
      expect(args).toMatchObject({
        observation: {
          coreSequence: domAtSend[index],
          activeDocument: { id: domAtSend[index], revision: '1' },
        },
      });
    }
    expect(vi.mocked(invoke).mock.calls.at(-1)?.[1]).toMatchObject({
      observation: { coreSequence: '2', activeDocument: { id: '2' } },
    });
  });
  it('普通实例不注册观测桥或上报命令', async () => {
    setup();
    await flushPromises();
    expect(invoke).not.toHaveBeenCalled();
    expect(window.__LAYERLENS_DEV_VIEW__).toBeUndefined();
  });
  it('真实应用序号进入投影，慢 IPC 只保留最新待发状态，读取不能回写', async () => {
    window.__LAYERLENS_DEV_BOOT__ = { instanceId: 'instance' };
    let release: (() => void) | undefined;
    vi.mocked(invoke)
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            release = resolve;
          }),
      )
      .mockResolvedValue(undefined);
    const { sequence, wrapper } = setup();
    await flushPromises();
    expect(invoke).toHaveBeenCalledTimes(1);
    sequence.value = '2';
    await flushPromises();
    sequence.value = '3';
    await flushPromises();
    expect(invoke).toHaveBeenCalledTimes(1);
    const read = window.__LAYERLENS_DEV_VIEW__?.read();
    expect(read?.coreSequence).toBe('3');
    if (read) read.coreSequence = '999';
    expect(window.__LAYERLENS_DEV_VIEW__?.read()?.coreSequence).toBe('3');
    release?.();
    await flushPromises();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(vi.mocked(invoke).mock.calls[1]?.[1]).toMatchObject({
      observation: { coreSequence: '3', instanceId: 'instance' },
    });
    wrapper.unmount();
    sequence.value = '4';
    await flushPromises();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(window.__LAYERLENS_DEV_VIEW__).toBeUndefined();
  });
  it('DOM 合成键盘事件不计为真实输入；上报失败可诊断', async () => {
    window.__LAYERLENS_DEV_BOOT__ = { instanceId: 'instance' };
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    vi.mocked(invoke).mockRejectedValue(new Error('disconnected'));
    setup();
    await flushPromises();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    window.dispatchEvent(new FocusEvent('blur'));
    window.dispatchEvent(new FocusEvent('focus'));
    await flushPromises();
    expect(window.__LAYERLENS_DEV_VIEW__?.read()?.input.keyEvents).toBe('0');
    expect(window.__LAYERLENS_DEV_VIEW__?.read()?.window.blurEvents).toBe('0');
    expect(window.__LAYERLENS_DEV_VIEW__?.read()?.window.focusEvents).toBe('0');
    expect(window.__LAYERLENS_DEV_VIEW__?.error).toBe('disconnected');
    expect(warning).toHaveBeenCalledOnce();
  });
});
