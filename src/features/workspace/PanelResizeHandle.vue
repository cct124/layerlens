<script lang="ts">
// Reka 的鼠标注册器是窗口级的；边界交点可能同时激活两轴，取消应覆盖同次手势。
let cancellingResize = false;
</script>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { SplitterResizeHandle } from 'reka-ui';
import { SPLITTER_SIZE } from './panelLayout';

defineProps<{
  id: string;
  direction: 'horizontal' | 'vertical';
  label: string;
  valueText: string;
}>();
const emit = defineEmits<{ start: []; commit: []; cancel: []; reset: [] }>();
const root = ref<InstanceType<typeof SplitterResizeHandle> | null>(null);
const keyboardEvents = ref(0);
let active = false;
let keyboardPending = false;
let disposed = false;
const resizeKeys = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'];

function dragging(value: boolean) {
  if (value) {
    active = true;
    const element: unknown = root.value?.$el;
    if (element instanceof HTMLElement) element.focus();
    emit('start');
  } else if (active) {
    active = false;
    if (cancellingResize) emit('cancel');
    else emit('commit');
  }
}
function cancel() {
  if (keyboardPending) {
    keyboardPending = false;
    emit('cancel');
  }
  if (!active) return;
  cancellingResize = true;
  // Reka 2.10 的公共接口没有取消手势：沿其标准 mouseup 路径结束注册器和全局光标，
  // 再让父组件通过公开 resize() 恢复尺寸；不改库内部状态，也不重挂载面板内容。
  try {
    window.dispatchEvent(new MouseEvent('mouseup', { clientX: -1, clientY: -1 }));
  } finally {
    cancellingResize = false;
  }
}
function escape(event: KeyboardEvent) {
  if ((active || keyboardPending) && event.key === 'Escape' && !event.isComposing) {
    event.preventDefault();
    event.stopImmediatePropagation();
    cancel();
  }
}
function keydown(event: KeyboardEvent) {
  if (active || event.isComposing || event.altKey || event.ctrlKey || event.metaKey) {
    event.preventDefault();
    return;
  }
  if (resizeKeys.includes(event.key) && !keyboardPending) {
    keyboardPending = true;
    emit('start');
  }
}
function keyup(event: KeyboardEvent) {
  if (!keyboardPending || !resizeKeys.includes(event.key) || disposed) return;
  keyboardPending = false;
  // 原生事件监听器之间可能执行微任务；nextTick 不能代表后续 keydown 监听已完成。
  // 松键时该次键盘调整已结束，和鼠标松开一致地提交偏好；失焦／Esc 会先撤销。
  emit('commit');
  keyboardEvents.value++;
}
function mouseDown(event: MouseEvent) {
  if (event.button !== 0 || event.buttons !== 1) event.stopPropagation();
}
function visibility() {
  if (document.hidden) cancel();
}
onMounted(() => {
  window.addEventListener('keydown', escape, true);
  window.addEventListener('blur', cancel);
  window.addEventListener('pointercancel', cancel, true);
  window.addEventListener('touchcancel', cancel, true);
  document.addEventListener('visibilitychange', visibility);
});
onBeforeUnmount(() => {
  disposed = true;
  cancel();
  window.removeEventListener('keydown', escape, true);
  window.removeEventListener('blur', cancel);
  window.removeEventListener('pointercancel', cancel, true);
  window.removeEventListener('touchcancel', cancel, true);
  document.removeEventListener('visibilitychange', visibility);
});
defineExpose({ cancel });
</script>

<template>
  <SplitterResizeHandle
    :id="id"
    ref="root"
    :hit-area-margins="{ fine: 0, coarse: 0 }"
    class="panel-resize-handle"
    :data-dev-target="direction === 'horizontal' ? 'panel:width' : 'panel:height'"
    :data-panel-key-events="keyboardEvents"
    :style="{ flexBasis: `${SPLITTER_SIZE}px` }"
    :aria-label="label"
    :aria-orientation="direction === 'horizontal' ? 'vertical' : 'horizontal'"
    :aria-valuetext="valueText"
    :title="`${label} · 拖动或方向键调整 · 双击恢复默认`"
    @dragging="dragging"
    @mousedown="mouseDown"
    @keydown.capture="keydown"
    @keydown.stop
    @keyup.capture="keyup"
    @keyup.stop
    @blur="cancel"
    @dblclick.prevent.stop="emit('reset')"
    @contextmenu.prevent
  />
</template>

<style scoped>
.panel-resize-handle {
  flex-grow: 0;
  flex-shrink: 0;
  background: transparent;
  touch-action: none;
}
.panel-resize-handle[data-orientation='horizontal'] {
  cursor: col-resize;
}
.panel-resize-handle[data-orientation='vertical'] {
  cursor: row-resize;
}
</style>
