import { onScopeDispose, ref, watch } from 'vue';
import type { Ref } from 'vue';
import type { WorkspacePreview } from '../../shared/api/generated';
import { readPreview, workspaceMessage } from '../../shared/api/workspace';

/** 串行读取 PNG，只持有当前 Blob URL；切换、关闭和销毁立即撤销旧图像。 */
export function usePreview(preview: Readonly<Ref<WorkspacePreview | null>>) {
  const url = ref<string | null>(null);
  const error = ref<string | null>(null);
  const loading = ref(false);
  let disposed = false;
  let running = false;
  let generation = 0;
  let wanted: { documentId: string; revision: string } | null = null;
  let attempted = -1;
  function release() {
    if (url.value) URL.revokeObjectURL(url.value);
    url.value = null;
  }
  async function pump() {
    if (disposed || running || !wanted || attempted === generation) return;
    running = true;
    loading.value = true;
    const target = wanted;
    const current = generation;
    attempted = current;
    try {
      const bytes = await readPreview(target.documentId, target.revision);
      if (!disposed && current === generation)
        url.value = URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
    } catch (cause: unknown) {
      if (!disposed && current === generation) error.value = workspaceMessage(cause);
    } finally {
      running = false;
      if (!disposed) {
        loading.value = false;
        void pump();
      }
    }
  }
  watch(
    () => {
      const p = preview.value;
      return p?.state.phase === 'ready' ? `${p.documentId}:${p.revision}` : null;
    },
    () => {
      generation++;
      release();
      error.value = null;
      const p = preview.value;
      wanted =
        p?.state.phase === 'ready' ? { documentId: p.documentId, revision: p.revision } : null;
      void pump();
    },
    { immediate: true, flush: 'sync' },
  );
  function retry() {
    generation++;
    error.value = null;
    release();
    void pump();
  }
  function imageFailed() {
    release();
    error.value = '浏览器无法显示预览图像，请重试。';
  }
  onScopeDispose(() => {
    disposed = true;
    generation++;
    wanted = null;
    release();
  });
  return { url, error, loading, retry, imageFailed };
}
