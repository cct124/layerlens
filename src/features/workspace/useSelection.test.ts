import { effectScope, shallowRef } from 'vue';
import { flushPromises } from '@vue/test-utils';
import { beforeEach, expect, it, vi } from 'vitest';
import { selectionRequest } from '../../shared/api/selection';
import type {
  ScopeDto,
  SelectionReply,
  SelectionSummaryDto,
  TaskDto,
} from '../../shared/api/generated';
import { useSelection } from './useSelection';

vi.mock('../../shared/api/selection', () => ({ selectionRequest: vi.fn() }));
const sessionId = 'a'.repeat(32);
const scope: ScopeDto = {
  snapshotId: '1',
  documentId: '1',
  documentRevision: '1',
  documentName: 'A.psd',
  targetCount: 1,
  contentCount: 1,
  textLayerCount: 0,
};
const task: TaskDto = { id: '1', name: 'task', status: 'active', scope };
const selected = (): SelectionSummaryDto => ({
  sessionId,
  selectionRevision: '2',
  documentId: '1',
  documentRevision: '1',
  scope,
  layerIds: [0],
  region: null,
});
const emptyTasks = (): SelectionReply => ({
  kind: 'tasks',
  page: { sessionId, tasks: [], nextAfter: null, total: 0, capacity: 128 },
});
function setup() {
  const state = shallowRef<SelectionSummaryDto | null>(selected());
  const context = effectScope();
  const sync = vi.fn(async () => {});
  const model = context.run(() => useSelection(state, sync))!;
  return { state, model, sync, stop: () => context.stop() };
}
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { resolve, reject, promise };
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(selectionRequest).mockResolvedValue(emptyTasks());
});

it('区域提交使用原文档与版本，回执不提前替换摘要；拒绝后保留原选择', async () => {
  const { state, model, stop } = setup();
  await flushPromises();
  const bounds = { x: 5.5, y: 10, width: 100, height: 80 };
  vi.mocked(selectionRequest).mockRejectedValueOnce(new Error('范围提交失败'));
  await model.selectRegion(bounds);
  expect(state.value).toEqual(selected());
  expect(model.error).toBe('范围提交失败');
  vi.mocked(selectionRequest).mockResolvedValueOnce({
    kind: 'committed',
    sessionId,
    selectionRevision: '3',
    snapshotId: '2',
  });
  await model.selectRegion(bounds);
  expect(selectionRequest).toHaveBeenLastCalledWith(sessionId, {
    kind: 'region',
    documentId: '1',
    documentRevision: '1',
    expectedRevision: '2',
    bounds,
  });
  expect(model.busy).toBe(true);
  expect(state.value?.region).toBeNull();
  state.value = {
    ...selected(),
    selectionRevision: '3',
    layerIds: [],
    region: bounds,
    scope: { ...scope, snapshotId: '2' },
  };
  expect(model.busy).toBe(false);
  stop();
});

it.each(['success', 'failure'])('换文档后迟到的区域 %s 不写回提示或确认版本', async (result) => {
  const { state, model, sync, stop } = setup();
  await flushPromises();
  const delayed = deferred<SelectionReply>();
  vi.mocked(selectionRequest).mockReturnValueOnce(delayed.promise);
  const pending = model.selectRegion({ x: 0, y: 0, width: 100, height: 80 });
  state.value = {
    ...selected(),
    documentId: '2',
    documentRevision: '2',
    scope: null,
    layerIds: [],
    selectionRevision: '4',
  };
  if (result === 'failure') delayed.reject(new Error('旧文档已关闭'));
  else delayed.resolve({ kind: 'committed', sessionId, selectionRevision: '3', snapshotId: '2' });
  await pending;
  expect(model.error).toBeNull();
  expect(model.notice).toBeNull();
  expect(model.busy).toBe(false);
  expect(sync).not.toHaveBeenCalled();
  stop();
});

it('失败保留原范围；成功回执不能先于核心摘要假装已勾选', async () => {
  const { state, model, stop } = setup();
  await flushPromises();
  vi.mocked(selectionRequest).mockRejectedValueOnce({
    code: 'STALE_SELECTION',
    message: '范围已变化',
  });
  await model.toggleLayer(1);
  expect(state.value?.layerIds).toEqual([0]);
  expect(model.error).toContain('范围已变化');
  vi.mocked(selectionRequest).mockResolvedValueOnce({
    kind: 'committed',
    sessionId,
    selectionRevision: '3',
    snapshotId: '2',
  });
  await model.toggleLayer(1);
  expect(model.busy).toBe(true);
  expect(state.value?.layerIds).toEqual([0]);
  state.value = {
    ...selected(),
    selectionRevision: '3',
    scope: { ...scope, snapshotId: '2' },
    layerIds: [0, 1],
  };
  expect(model.busy).toBe(false);
  stop();
});

it('不确定的创建失败重试使用同一请求 ID；已确认后新的意图使用新 ID', async () => {
  const { model, stop } = setup();
  await flushPromises();
  vi.mocked(selectionRequest).mockRejectedValueOnce(new Error('连接中断'));
  await model.createTask('task');
  const first = vi.mocked(selectionRequest).mock.calls.at(-1)![1];
  vi.mocked(selectionRequest).mockResolvedValueOnce({ kind: 'task', sessionId, task });
  await model.createTask('task');
  const creates = vi
    .mocked(selectionRequest)
    .mock.calls.filter(([, operation]) => operation.kind === 'createTask');
  expect(creates[1]![1]).toEqual(first);
  expect(model.tasks).toEqual([task]);
  expect(JSON.parse(model.reference!)).toEqual({ sessionId, taskId: '1' });
  vi.mocked(selectionRequest).mockResolvedValueOnce({
    kind: 'task',
    sessionId,
    task: { ...task, id: '2' },
  });
  await model.createTask('task');
  const last = vi
    .mocked(selectionRequest)
    .mock.calls.filter(([, op]) => op.kind === 'createTask')
    .at(-1)![1];
  expect(last).not.toEqual(first);
  stop();
});

it('旧范围迟到内容丢弃，任务内容跨改选和关闭保留固定关联', async () => {
  const { state, model, stop } = setup();
  await flushPromises();
  const delayed = deferred<SelectionReply>();
  vi.mocked(selectionRequest).mockReturnValueOnce(delayed.promise);
  model.viewScope();
  state.value = { ...selected(), scope: { ...scope, snapshotId: '2' }, selectionRevision: '3' };
  await flushPromises();
  delayed.resolve({
    kind: 'content',
    target: { kind: 'snapshot', snapshotId: '1' },
    page: {
      sessionId,
      snapshotId: '1',
      documentId: '1',
      documentRevision: '1',
      layers: [],
      layerCount: 1,
      textLayerCount: 0,
      warnings: [],
      truncated: false,
      nextCursor: null,
    },
  });
  await flushPromises();
  expect(model.content).toBeNull();
  vi.mocked(selectionRequest).mockResolvedValueOnce({
    kind: 'content',
    target: { kind: 'task', taskId: '1' },
    page: {
      sessionId,
      snapshotId: '1',
      documentId: '1',
      documentRevision: '1',
      layers: [],
      layerCount: 1,
      textLayerCount: 0,
      warnings: [],
      truncated: false,
      nextCursor: null,
    },
  });
  model.viewScope(task);
  await flushPromises();
  state.value = {
    ...selected(),
    selectionRevision: '4',
    documentId: null,
    documentRevision: null,
    scope: null,
    layerIds: [],
  };
  await flushPromises();
  expect(model.content?.scope.documentName).toBe('A.psd');
  expect(model.content?.page?.snapshotId).toBe('1');
  vi.mocked(selectionRequest).mockResolvedValueOnce({
    kind: 'task',
    sessionId,
    task: { ...task, status: 'released' },
  });
  await model.releaseTask(task);
  expect(model.content).toBeNull();
  expect(model.tasks[0]?.status).toBe('released');
  stop();
});

it('旧范围读取失败不覆盖新范围提示，剪贴板失败提供手动复制内容', async () => {
  const { state, model, stop } = setup();
  await flushPromises();
  const old = deferred<SelectionReply>();
  vi.mocked(selectionRequest).mockReturnValueOnce(old.promise);
  model.viewScope();
  state.value = { ...selected(), scope: null, layerIds: [], selectionRevision: '3' };
  await flushPromises();
  old.reject(new Error('旧范围读取失败'));
  await flushPromises();
  expect(model.error).toBeNull();
  const writeText = vi.fn().mockRejectedValue(new Error('权限不足'));
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  try {
    await model.copyTask(task);
    expect(model.error).toContain('请从引用框手动复制');
    expect(model.reference).toBe(JSON.stringify({ sessionId, taskId: '1' }));
  } finally {
    if (original) Object.defineProperty(navigator, 'clipboard', original);
    else Reflect.deleteProperty(navigator, 'clipboard');
    stop();
  }
});

it('换会话和卸载后，旧任务列表不能写回', async () => {
  const delayed = deferred<SelectionReply>();
  vi.mocked(selectionRequest).mockReturnValueOnce(delayed.promise);
  const { state, model, stop } = setup();
  state.value = { ...selected(), sessionId: 'b'.repeat(32) };
  await flushPromises();
  vi.mocked(selectionRequest).mockResolvedValueOnce({
    kind: 'tasks',
    page: { sessionId: 'b'.repeat(32), tasks: [], nextAfter: null, total: 0, capacity: 128 },
  });
  delayed.resolve({
    kind: 'tasks',
    page: { sessionId, tasks: [task], nextAfter: null, total: 1, capacity: 128 },
  });
  await flushPromises();
  expect(model.tasks).toEqual([]);
  const disposed = deferred<SelectionReply>();
  vi.mocked(selectionRequest).mockReturnValueOnce(disposed.promise);
  void model.refreshTasks();
  stop();
  disposed.resolve({
    kind: 'tasks',
    page: { sessionId: 'b'.repeat(32), tasks: [task], nextAfter: null, total: 1, capacity: 128 },
  });
  await flushPromises();
  expect(model.tasks).toEqual([]);
});
