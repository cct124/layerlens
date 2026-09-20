import { effectScope, shallowRef } from 'vue';
import { flushPromises } from '@vue/test-utils';
import { beforeEach, expect, it, vi } from 'vitest';
import { selectionRequest } from '../../shared/api/selection';
import type { SelectionReply, SelectionSummaryDto } from '../../shared/api/generated';
import { useSelection } from './useSelection';
import { useCanvasTool } from './useCanvasTool';

vi.mock('../../shared/api/selection', () => ({ selectionRequest: vi.fn() }));
const sessionId = 'a'.repeat(32);
const selected = (): SelectionSummaryDto => ({
  sessionId,
  selectionRevision: '2',
  documentId: '1',
  documentRevision: '1',
  layerIds: [],
  region: { x: 20, y: 20, width: 100, height: 80 },
  scope: {
    snapshotId: '1',
    documentId: '1',
    documentRevision: '1',
    documentName: 'A.psd',
    targetCount: 1,
    contentCount: 1,
    textLayerCount: 0,
  },
});
const cleared = (): SelectionSummaryDto => ({
  ...selected(),
  selectionRevision: '3',
  scope: null,
  region: null,
});
const receipt = (): SelectionReply => ({
  kind: 'committed',
  sessionId,
  selectionRevision: '3',
  snapshotId: null,
});
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(selectionRequest).mockResolvedValue({
    kind: 'tasks',
    page: { sessionId, tasks: [], total: 0, capacity: 128, nextAfter: null },
  });
});
async function setup() {
  const state = shallowRef<SelectionSummaryDto | null>(selected());
  const context = effectScope(),
    cancelDraft = vi.fn();
  const selection = context.run(() => useSelection(state, async () => {}))!;
  const model = context.run(() => useCanvasTool(selection, cancelDraft))!;
  await flushPromises();
  await model.chooseTool('region');
  cancelDraft.mockClear();
  return { state, selection, model, cancelDraft, stop: () => context.stop() };
}

it.each(['receipt-first', 'summary-first'])(
  '%s：同时有核心回执与空摘要后才进入移动工具，重复点击不重复清空',
  async (order) => {
    const { state, model, cancelDraft, stop } = await setup();
    let resolve!: (reply: SelectionReply) => void;
    vi.mocked(selectionRequest).mockReturnValueOnce(
      new Promise<SelectionReply>((done) => {
        resolve = done;
      }),
    );
    const pending = model.chooseTool('pan');
    await model.chooseTool('pan');
    expect(cancelDraft).toHaveBeenCalled();
    expect(
      vi.mocked(selectionRequest).mock.calls.filter(([, op]) => op.kind === 'clear'),
    ).toHaveLength(1);
    expect(selectionRequest).toHaveBeenLastCalledWith(sessionId, {
      kind: 'clear',
      documentId: '1',
      documentRevision: '1',
      expectedRevision: '2',
    });
    if (order === 'summary-first') {
      state.value = cleared();
      expect(model.tool.value).toBe('region');
      resolve(receipt());
      await pending;
    } else {
      resolve(receipt());
      await pending;
      expect(model.tool.value).toBe('region');
      state.value = cleared();
    }
    expect(model.tool.value).toBe('pan');
    expect(model.pending.value).toBeNull();
    await model.chooseTool('region');
    expect(state.value?.scope).toBeNull();
    stop();
  },
);

it('清空失败保留工具与原范围并显示错误，随后可以重试；固定任务内容保留', async () => {
  const { state, selection, model, stop } = await setup();
  const scope = selected().scope!;
  selection.tasks = [{ id: '1', name: '固定任务', status: 'active', scope }];
  selection.content = { sessionId, target: { kind: 'task', taskId: '1' }, scope, page: null };
  vi.mocked(selectionRequest).mockRejectedValueOnce(new Error('清空失败'));
  await model.chooseTool('pan');
  expect(state.value).toEqual(selected());
  expect(model.tool.value).toBe('region');
  expect(selection.error).toBe('清空失败');
  expect(model.pending.value).toBeNull();
  vi.mocked(selectionRequest).mockResolvedValueOnce(receipt());
  await model.chooseTool('pan');
  state.value = cleared();
  await flushPromises();
  expect(model.tool.value).toBe('pan');
  expect(selection.tasks[0]?.status).toBe('active');
  expect(selection.content?.scope).toEqual(scope);
  stop();
});

it.each(['document', 'session', 'revision', 'reselect', 'tool', 'dispose'])(
  '%s 变化使迟到清空回执不能切换新上下文的工具',
  async (change) => {
    const { state, model, stop } = await setup();
    let resolve!: (reply: SelectionReply) => void;
    vi.mocked(selectionRequest).mockReturnValueOnce(
      new Promise<SelectionReply>((done) => {
        resolve = done;
      }),
    );
    const pending = model.chooseTool('pan');
    if (change === 'document')
      state.value = {
        ...cleared(),
        documentId: '2',
        documentRevision: '2',
        selectionRevision: '4',
      };
    else if (change === 'session') state.value = { ...cleared(), sessionId: 'b'.repeat(32) };
    else if (change === 'revision') state.value = { ...cleared(), documentRevision: '2' };
    else if (change === 'reselect') state.value = { ...selected(), selectionRevision: '4' };
    else if (change === 'tool') {
      await model.chooseTool('region');
      state.value = cleared();
    } else stop();
    resolve(receipt());
    await pending;
    expect(model.tool.value).toBe('region');
    expect(model.pending.value).toBeNull();
    stop();
  },
);

it('空范围切到移动工具不发核心请求，忙碌时不悄悄切换', async () => {
  const { state, selection, model, stop } = await setup();
  selection.running = true;
  await model.chooseTool('pan');
  expect(model.tool.value).toBe('region');
  selection.running = false;
  state.value = cleared();
  await model.chooseTool('pan');
  expect(model.tool.value).toBe('pan');
  expect(vi.mocked(selectionRequest).mock.calls.some(([, op]) => op.kind === 'clear')).toBe(false);
  stop();
});
