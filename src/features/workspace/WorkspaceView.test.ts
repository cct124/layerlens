import { flushPromises, mount } from '@vue/test-utils';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { VueWrapper } from '@vue/test-utils';
import type { EventCallback } from '@tauri-apps/api/event';
import type {
  LayerResponse,
  WorkspaceDocument,
  WorkspaceSnapshot,
} from '../../shared/api/generated';
import WorkspaceView from './WorkspaceView.vue';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
const doc = (id: string): WorkspaceDocument => ({
  id,
  revision: id,
  name: `design-${id}.psd`,
  path: `C:/design-${id}.psd`,
  width: 1000,
  height: 800,
  layerCount: 3,
  colorMode: 'RGB',
  bitDepth: 8,
  previewNote: '保存时合成图，无 ICC 转换',
  selectedLayerId: null,
});
const state = (
  sequence: string,
  ids: string[] = [],
  active = ids[0] ?? null,
): WorkspaceSnapshot => ({
  protocolVersion: 1,
  sequence,
  documents: ids.map(doc),
  activeDocumentId: active,
  jobs: [],
  preview: active
    ? { documentId: active, revision: active, state: { phase: 'ready', cacheHit: false } }
    : null,
  notices: [],
  resources: { sourceBytes: '0', decodedBytes: '0', outputBytes: '0', cacheBytes: '0' },
  shuttingDown: false,
});
const png = () => new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]).buffer;
/** 测试默认图层查询返回空页，避免图层树因响应无法校验而报错。 */
const emptyLayers = (documentId: string, revision: string): LayerResponse => ({
  documentId,
  revision,
  result: { kind: 'list', page: { offset: 0, total: 0, nextOffset: null, layers: [] } },
});
const layerRequest = (args: unknown) =>
  (args as { request: { documentId: string; revision: string } }).request;
let wrapper: VueWrapper | undefined;
let receive: EventCallback<unknown>;
const stop = vi.fn();
let current: WorkspaceSnapshot;
function emit(next: unknown) {
  receive({ event: 'workspace-changed', id: 1, payload: next });
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.stubGlobal('URL', { createObjectURL: vi.fn(() => 'blob:preview'), revokeObjectURL: vi.fn() });
  current = state('1');
  vi.mocked(isTauri).mockReturnValue(true);
  vi.mocked(listen).mockImplementation(async (_name, callback) => {
    receive = callback;
    return stop;
  });
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'get_app_info')
      return { appName: 'LayerLens', appVersion: '0.1.0', protocolVersion: 1 };
    if (command === 'read_preview') return png();
    if (command === 'choose_psd_files') return ['C:/picked.psd'];
    if (command === 'read_layers') {
      const request = layerRequest(args);
      return emptyLayers(request.documentId, request.revision);
    }
    return current;
  });
});
afterEach(() => {
  wrapper?.unmount();
  wrapper = undefined;
  vi.unstubAllGlobals();
});

describe('只读桌面工作区', () => {
  it('先订阅再读取状态，文件选择只转发到核心', async () => {
    wrapper = mount(WorkspaceView);
    await flushPromises();
    expect(wrapper.text()).toContain('桌面工作区已连接');
    expect(wrapper.text()).toContain('v0.1.0');
    await wrapper.get('.app-header .primary-button').trigger('click');
    await flushPromises();
    expect(invoke).toHaveBeenCalledWith('workspace_action', {
      request: { protocolVersion: 1, action: { kind: 'open', path: 'C:/picked.psd' } },
    });
    expect(wrapper.findAll('.document-tab')).toHaveLength(0);
  });
  it('浏览器预览不调用桌面能力', async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    wrapper = mount(WorkspaceView);
    await flushPromises();
    expect(invoke).not.toHaveBeenCalled();
    expect(listen).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain('浏览器预览');
  });
  it('取消文件选择不提交打开请求，也不改变已有标签和视图', async () => {
    current = state('1', ['1']);
    const base = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args, options) =>
      command === 'choose_psd_files' ? Promise.resolve([]) : base(command, args, options),
    );
    wrapper = mount(WorkspaceView);
    await flushPromises();
    await wrapper.get('[aria-label="放大"]').trigger('click');
    const transform = wrapper.get('img').attributes('style');
    const actions = () =>
      vi.mocked(invoke).mock.calls.filter((call) => call[0] === 'workspace_action');
    const before = actions().length;
    await wrapper.get('.app-header .primary-button').trigger('click');
    await flushPromises();
    expect(actions()).toHaveLength(before);
    expect(wrapper.findAll('.document-tab')).toHaveLength(1);
    expect(wrapper.get('img').attributes('style')).toBe(transform);
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.get('.app-header .primary-button').attributes('disabled')).toBeUndefined();
  });
  it('多选按序提交，队列拒绝后停止后续文件并保留已打开文档', async () => {
    current = state('1', ['1']);
    const first = deferred<WorkspaceSnapshot>();
    const base = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args, options) => {
      if (command === 'choose_psd_files')
        return Promise.resolve(['C:/first.psd', 'C:/second.psd', 'C:/third.psd']);
      if (command === 'workspace_action') {
        const { action } = (args as { request: { action: { kind: string; path?: string } } })
          .request;
        if (action.kind === 'open')
          return action.path === 'C:/first.psd'
            ? first.promise
            : Promise.reject({ code: 'BUSY', message: '队列已满' });
      }
      return base(command, args, options);
    });
    wrapper = mount(WorkspaceView);
    await flushPromises();
    const actions = () =>
      vi
        .mocked(invoke)
        .mock.calls.filter((call) => call[0] === 'workspace_action')
        .map(
          (call) =>
            (call[1] as { request: { action: { kind: string; path?: string } } }).request.action,
        )
        .filter((action) => action.kind === 'open');
    await wrapper.get('.app-header .primary-button').trigger('click');
    await flushPromises();
    expect(actions()).toEqual([{ kind: 'open', path: 'C:/first.psd' }]);
    expect(wrapper.get('.app-header .primary-button').attributes('disabled')).toBeDefined();
    first.resolve(state('2', ['1', '2'], '2'));
    await flushPromises();
    expect(actions()).toEqual([
      { kind: 'open', path: 'C:/first.psd' },
      { kind: 'open', path: 'C:/second.psd' },
    ]);
    expect(wrapper.findAll('.document-tab')).toHaveLength(2);
    expect(wrapper.get('[role="alert"]').text()).toContain('C:/second.psd');
    expect(wrapper.get('[role="alert"]').text()).toContain('队列已满');
    expect(wrapper.get('[role="alert"]').text()).toContain('本批后续文件尚未打开');
    expect(wrapper.get('.app-header .primary-button').attributes('disabled')).toBeUndefined();
  });
  it('晚到的初始化响应和旧事件不能覆盖新快照', async () => {
    const response = deferred<WorkspaceSnapshot>();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === 'workspace_action') return response.promise;
      if (command === 'read_preview') return Promise.resolve(png());
      if (command === 'read_layers') {
        const request = layerRequest(args);
        return Promise.resolve(emptyLayers(request.documentId, request.revision));
      }
      return Promise.resolve({ appName: 'LayerLens', appVersion: '0.1.0', protocolVersion: 1 });
    });
    wrapper = mount(WorkspaceView);
    await flushPromises();
    emit(state('9007199254740994', ['1']));
    await flushPromises();
    response.resolve(state('1'));
    emit(state('9007199254740993', ['2']));
    await flushPromises();
    expect(wrapper.get('.document-details summary').text()).toContain('design-1.psd');
  });
  it('拒绝修订关联失配的通知并保留有效状态', async () => {
    current = state('1', ['1']);
    wrapper = mount(WorkspaceView);
    await flushPromises();
    emit({
      ...state('2', ['2']),
      preview: { documentId: '1', revision: '1', state: { phase: 'ready', cacheHit: false } },
    });
    await flushPromises();
    expect(wrapper.get('.document-details summary').text()).toContain('design-1.psd');
    expect(wrapper.get('[role="alert"]').text()).toContain('通知无效');
  });
  it('图像串行读取，切换后丢弃旧结果并在关闭时撤销 URL', async () => {
    const old = deferred<ArrayBuffer>();
    current = state('1', ['1']);
    const base = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args, options) =>
      command === 'read_preview' &&
      vi.mocked(invoke).mock.calls.filter((c) => c[0] === 'read_preview').length === 1
        ? old.promise
        : base(command, args, options),
    );
    wrapper = mount(WorkspaceView);
    await flushPromises();
    emit(state('2', ['1', '2'], '2'));
    await flushPromises();
    expect(vi.mocked(invoke).mock.calls.filter((c) => c[0] === 'read_preview')).toHaveLength(1);
    old.resolve(png());
    await flushPromises();
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('read_preview', {
      request: { protocolVersion: 1, documentId: '2', revision: '2' },
    });
    emit(state('3'));
    await flushPromises();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:preview');
    expect(wrapper.find('img').exists()).toBe(false);
  });
  it('每个文档保留自己的缩放和平移', async () => {
    current = state('1', ['1', '2'], '1');
    wrapper = mount(WorkspaceView);
    await flushPromises();
    await wrapper.get('[aria-label="放大"]').trigger('click');
    await wrapper.get('[aria-label="PSD 预览画布"]').trigger('keydown', { key: 'ArrowRight' });
    const transform = wrapper.get('img').attributes('style');
    emit(state('2', ['1', '2'], '2'));
    await flushPromises();
    expect(wrapper.get('img').attributes('style')).not.toBe(transform);
    emit(state('3', ['1', '2'], '1'));
    await flushPromises();
    expect(wrapper.get('img').attributes('style')).toBe(transform);
  });
  it('预览失败保留文档信息并允许重试', async () => {
    current = state('1', ['1']);
    current.preview = {
      documentId: '1',
      revision: '1',
      state: { phase: 'failed', error: { code: 'PREVIEW_FAILED', message: '缺少合成图' } },
    };
    wrapper = mount(WorkspaceView);
    await flushPromises();
    expect(wrapper.text()).toContain('缺少合成图');
    expect(wrapper.text()).toContain('1000 × 800');
    await wrapper.get('.canvas-message button').trigger('click');
    expect(invoke).toHaveBeenCalledWith('workspace_action', {
      request: { protocolVersion: 1, action: { kind: 'retryPreview' } },
    });
  });
  it('订阅失败可重连，销毁后移除监听且丢弃晚到图像', async () => {
    vi.mocked(listen).mockRejectedValueOnce(new Error('连接中断'));
    wrapper = mount(WorkspaceView);
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain('连接中断');
    const image = deferred<ArrayBuffer>();
    current = state('1', ['1']);
    const base = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args, options) =>
      command === 'read_preview' ? image.promise : base(command, args, options),
    );
    await wrapper.get('.error-notice button').trigger('click');
    await flushPromises();
    wrapper.unmount();
    wrapper = undefined;
    image.resolve(png());
    await flushPromises();
    expect(stop).toHaveBeenCalledOnce();
    expect(URL.createObjectURL).not.toHaveBeenCalled();
  });
});
