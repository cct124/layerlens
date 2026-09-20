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
  SelectionRequest,
  TaskDto,
  LayerDetails,
} from '../../shared/api/generated';
import WorkspaceView from './WorkspaceView.vue';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));
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
  selection: {
    sessionId: 'a'.repeat(32),
    selectionRevision: sequence,
    documentId: active,
    documentRevision: active,
    scope: null,
    layerIds: [],
    region: null,
  },
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
async function fileMenu() {
  const trigger = wrapper!.get('[aria-label="文件"]');
  if (trigger.attributes('aria-expanded') !== 'true') await trigger.trigger('click');
  await flushPromises();
  return wrapper!.get('[aria-label="打开 PSD"]');
}

beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
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
    if (command === 'selection_request')
      return {
        kind: 'tasks',
        page: { sessionId: 'a'.repeat(32), tasks: [], nextAfter: null, total: 0, capacity: 128 },
      };
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

it('画布区域通过桌面接口确认，切换恢复各文档范围；重载发起即取消草稿', async () => {
  current = state('1', ['1', '2']);
  const regionRequests: SelectionRequest[] = [];
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation((command, args, options) => {
    if (command === 'selection_request') {
      const { request } = args as { request: SelectionRequest };
      if (request.operation.kind === 'region') {
        regionRequests.push(request);
        const region = request.operation.bounds;
        current = state('2', ['1', '2']);
        current.selection = {
          ...current.selection,
          region,
          scope: {
            snapshotId: '8',
            documentId: '1',
            documentRevision: '1',
            documentName: 'design-1.psd',
            targetCount: 2,
            contentCount: 2,
            textLayerCount: 1,
          },
        };
        return Promise.resolve({
          kind: 'committed',
          sessionId: current.selection.sessionId,
          selectionRevision: '2',
          snapshotId: '8',
        });
      }
    }
    return base(command, args, options);
  });
  wrapper = mount(WorkspaceView);
  await flushPromises();
  await wrapper.get('[aria-label="区域选择"]').trigger('click');
  await wrapper
    .findAll('button')
    .find((button) => button.text() === '100%')!
    .trigger('click');
  function viewport() {
    const element = wrapper!.get<HTMLElement>('.preview-viewport').element;
    vi.spyOn(element, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 1000, 800));
    element.setPointerCapture = vi.fn();
    element.hasPointerCapture = () => false;
    return element;
  }
  async function pointer(element: Element, type: string, x: number, y: number) {
    element.dispatchEvent(
      new PointerEvent(type, { bubbles: true, button: 0, pointerId: 1, clientX: x, clientY: y }),
    );
    await flushPromises();
  }
  const first = viewport();
  await pointer(first, 'pointerdown', 100, 120);
  await pointer(first, 'pointermove', 220, 240);
  expect(regionRequests).toHaveLength(0);
  await pointer(first, 'pointerup', 220, 240);
  expect(regionRequests[0]?.operation).toEqual({
    kind: 'region',
    documentId: '1',
    documentRevision: '1',
    expectedRevision: '1',
    bounds: { x: 100, y: 120, width: 120, height: 120 },
  });
  expect(wrapper.text()).toContain('已框选区域');
  expect(wrapper.get('.region-boundary').attributes('style')).toContain('width: 120px');
  const selected = current.selection;
  emit(state('3', ['1', '2'], '2'));
  await flushPromises();
  expect(wrapper.find('.region-boundary').exists()).toBe(false);
  current = { ...state('4', ['1', '2']), selection: { ...selected, selectionRevision: '4' } };
  emit(current);
  await flushPromises();
  expect(wrapper.get('.region-boundary').attributes('style')).toContain('width: 120px');
  const restored = viewport();
  await pointer(restored, 'pointerdown', 300, 300);
  await pointer(restored, 'pointermove', 350, 350);
  await wrapper
    .findAll('button')
    .find((button) => button.text() === '从磁盘重新加载')!
    .trigger('click');
  await pointer(restored, 'pointerup', 360, 360);
  expect(regionRequests).toHaveLength(1);
  expect(wrapper.find('.region-draft').exists()).toBe(false);
});

it('画布空闲 Esc 清空失败可重试，等待权威通知且始终保留区域工具', async () => {
  current = state('1', ['1', '2']);
  const region = { x: 100, y: 100, width: 200, height: 200 };
  current.selection = {
    ...current.selection,
    region,
    scope: {
      snapshotId: '1',
      documentId: '1',
      documentRevision: '1',
      documentName: 'design-1.psd',
      targetCount: 1,
      contentCount: 1,
      textLayerCount: 0,
    },
  };
  let attempts = 0;
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation((command, args, options) => {
    if (command === 'selection_request') {
      const { request } = args as { request: SelectionRequest };
      if (request.operation.kind === 'clear') {
        attempts++;
        expect(request.operation).toEqual({
          kind: 'clear',
          documentId: '1',
          documentRevision: '1',
          expectedRevision: '1',
        });
        return attempts === 1
          ? Promise.reject(new Error('暂时无法清空'))
          : Promise.resolve({
              kind: 'committed',
              sessionId: current.selection.sessionId,
              selectionRevision: '2',
              snapshotId: null,
            });
      }
    }
    return base(command, args, options);
  });
  wrapper = mount(WorkspaceView);
  await flushPromises();
  await wrapper.get('[aria-label="区域选择"]').trigger('click');
  await wrapper.get('.task-name input').trigger('keydown', { key: 'Escape' });
  expect(attempts).toBe(0);
  const viewport = wrapper.get('.preview-viewport');
  await fileMenu();
  await wrapper.get('[aria-label="打开 PSD"]').trigger('keydown', { key: 'Escape' });
  expect(attempts).toBe(0);
  expect(wrapper.find('[role="menu"]').exists()).toBe(false);
  await viewport.trigger('keydown', { key: 'Escape' });
  await flushPromises();
  expect(wrapper.text()).toContain('暂时无法清空');
  expect(wrapper.find('.region-boundary').exists()).toBe(true);
  expect(wrapper.get('[aria-label="区域选择"]').attributes('aria-pressed')).toBe('true');
  await viewport.trigger('keydown', { key: 'Escape' });
  await flushPromises();
  expect(wrapper.find('.region-boundary').exists()).toBe(true);
  await viewport.trigger('keydown', { key: 'Escape' });
  expect(attempts).toBe(2);
  current = state('2', ['1', '2']);
  emit(current);
  await flushPromises();
  expect(wrapper.find('.region-boundary').exists()).toBe(false);
  expect(wrapper.text()).toContain('尚未选择范围');
  expect(wrapper.get('[aria-label="区域选择"]').attributes('aria-pressed')).toBe('true');
  await viewport.trigger('keydown', { key: 'Escape' });
  expect(attempts).toBe(2);
});

it('图层勾选经核心确认后可固定任务，关闭文档仍可查看并释放任务', async () => {
  current = state('1', ['1']);
  const sessionId = current.selection.sessionId;
  const scope = {
    snapshotId: '1',
    documentId: '1',
    documentRevision: '1',
    documentName: 'design-1.psd',
    targetCount: 1,
    contentCount: 1,
    textLayerCount: 0,
  };
  const details: LayerDetails = {
    layer: {
      id: 0,
      parentId: null,
      name: '目标图层',
      nameTruncated: false,
      kind: 'bitmap',
      bounds: { x: 0, y: 0, width: 10, height: 10 },
      visible: true,
      effectiveVisible: true,
      opacity: 255,
    },
    export: { status: 'supported', reason: 'fixture' },
    exportBlockers: [],
    diagnostics: [],
    diagnosticsTruncated: false,
    text: null,
  };
  let tasks: TaskDto[] = [];
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args, options) => {
    if (command === 'read_layers')
      return {
        documentId: '1',
        revision: '1',
        result: {
          kind: 'list',
          page: { offset: 0, total: 1, nextOffset: null, layers: [details.layer] },
        },
      };
    if (command !== 'selection_request') return base(command, args, options);
    const { operation } = (args as { request: SelectionRequest }).request;
    switch (operation.kind) {
      case 'tasks':
        return {
          kind: 'tasks',
          page: { sessionId, tasks, total: tasks.length, capacity: 128, nextAfter: null },
        };
      case 'layers':
        current = {
          ...state('2', ['1']),
          selection: {
            ...current.selection,
            selectionRevision: '2',
            scope,
            layerIds: operation.layerIds,
          },
        };
        return { kind: 'committed', sessionId, selectionRevision: '2', snapshotId: '1' };
      case 'createTask':
        tasks = [{ id: '1', name: operation.name, status: 'active', scope }];
        return { kind: 'task', sessionId, task: tasks[0] };
      case 'clear':
        expect(operation).toEqual({
          kind: 'clear',
          documentId: '1',
          documentRevision: '1',
          expectedRevision: '2',
        });
        current = state('3', ['1']);
        return { kind: 'committed', sessionId, selectionRevision: '3', snapshotId: null };
      case 'releaseTask':
        tasks = tasks.map((task) => ({ ...task, status: 'released' }));
        return { kind: 'task', sessionId, task: tasks[0] };
      case 'content':
        return {
          kind: 'content',
          target: operation.target,
          page: {
            sessionId,
            snapshotId: '1',
            documentId: '1',
            documentRevision: '1',
            layerCount: 1,
            textLayerCount: 0,
            layers: [{ stackIndex: 0, role: 'target', intersection: null, details }],
            warnings: [],
            truncated: false,
            nextCursor: null,
          },
        };
      default:
        throw new Error('未预期的选择操作');
    }
  });
  wrapper = mount(WorkspaceView);
  await flushPromises();
  const button = (text: string) => wrapper!.findAll('button').find((b) => b.text() === text)!;
  expect(button('固定任务').attributes('disabled')).toBeDefined();
  await wrapper.get('input[type=checkbox]').setValue(true);
  await flushPromises();
  expect(wrapper.text()).toContain('包含 1 个图层');
  await button('固定任务').trigger('click');
  await flushPromises();
  expect(wrapper.get('[aria-label=任务引用]').element).toBeInstanceOf(HTMLTextAreaElement);
  expect(wrapper.get<HTMLTextAreaElement>('[aria-label=任务引用]').element.value).toContain(
    sessionId,
  );
  await wrapper.get('[aria-label="区域选择"]').trigger('click');
  await wrapper.get('[aria-label="平移画布"]').trigger('click');
  await flushPromises();
  expect(wrapper.get('[aria-label="平移画布"]').attributes('aria-pressed')).toBe('true');
  expect(wrapper.text()).toContain('尚未选择范围');
  expect(tasks[0]?.status).toBe('active');
  current = state('4');
  emit(current);
  await flushPromises();
  await button('查看任务内容').trigger('click');
  await flushPromises();
  expect(wrapper.get('[aria-label=固定范围内容]').text()).toContain('目标图层');
  await button('释放任务').trigger('click');
  await flushPromises();
  expect(button('查看任务内容').attributes('disabled')).toBeDefined();
  expect(wrapper.find('[aria-label=固定范围内容]').exists()).toBe(false);
});

it('停靠面板独立切换，保留搜索、任务草稿及无文档时的任务入口', async () => {
  current = state('1', ['1']);
  wrapper = mount(WorkspaceView, { attachTo: document.body });
  await flushPromises();
  expect(wrapper.get('#inspector-panel-properties').isVisible()).toBe(true);
  expect(wrapper.get('#resources-panel-layers').isVisible()).toBe(true);
  expect(wrapper.get('#resources-panel-tasks').isVisible()).toBe(false);
  await wrapper.get('[aria-label="搜索图层"]').setValue('标题');
  await wrapper.get('#inspector-tab-document').trigger('click');
  await wrapper.get('#resources-tab-tasks').trigger('click');
  expect(wrapper.get('#inspector-panel-document').isVisible()).toBe(true);
  expect(wrapper.get('#resources-panel-tasks').isVisible()).toBe(true);
  await wrapper.get('.task-name input').setValue('首页任务');
  await wrapper.get('#resources-tab-layers').trigger('click');
  expect(wrapper.get<HTMLInputElement>('[aria-label="搜索图层"]').element.value).toBe('标题');
  await wrapper.findAll('.tool-options button')[0]!.trigger('click');
  expect(wrapper.get<HTMLInputElement>('.task-name input').element.value).toBe('首页任务');
  const callsBeforeReset = vi.mocked(invoke).mock.calls.length;
  await wrapper.get('[aria-label="窗口"]').trigger('click');
  await wrapper.get('[aria-label="重置面板布局"]').trigger('click');
  await flushPromises();
  expect(vi.mocked(invoke).mock.calls).toHaveLength(callsBeforeReset);
  expect(wrapper.get<HTMLInputElement>('.task-name input').element.value).toBe('首页任务');
  expect(wrapper.get<HTMLInputElement>('[aria-label="搜索图层"]').element.value).toBe('标题');
  current = state('2');
  emit(current);
  await flushPromises();
  expect(wrapper.get('#resources-panel-tasks').isVisible()).toBe(true);
  expect(wrapper.get('.task-name input').isVisible()).toBe(true);
  expect(wrapper.get('[aria-label="区域选择"]').attributes('disabled')).toBeDefined();
});

it('工具快捷键经核心清空确认才切换，错误在图层面板下也可见；输入中不抢快捷键', async () => {
  current = state('1', ['1']);
  current.selection = {
    ...current.selection,
    region: { x: 0, y: 0, width: 100, height: 100 },
    scope: {
      snapshotId: '1',
      documentId: '1',
      documentRevision: '1',
      documentName: 'design-1.psd',
      targetCount: 1,
      contentCount: 1,
      textLayerCount: 0,
    },
  };
  const base = vi.mocked(invoke).getMockImplementation()!;
  let attempts = 0;
  vi.mocked(invoke).mockImplementation((command, args, options) => {
    if (
      command === 'selection_request' &&
      (args as { request: SelectionRequest }).request.operation.kind === 'clear'
    ) {
      attempts++;
      return attempts === 1
        ? Promise.reject(new Error('清空失败'))
        : Promise.resolve({
            kind: 'committed',
            sessionId: current.selection.sessionId,
            selectionRevision: '2',
            snapshotId: null,
          });
    }
    return base(command, args, options);
  });
  wrapper = mount(WorkspaceView, { attachTo: document.body });
  await flushPromises();
  const key = (value: string, ctrlKey = false) =>
    window.dispatchEvent(new KeyboardEvent('keydown', { key: value, ctrlKey, cancelable: true }));
  key('m');
  await flushPromises();
  expect(wrapper.get('[aria-label="区域选择"]').attributes('aria-pressed')).toBe('true');
  await wrapper.get('[aria-label="搜索图层"]').trigger('keydown', { key: 'v' });
  expect(attempts).toBe(0);
  key('v');
  await flushPromises();
  expect(wrapper.get('.selection-feedback [role=alert]').isVisible()).toBe(true);
  expect(wrapper.get('[aria-label="区域选择"]').attributes('aria-pressed')).toBe('true');
  key('v');
  await flushPromises();
  key('v');
  key('m');
  await flushPromises();
  expect(attempts).toBe(2);
  expect(wrapper.get('[aria-label="区域选择"]').attributes('aria-pressed')).toBe('true');
  current = state('2', ['1']);
  emit(current);
  await flushPromises();
  expect(wrapper.get('[aria-label="平移画布"]').attributes('aria-pressed')).toBe('true');
  key('1', true);
  await flushPromises();
  expect(wrapper.get('[aria-label="当前缩放"]').text()).toBe('100%');
  key('0', true);
  await flushPromises();
  expect(wrapper.get('[aria-label="当前缩放"]').text()).not.toBe('100%');
  key('o', true);
  await flushPromises();
  expect(invoke).toHaveBeenCalledWith('choose_psd_files', { request: { protocolVersion: 1 } });
  const before = vi.mocked(invoke).mock.calls.length;
  wrapper.unmount();
  wrapper = undefined;
  key('o', true);
  expect(vi.mocked(invoke).mock.calls).toHaveLength(before);
});

it('菜单缩放与面板切换复用现有视图，文件关闭调用核心而不释放固定任务', async () => {
  current = state('1', ['1']);
  wrapper = mount(WorkspaceView, { attachTo: document.body });
  await flushPromises();
  await wrapper.get('[aria-label="视图"]').trigger('click');
  await wrapper.get('[aria-label="实际像素（100%）"]').trigger('click');
  expect(wrapper.get('[aria-label="当前缩放"]').text()).toBe('100%');
  await wrapper.get('[aria-label="视图"]').trigger('click');
  await wrapper.get('.menu-popup [aria-label="适应窗口"]').trigger('click');
  expect(wrapper.get('[aria-label="当前缩放"]').text()).not.toBe('100%');
  await wrapper.get('[aria-label="窗口"]').trigger('click');
  await wrapper.get('.menu-popup [aria-label="任务"]').trigger('click');
  expect(wrapper.get('#resources-panel-tasks').isVisible()).toBe(true);
  await wrapper.get('[aria-label="窗口"]').trigger('click');
  expect(wrapper.get('.menu-popup [aria-label="任务"]').attributes('aria-checked')).toBe('true');
  await wrapper.get('.menu-popup [aria-label="文档信息"]').trigger('click');
  expect(wrapper.get('#inspector-panel-document').isVisible()).toBe(true);
  await fileMenu();
  await wrapper.get('[aria-label="重新载入"]').trigger('click');
  expect(invoke).toHaveBeenCalledWith('workspace_action', {
    request: { protocolVersion: 1, action: { kind: 'reload', documentId: '1' } },
  });
  await flushPromises();
  await fileMenu();
  await wrapper.get('[aria-label="关闭当前文档"]').trigger('click');
  expect(invoke).toHaveBeenCalledWith('workspace_action', {
    request: { protocolVersion: 1, action: { kind: 'close', documentId: '1' } },
  });
  current = state('2');
  emit(current);
  await flushPromises();
  await fileMenu();
  expect(wrapper.get('[aria-label="重新载入"]').attributes('disabled')).toBeDefined();
  expect(wrapper.get('[aria-label="关闭当前文档"]').attributes('disabled')).toBeDefined();
  expect(
    vi
      .mocked(invoke)
      .mock.calls.filter((call) => call[0] === 'selection_request')
      .map((call) => (call[1] as { request: SelectionRequest }).request.operation.kind),
  ).toEqual(['tasks']);
});

describe('只读桌面工作区', () => {
  it('先订阅再读取状态，文件选择只转发到核心', async () => {
    wrapper = mount(WorkspaceView);
    await flushPromises();
    expect(wrapper.text()).toContain('工作区已连接');
    expect(wrapper.text()).toContain('v0.1.0');
    await (await fileMenu()).trigger('click');
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
    await (await fileMenu()).trigger('click');
    await flushPromises();
    expect(actions()).toHaveLength(before);
    expect(wrapper.findAll('.document-tab')).toHaveLength(1);
    expect(wrapper.get('img').attributes('style')).toBe(transform);
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect((await fileMenu()).attributes('disabled')).toBeUndefined();
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
    await (await fileMenu()).trigger('click');
    await flushPromises();
    expect(actions()).toEqual([{ kind: 'open', path: 'C:/first.psd' }]);
    expect((await fileMenu()).attributes('disabled')).toBeDefined();
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
    expect((await fileMenu()).attributes('disabled')).toBeUndefined();
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
    expect(wrapper.get('.document-details .document-name').text()).toContain('design-1.psd');
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
    expect(wrapper.get('.document-details .document-name').text()).toContain('design-1.psd');
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
