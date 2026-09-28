import { effectScope, shallowRef } from 'vue';
import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, expect, it, vi } from 'vitest';
import { cleanupRequest } from '../../shared/api/cleanup';
import type { CleanupPlanDto, CleanupReceiptDto, CleanupReply } from '../../shared/api/generated';
import { useCleanup } from './useCleanup';
import CleanupDialog from './CleanupDialog.vue';
import { mockNativeDialog } from '../../test/nativeDialog';
vi.mock('../../shared/api/cleanup', async (original) => ({
  ...(await original<typeof import('../../shared/api/cleanup')>()),
  cleanupRequest: vi.fn(),
}));
const sessionId = 'a'.repeat(32);
const plan: CleanupPlanDto = {
  planId: '1',
  documentId: '2',
  documentName: 'A.psd',
  openRevision: '3',
  selectionId: null,
  liveRevisions: ['3'],
  tasks: [{ id: '33', name: '旧任务', documentRevision: '3' }],
  loadJobs: [],
  unresolvedLoadJobs: ['7'],
  previewJobs: [],
  expiresInSeconds: 300,
};
const receipt: CleanupReceiptDto = {
  planId: '1',
  documentId: '2',
  revision: '1',
  invalidatedTaskIds: ['33'],
};
const target = { kind: 'document', documentId: '2', documentRevision: '3' } as const;
function setup() {
  const context = effectScope();
  const session = shallowRef<string | null>(sessionId);
  const apply = vi.fn(),
    sync = vi.fn(async () => {});
  const model = context.run(() => useCleanup(session, apply, sync))!;
  return { model, apply, sync, session, stop: () => context.stop() };
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
  vi.mocked(cleanupRequest).mockImplementation(
    async (sessionId, operation): Promise<CleanupReply> => {
      if (operation.kind === 'prepare') return { kind: 'prepared', sessionId, plan };
      if (operation.kind === 'confirm') return { kind: 'committed', sessionId, receipt };
      return { kind: 'cancelled', sessionId, planId: operation.planId };
    },
  );
});
it('预检与取消不应用失效，只有明确确认后更新投影并同步', async () => {
  const { model, apply, sync, stop } = setup();
  await model.begin(target, '2');
  expect(model.state?.kind).toBe('ready');
  expect(apply).not.toHaveBeenCalled();
  await model.cancel();
  expect(model.opened).toBe(false);
  expect(sync).not.toHaveBeenCalled();
  await model.begin(target, '2');
  await model.confirm();
  expect(model.state?.kind).toBe('complete');
  expect(apply).toHaveBeenCalledExactlyOnceWith(receipt, sessionId);
  expect(sync).toHaveBeenCalledTimes(1);
  stop();
});
it('影响变化只显示重新预检，不自动执行新计划；过期同样处理', async () => {
  const { model, apply, stop } = setup();
  await model.begin(target, '2');
  vi.mocked(cleanupRequest).mockRejectedValueOnce({
    code: 'STALE_CLEANUP_PLAN',
    message: '影响变化',
  });
  await model.confirm();
  expect(model.state?.kind).toBe('stale');
  expect(cleanupRequest).toHaveBeenCalledTimes(2);
  await model.confirm();
  expect(cleanupRequest).toHaveBeenCalledTimes(2);
  await model.recheck();
  expect(model.state?.kind).toBe('ready');
  expect(apply).not.toHaveBeenCalled();
  vi.mocked(cleanupRequest).mockRejectedValueOnce({
    code: 'CLEANUP_PLAN_EXPIRED',
    message: '过期',
  });
  await model.confirm();
  expect(model.state?.kind).toBe('stale');
  stop();
});
it('响应丢失仅重试原计划，取消时返回已执行回执不能伪装为取消', async () => {
  const { model, apply, stop } = setup();
  await model.begin(target, '2');
  vi.mocked(cleanupRequest).mockRejectedValueOnce(new Error('断连'));
  await model.confirm();
  expect(model.state?.kind).toBe('uncertain');
  await model.recheck();
  expect(cleanupRequest).toHaveBeenCalledTimes(2);
  await model.confirm();
  expect(cleanupRequest).toHaveBeenLastCalledWith(sessionId, { kind: 'confirm', planId: '1' });
  expect(apply).toHaveBeenCalledTimes(1);
  await model.cancel();
  await model.begin(target, '2');
  vi.mocked(cleanupRequest).mockResolvedValueOnce({ kind: 'committed', sessionId, receipt });
  await model.cancel();
  expect(model.state?.kind).toBe('complete');
  stop();
});
it('确认等待期间阻止双击；同步失败保留真实成功，不重复确认', async () => {
  const { model, sync, apply, stop } = setup();
  await model.begin(target, '2');
  const pending = deferred<CleanupReply>();
  vi.mocked(cleanupRequest).mockReturnValueOnce(pending.promise);
  sync.mockRejectedValueOnce(new Error('同步失败'));
  const first = model.confirm();
  await model.confirm();
  await model.cancel();
  expect(cleanupRequest).toHaveBeenCalledTimes(2);
  pending.resolve({ kind: 'committed', sessionId, receipt });
  await first;
  expect(model.state?.kind).toBe('complete');
  expect(model.error).toContain('清理已经生效');
  await model.synchronize();
  expect(model.error).toBeNull();
  expect(apply).toHaveBeenCalledTimes(1);
  stop();
});
it('换会话或卸载后的迟到预检不打开窗口，并回收服务端计划', async () => {
  for (const dispose of [false, true]) {
    const { model, session, apply, stop } = setup();
    const pending = deferred<CleanupReply>();
    vi.mocked(cleanupRequest).mockReturnValueOnce(pending.promise);
    const begin = model.begin(target, '2');
    if (dispose) stop();
    else session.value = 'b'.repeat(32);
    pending.resolve({ kind: 'prepared', sessionId, plan });
    await begin;
    expect(model.opened).toBe(false);
    expect(apply).not.toHaveBeenCalled();
    expect(cleanupRequest).toHaveBeenLastCalledWith(sessionId, { kind: 'cancel', planId: '1' });
    stop();
  }
});
it('确认对话框列明任务和潜在别名；Esc 不传播到画布且取消走后端', async () => {
  const restore = mockNativeDialog();
  const { model, stop } = setup();
  const wrapper = mount(CleanupDialog, { props: { model }, attachTo: document.body });
  const listener = vi.fn();
  window.addEventListener('keydown', listener);
  try {
    await model.begin(target, '2');
    await flushPromises();
    expect(wrapper.get('dialog').attributes('open')).toBeDefined();
    expect(wrapper.text()).toContain('旧任务');
    expect(wrapper.text()).toContain('潜在路径别名作业：7');
    await wrapper.get('dialog').trigger('keydown', { key: 'Escape' });
    expect(listener).not.toHaveBeenCalled();
    await wrapper.get('dialog').trigger('cancel');
    await flushPromises();
    expect(model.opened).toBe(false);
    expect(wrapper.get('dialog').attributes('open')).toBeUndefined();
  } finally {
    window.removeEventListener('keydown', listener);
    wrapper.unmount();
    stop();
    restore();
  }
});
