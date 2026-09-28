import { computed, onMounted, onScopeDispose, ref, shallowRef } from 'vue';
import type {
  CleanupReceiptDto,
  WorkspaceAction,
  WorkspaceSnapshot,
} from '../../shared/api/generated';
import { isDesktopAvailable } from '../../shared/api/desktop';
import {
  choosePsdFiles,
  subscribeWorkspace,
  workspaceAction,
  workspaceMessage,
} from '../../shared/api/workspace';
import { usePreview } from './usePreview';

/** 界面保存最后一份已校验快照和视图状态；所有文档操作由核心裁决。 */
export function useWorkspace() {
  const desktop = isDesktopAvailable();
  const snapshot = shallowRef<WorkspaceSnapshot | null>(null);
  const error = ref<string | null>(null);
  const connecting = ref(false);
  const picking = ref(false);
  const cleanupBarrier = shallowRef<{
    sessionId: string;
    revision: string;
    documents: string[];
  } | null>(null);
  const cleanupPending = computed(() => cleanupBarrier.value !== null);
  let disposed = false;
  let cleanupSyncError: string | null = null;
  let unsubscribe: (() => void) | undefined;
  const active = computed(
    () =>
      snapshot.value?.documents.find(
        (d) =>
          d.id === snapshot.value?.activeDocumentId &&
          !cleanupBarrier.value?.documents.includes(d.id),
      ) ?? null,
  );
  const preview = usePreview(
    computed(() => {
      const value = snapshot.value?.preview;
      return value && !cleanupBarrier.value?.documents.includes(value.documentId) ? value : null;
    }),
  );
  function accept(next: WorkspaceSnapshot) {
    if (!disposed && (!snapshot.value || BigInt(next.sequence) > BigInt(snapshot.value.sequence))) {
      snapshot.value = next;
      const barrier = cleanupBarrier.value;
      if (
        barrier &&
        (next.selection.sessionId !== barrier.sessionId ||
          BigInt(next.cleanup.revision) >= BigInt(barrier.revision))
      )
        cleanupBarrier.value = null;
    }
  }
  /** 核心回执已经生效时立即撤销目标图像／属性，绝不等待迟到的通知；不伪造新的文档列表。 */
  function applyCleanup(receipt: CleanupReceiptDto, sessionId: string) {
    if (
      snapshot.value?.selection.sessionId !== sessionId ||
      BigInt(snapshot.value.cleanup.revision) >= BigInt(receipt.revision)
    )
      return;
    const before = cleanupBarrier.value;
    cleanupBarrier.value = {
      sessionId,
      revision:
        before && BigInt(before.revision) > BigInt(receipt.revision)
          ? before.revision
          : receipt.revision,
      documents: [...new Set([...(before?.documents ?? []), receipt.documentId])],
    };
  }
  async function syncCleanup() {
    try {
      accept(await workspaceAction({ kind: 'snapshot' }));
      if (!disposed && cleanupBarrier.value)
        throw new Error('工作区尚未确认清理后的状态，请重试同步。');
      if (!disposed && cleanupSyncError !== null && error.value === cleanupSyncError)
        error.value = null;
      cleanupSyncError = null;
    } catch (cause: unknown) {
      if (!disposed) {
        cleanupSyncError = `清理已生效，工作区同步失败：${workspaceMessage(cause)}`;
        error.value = cleanupSyncError;
      }
      throw cause;
    }
  }
  async function act(action: WorkspaceAction) {
    try {
      accept(await workspaceAction(action));
    } catch (cause: unknown) {
      if (!disposed) error.value = workspaceMessage(cause);
    }
  }
  async function connect() {
    if (!desktop || disposed || connecting.value) return;
    connecting.value = true;
    error.value = null;
    try {
      // 先订阅再取快照，覆盖握手期间的完成事件；顺序号消除重复／乱序返回。
      if (!unsubscribe) {
        const stop = await subscribeWorkspace(accept, (cause) => {
          if (!disposed) error.value = cause.message;
        });
        if (disposed) {
          stop();
          return;
        }
        unsubscribe = stop;
      }
      accept(await workspaceAction({ kind: 'snapshot' }));
    } catch (cause: unknown) {
      if (!disposed) error.value = workspaceMessage(cause);
    } finally {
      if (!disposed) connecting.value = false;
    }
  }
  async function open() {
    if (picking.value) return;
    picking.value = true;
    error.value = null;
    try {
      const paths = await choosePsdFiles();
      for (const path of paths) {
        if (disposed) break;
        // 尊重核心有限队列；遇到拒绝停止本批并明确提示，不无限重试。
        try {
          accept(await workspaceAction({ kind: 'open', path }));
        } catch (cause: unknown) {
          throw new Error(
            `打开 ${path} 未被接受：${workspaceMessage(cause)} 本批后续文件尚未打开，请稍后重新选择。`,
            { cause },
          );
        }
      }
    } catch (cause: unknown) {
      if (!disposed) error.value = workspaceMessage(cause);
    } finally {
      if (!disposed) picking.value = false;
    }
  }
  function focus() {
    void connect();
  }
  onMounted(() => {
    void connect();
    window.addEventListener('focus', focus);
  });
  onScopeDispose(() => {
    disposed = true;
    unsubscribe?.();
    window.removeEventListener('focus', focus);
  });
  return {
    desktop,
    snapshot,
    active,
    error,
    connecting,
    picking,
    preview,
    act,
    connect,
    open,
    applyCleanup,
    syncCleanup,
    cleanupPending,
  };
}
