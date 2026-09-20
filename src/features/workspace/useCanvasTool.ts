import { onScopeDispose, ref, shallowRef, watchEffect } from 'vue';
import type { CanvasTool } from './regionDraft';
import type { useSelection } from './useSelection';

/** 工具意图属于界面状态；点击移动工具通过核心清空，回执与空摘要确认后才完成切换。 */
export function useCanvasTool(selection: ReturnType<typeof useSelection>, cancelDraft: () => void) {
  const tool = ref<CanvasTool>('pan');
  const pending = shallowRef<{
    sessionId: string;
    documentId: string;
    documentRevision: string;
    selectionRevision: string | null;
  } | null>(null);
  let disposed = false;
  watchEffect(
    () => {
      const intent = pending.value;
      if (!intent) return;
      const current = selection.summary;
      if (
        current?.sessionId !== intent.sessionId ||
        current.documentId !== intent.documentId ||
        current.documentRevision !== intent.documentRevision
      ) {
        pending.value = null;
        return;
      }
      if (intent.selectionRevision === null) return;
      const version = BigInt(current.selectionRevision),
        confirmed = BigInt(intent.selectionRevision);
      if (version < confirmed) return;
      if (version === confirmed && current.scope === null) tool.value = 'pan';
      // 后续改选或切换过标签时，旧清空回执不能再切换工具。
      pending.value = null;
    },
    { flush: 'sync' },
  );

  async function chooseTool(next: CanvasTool) {
    cancelDraft();
    if (next === 'region') {
      pending.value = null;
      tool.value = next;
      return;
    }
    if (selection.busy || pending.value || disposed) return;
    const current = selection.summary;
    if (!current?.scope) {
      tool.value = 'pan';
      return;
    }
    const intent = {
      sessionId: current.sessionId,
      documentId: current.scope.documentId,
      documentRevision: current.scope.documentRevision,
      selectionRevision: null,
    };
    pending.value = intent;
    const revision = await selection.clear();
    if (disposed || pending.value !== intent) return;
    pending.value = revision === null ? null : { ...intent, selectionRevision: revision };
  }
  onScopeDispose(() => {
    disposed = true;
    pending.value = null;
  });
  return { tool, pending, chooseTool };
}
