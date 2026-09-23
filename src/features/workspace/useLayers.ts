import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import type { Ref } from 'vue';
import type {
  LayerDetails,
  LayerQuery,
  LayerSummary,
  WorkspaceDocument,
} from '../../shared/api/generated';
import { readLayers } from '../../shared/api/layers';
import { workspaceMessage } from '../../shared/api/workspace';

/** 同一工作区只执行一个图层查询；待展示目标可替换，快速切换不会累积查询队列。 */
export function useLayers(active: Readonly<Ref<WorkspaceDocument | null>>) {
  const layers = shallowRef<LayerSummary[]>([]);
  const details = shallowRef<LayerDetails | null>(null);
  const listError = ref<string | null>(null);
  const detailError = ref<string | null>(null);
  const error = computed(
    () => [listError.value, detailError.value].filter((value) => value !== null).join('；') || null,
  );
  const loading = ref(false);
  // 详情失败不影响完整边缘索引；列表失败或仍在分页时不能将部分图层当成完整目标。
  const complete = computed(() => !!active.value && !loading.value && listError.value === null);
  const detailLoading = ref(false);
  let disposed = false,
    running = false,
    generation = 0,
    detailGeneration = 0;
  let nextOffset: number | null = null;
  let wantedDetail: { layerId: number; textStart: number } | null = null;
  async function pump() {
    if (running || disposed) return;
    running = true;
    try {
      while (!disposed && active.value && (wantedDetail || nextOffset !== null)) {
        const doc = active.value,
          current = generation,
          detailCurrent = detailGeneration;
        const query: LayerQuery = wantedDetail
          ? { kind: 'details', ...wantedDetail }
          : { kind: 'list', offset: nextOffset ?? 0, limit: 128 };
        if (query.kind === 'details') wantedDetail = null;
        else nextOffset = null;
        try {
          const response = await readLayers(doc.id, doc.revision, query);
          if (disposed || current !== generation) continue;
          if (response.result.kind === 'list') {
            const page = response.result.page;
            const combined = [...layers.value, ...page.layers];
            const known = new Set(combined.map((layer) => layer.id));
            if (
              page.offset !== layers.value.length ||
              combined.some((layer) => layer.parentId !== null && !known.has(layer.parentId))
            )
              throw new Error('图层层级或分页不连续，请重新读取。');
            layers.value = combined;
            nextOffset = page.nextOffset;
            loading.value = nextOffset !== null;
          } else if (detailCurrent === detailGeneration) {
            details.value = response.result.details;
            detailLoading.value = false;
          }
        } catch (cause: unknown) {
          if (
            !disposed &&
            current === generation &&
            (query.kind === 'list' || detailCurrent === detailGeneration)
          ) {
            if (query.kind === 'list') {
              listError.value = workspaceMessage(cause);
              loading.value = false;
            } else {
              detailError.value = workspaceMessage(cause);
              detailLoading.value = false;
            }
          }
        }
      }
    } finally {
      running = false;
    }
  }
  function readText(start = 0) {
    detailGeneration++;
    details.value = null;
    detailError.value = null;
    const id = active.value?.selectedLayerId;
    wantedDetail = id === undefined || id === null ? null : { layerId: id, textStart: start };
    detailLoading.value = wantedDetail !== null;
    void pump();
  }
  function reload() {
    generation++;
    layers.value = [];
    listError.value = null;
    nextOffset = active.value ? 0 : null;
    loading.value = active.value !== null;
    readText();
  }
  watch(() => (active.value ? `${active.value.id}:${active.value.revision}` : null), reload, {
    immediate: true,
    flush: 'sync',
  });
  watch(
    () => active.value?.selectedLayerId,
    () => readText(),
    { flush: 'sync' },
  );
  onScopeDispose(() => {
    disposed = true;
    generation++;
    wantedDetail = null;
    nextOffset = null;
  });
  return { layers, details, error, loading, complete, detailLoading, readText, reload };
}
