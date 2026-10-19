import { onMounted, onScopeDispose, shallowRef, watch } from 'vue';
import type { Ref } from 'vue';
import { createCompositeRenderer, emptyCompositeState } from './compositeRenderer';
import type { CompositeFrame } from './compositeRenderer';

/** 生产 Canvas 桥：不依赖开发引导、实验字体或实验服务。 */
export function useCompositeCanvas(
  host: Ref<HTMLDivElement | null>,
  frame: () => CompositeFrame,
  failed: (message: string) => void,
) {
  const state = shallowRef(emptyCompositeState());
  let renderer: ReturnType<typeof createCompositeRenderer> | null = null;
  let reportedError: string | null = null;
  onMounted(() => {
    if (!host.value) return;
    renderer = createCompositeRenderer(host.value, (value) => {
      state.value = value;
      if (value.error && value.error !== reportedError) {
        reportedError = value.error;
        failed(value.error);
      } else if (!value.error) reportedError = null;
    });
    renderer.update(frame());
  });
  watch(frame, (value) => renderer?.update(value), { flush: 'post' });
  onScopeDispose(() => {
    renderer?.dispose();
    renderer = null;
  });
  return state;
}
