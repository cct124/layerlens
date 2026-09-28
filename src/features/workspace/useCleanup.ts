import { computed, onScopeDispose, reactive, ref, shallowRef, watch } from 'vue';
import type { Ref } from 'vue';
import { cleanupRequest } from '../../shared/api/cleanup';
import type { CleanupPlanDto, CleanupReceiptDto, CleanupTarget } from '../../shared/api/generated';
import { workspaceMessage } from '../../shared/api/workspace';
import { isWorkspaceError } from '../../shared/api/workspaceValidation';

interface Context {
  sessionId: string;
  target: CleanupTarget;
  documentId: string;
}
type State =
  | { kind: 'preparing' | 'failed'; context: Context }
  | {
      kind: 'ready' | 'confirming' | 'cancelling' | 'stale' | 'uncertain';
      context: Context;
      plan: CleanupPlanDto;
    }
  | { kind: 'complete'; context: Context; receipt: CleanupReceiptDto };

/** 只保管当前确认意图。未知执行结果只能重试原计划，不自动预检并扩大清理范围。 */
export function useCleanup(
  session: Readonly<Ref<string | null>>,
  apply: (receipt: CleanupReceiptDto, sessionId: string) => void,
  sync: () => Promise<void>,
) {
  const state = shallowRef<State | null>(null);
  const error = ref<string | null>(null);
  const syncing = ref(false);
  let disposed = false;
  const opened = computed(() => state.value !== null);
  const busy = computed(
    () =>
      syncing.value || ['preparing', 'confirming', 'cancelling'].includes(state.value?.kind ?? ''),
  );
  const plan = computed(() => (state.value && 'plan' in state.value ? state.value.plan : null));
  const live = (context: Context) =>
    !disposed && state.value?.context === context && session.value === context.sessionId;

  function retire(context: Context, planId: string) {
    // 页面卸载／换会话时尽力回收；断连由服务端 TTL 和退出兜底，不能吞掉失败。
    void cleanupRequest(context.sessionId, { kind: 'cancel', planId }).catch((cause: unknown) => {
      console.warn('cleanup_cancel_failed', { planId, message: workspaceMessage(cause) });
    });
  }
  async function prepare(context: Context) {
    state.value = { kind: 'preparing', context };
    error.value = null;
    try {
      const reply = await cleanupRequest(context.sessionId, {
        kind: 'prepare',
        target: context.target,
      });
      if (reply.kind !== 'prepared') throw new Error('预检未返回清理计划。');
      if (!live(context)) {
        retire(context, reply.plan.planId);
        return;
      }
      if (reply.plan.documentId !== context.documentId) {
        retire(context, reply.plan.planId);
        throw new Error('清理计划不属于选定文档。');
      }
      state.value = { kind: 'ready', context, plan: reply.plan };
    } catch (cause: unknown) {
      if (live(context)) {
        state.value = { kind: 'failed', context };
        error.value = workspaceMessage(cause);
      }
    }
  }
  async function begin(target: CleanupTarget, documentId: string) {
    if (opened.value || disposed || !session.value) return;
    await prepare({ sessionId: session.value, target, documentId });
  }
  async function recheck() {
    const current = state.value;
    if (!busy.value && current && (current.kind === 'stale' || current.kind === 'failed'))
      await prepare(current.context);
  }
  async function synchronize() {
    const current = state.value;
    if (syncing.value || current?.kind !== 'complete') return;
    syncing.value = true;
    error.value = null;
    try {
      await sync();
    } catch (cause: unknown) {
      if (live(current.context))
        error.value = `清理已经生效，界面同步失败，可重试同步：${workspaceMessage(cause)}`;
    } finally {
      syncing.value = false;
    }
  }
  async function completed(context: Context, receipt: CleanupReceiptDto) {
    if (receipt.documentId !== context.documentId)
      throw new Error('清理回执与目标文档不匹配，请核对原计划。');
    state.value = { kind: 'complete', context, receipt };
    apply(receipt, context.sessionId);
    await synchronize();
  }
  async function confirm() {
    const current = state.value;
    if (busy.value || !current || (current.kind !== 'ready' && current.kind !== 'uncertain'))
      return;
    state.value = { ...current, kind: 'confirming' };
    error.value = null;
    try {
      const reply = await cleanupRequest(current.context.sessionId, {
        kind: 'confirm',
        planId: current.plan.planId,
      });
      if (!live(current.context)) return;
      if (reply.kind !== 'committed') throw new Error('未取得清理执行回执。');
      await completed(current.context, reply.receipt);
    } catch (cause: unknown) {
      if (!live(current.context)) return;
      const stale =
        isWorkspaceError(cause) &&
        (cause.code === 'STALE_CLEANUP_PLAN' || cause.code === 'CLEANUP_PLAN_EXPIRED');
      state.value = { ...current, kind: stale ? 'stale' : 'uncertain' };
      error.value = stale
        ? `影响已变化或计划过期；请重新预检并再次确认。${workspaceMessage(cause)}`
        : `未取得确定结果；重试将使用同一计划，也可取消并核对结果。${workspaceMessage(cause)}`;
    }
  }
  async function cancel() {
    const current = state.value;
    if (busy.value || !current) return;
    if (!('plan' in current)) {
      state.value = null;
      error.value = null;
      return;
    }
    state.value = { ...current, kind: 'cancelling' };
    error.value = null;
    try {
      const reply = await cleanupRequest(current.context.sessionId, {
        kind: 'cancel',
        planId: current.plan.planId,
      });
      if (!live(current.context)) return;
      if (reply.kind === 'committed') await completed(current.context, reply.receipt);
      else if (reply.kind === 'cancelled') state.value = null;
      else throw new Error('未取得取消回执。');
    } catch (cause: unknown) {
      if (live(current.context)) {
        state.value = { ...current, kind: 'uncertain' };
        error.value = `取消尚未确认，可再次取消核对。${workspaceMessage(cause)}`;
      }
    }
  }
  function reset() {
    const current = state.value;
    if (current && 'plan' in current) retire(current.context, current.plan.planId);
    state.value = null;
    error.value = null;
  }
  watch(session, reset, { flush: 'sync' });
  onScopeDispose(() => {
    disposed = true;
    reset();
  });
  return reactive({
    state,
    error,
    opened,
    busy,
    syncing,
    plan,
    begin,
    recheck,
    confirm,
    cancel,
    synchronize,
  });
}
