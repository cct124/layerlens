<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { fitZoom, initialView, locateBounds, zoomAround } from './canvasView';
import type { CanvasView } from './canvasView';
import type { Bounds } from '../../shared/api/generated';
const props = defineProps<{
  width: number;
  height: number;
  url: string | null;
  name: string;
  view: CanvasView;
  bounds: Bounds | null;
}>();
const emit = defineEmits<{ 'update:view': [view: CanvasView]; imageError: [] }>();
const viewport = ref<HTMLElement | null>(null);
const size = ref({ width: 0, height: 0 });
const effective = computed(() =>
  props.view.mode === 'fit'
    ? {
        ...initialView(),
        zoom: fitZoom(props.width, props.height, size.value.width, size.value.height),
      }
    : props.view,
);
let observer: ResizeObserver | undefined;
let drag: { id: number; x: number; y: number; view: CanvasView } | null = null;
onMounted(() => {
  observer = new ResizeObserver((entries) => {
    const entry = entries[0];
    if (entry) size.value = { width: entry.contentRect.width, height: entry.contentRect.height };
  });
  if (viewport.value) observer.observe(viewport.value);
});
onUnmounted(() => observer?.disconnect());
function zoom(factor: number) {
  emit('update:view', zoomAround(effective.value, effective.value.zoom * factor));
}
function wheel(event: WheelEvent) {
  if (!viewport.value) return;
  const bounds = viewport.value.getBoundingClientRect();
  emit(
    'update:view',
    zoomAround(
      effective.value,
      effective.value.zoom * Math.exp(-Math.max(-200, Math.min(200, event.deltaY)) * 0.002),
      event.clientX - bounds.left - bounds.width / 2,
      event.clientY - bounds.top - bounds.height / 2,
    ),
  );
}
function start(event: PointerEvent) {
  if (event.button !== 0 || !viewport.value) return;
  viewport.value.setPointerCapture(event.pointerId);
  drag = { id: event.pointerId, x: event.clientX, y: event.clientY, view: effective.value };
}
function move(event: PointerEvent) {
  if (drag?.id === event.pointerId)
    emit('update:view', {
      ...drag.view,
      mode: 'manual',
      x: drag.view.x + event.clientX - drag.x,
      y: drag.view.y + event.clientY - drag.y,
    });
}
function end() {
  drag = null;
}
function locate() {
  if (props.bounds?.width && props.bounds.height)
    emit(
      'update:view',
      locateBounds(props.width, props.height, props.bounds, size.value.width, size.value.height),
    );
}
defineExpose({ locate });
</script>
<template>
  <div class="preview-pane">
    <div class="canvas-controls" aria-label="画布视图">
      <span>保存时合成预览</span>
      <div class="zoom-controls">
        <button aria-label="缩小" @click="zoom(0.8)">−</button
        ><output aria-label="当前缩放">{{ (effective.zoom * 100).toFixed(0) }}%</output
        ><button aria-label="放大" @click="zoom(1.25)">+</button
        ><button @click="emit('update:view', { mode: 'manual', zoom: 1, x: 0, y: 0 })">100%</button
        ><button @click="emit('update:view', initialView())">适应窗口</button>
      </div>
    </div>
    <div
      ref="viewport"
      class="preview-viewport"
      aria-label="PSD 预览画布"
      tabindex="0"
      @wheel.prevent="wheel"
      @pointerdown="start"
      @pointermove="move"
      @pointerup="end"
      @pointercancel="end"
      @lostpointercapture="end"
      @keydown.left.prevent="
        emit('update:view', { ...effective, mode: 'manual', x: effective.x - 30 })
      "
      @keydown.right.prevent="
        emit('update:view', { ...effective, mode: 'manual', x: effective.x + 30 })
      "
      @keydown.up.prevent="
        emit('update:view', { ...effective, mode: 'manual', y: effective.y - 30 })
      "
      @keydown.down.prevent="
        emit('update:view', { ...effective, mode: 'manual', y: effective.y + 30 })
      "
    >
      <img
        v-if="url"
        :key="url"
        class="preview-image"
        :src="url"
        :alt="`${name} 保存时合成预览`"
        draggable="false"
        :width="width"
        :height="height"
        :style="{
          width: `${width}px`,
          height: `${height}px`,
          transform: `translate(-50%, -50%) translate(${effective.x}px, ${effective.y}px) scale(${effective.zoom})`,
        }"
        @error="emit('imageError')"
      />
      <div
        v-if="bounds && bounds.width > 0 && bounds.height > 0"
        class="layer-boundary"
        aria-label="选中图层几何边界"
        :style="{
          left: `calc(50% + ${effective.x + (bounds.x - width / 2) * effective.zoom}px)`,
          top: `calc(50% + ${effective.y + (bounds.y - height / 2) * effective.zoom}px)`,
          width: `${bounds.width * effective.zoom}px`,
          height: `${bounds.height * effective.zoom}px`,
        }"
      ></div>
      <div v-if="!url" class="canvas-message"><slot /></div>
    </div>
    <div class="canvas-footer">
      <span>{{ width }} × {{ height }} 原稿像素</span><span>拖动平移 · 滚轮缩放 · 方向键移动</span>
    </div>
  </div>
</template>
