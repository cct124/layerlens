<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { SplitterGroup, SplitterPanel } from 'reka-ui';
import PanelResizeHandle from './PanelResizeHandle.vue';
import {
  defaultPanelPreferences,
  inspectorLayout,
  PANEL_LAYOUT_KEY,
  parsePanelPreferences,
  sidebarLayout,
} from './panelLayout';

const root = ref<HTMLElement | null>(null);
const stack = ref<HTMLElement | null>(null);
const sidebar = ref<InstanceType<typeof SplitterPanel> | null>(null);
const inspector = ref<InstanceType<typeof SplitterPanel> | null>(null);
const widthHandle = ref<InstanceType<typeof PanelResizeHandle> | null>(null);
const heightHandle = ref<InstanceType<typeof PanelResizeHandle> | null>(null);
const preference = ref(defaultPanelPreferences());
const warning = ref<string | null>(null);
const width = ref(0);
const height = ref(0);
const widthLayout = computed(() =>
  sidebarLayout(width.value || 1000, preference.value.sidebarWidth),
);
const heightLayout = computed(() =>
  inspectorLayout(height.value || 600, preference.value.inspectorPercent),
);
const sidebarPercent = ref(widthLayout.value.percent);
const inspectorPercent = ref(heightLayout.value.percent);
const sidebarPixels = computed(() =>
  Math.round((sidebarPercent.value / 100) * widthLayout.value.available),
);
const before = { width: null as number | null, height: null as number | null };
let observer: ResizeObserver | undefined;
let disposed = false;
const message = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));

try {
  preference.value = parsePanelPreferences(localStorage.getItem(PANEL_LAYOUT_KEY));
} catch (cause: unknown) {
  warning.value = `无法恢复面板布局，已使用默认布局：${message(cause)}`;
}

function save() {
  try {
    localStorage.setItem(PANEL_LAYOUT_KEY, JSON.stringify(preference.value));
    warning.value = null;
  } catch (cause: unknown) {
    warning.value = `面板布局仅在本次运行有效，无法保存：${message(cause)}`;
  }
}
type Axis = 'width' | 'height';
function panel(axis: Axis) {
  return axis === 'width' ? sidebar.value : inspector.value;
}
function start(axis: Axis) {
  before[axis] = panel(axis)?.getSize() ?? null;
}
function commit(axis: Axis) {
  const value = panel(axis)?.getSize();
  const previous = before[axis];
  before[axis] = null;
  if (disposed || value === undefined || previous === null || Math.abs(value - previous) < 0.001)
    return;
  if (axis === 'width' && width.value > 0) {
    // 极小浏览器视口的临时退让不写成正常桌面的偏好下限。
    preference.value.sidebarWidth = Math.max(
      260,
      Math.min(560, (value / 100) * widthLayout.value.available),
    );
  } else if (axis === 'height' && height.value > 0) preference.value.inspectorPercent = value;
  else return;
  save();
}
function apply(axis: Axis) {
  if (disposed) return;
  panel(axis)?.resize(axis === 'width' ? widthLayout.value.percent : heightLayout.value.percent);
}
function cancel(axis: Axis) {
  before[axis] = null;
  void nextTick(() => apply(axis));
}
function reset(axis?: Axis) {
  widthHandle.value?.cancel();
  heightHandle.value?.cancel();
  const defaults = defaultPanelPreferences();
  if (!axis || axis === 'width') preference.value.sidebarWidth = defaults.sidebarWidth;
  if (!axis || axis === 'height') preference.value.inspectorPercent = defaults.inspectorPercent;
  void nextTick(() => {
    if (!axis || axis === 'width') apply('width');
    if (!axis || axis === 'height') apply('height');
  });
  save();
}
function layout(axis: Axis, values: number[]) {
  const value = values[axis === 'width' ? 1 : 0];
  if (value !== undefined && Number.isFinite(value)) {
    if (axis === 'width') sidebarPercent.value = value;
    else inspectorPercent.value = value;
  }
}
watch([width, () => preference.value.sidebarWidth], async () => {
  await nextTick();
  apply('width');
});
watch([height, () => preference.value.inspectorPercent], async () => {
  await nextTick();
  apply('height');
});
onMounted(() => {
  observer = new ResizeObserver((entries) => {
    for (const entry of entries) {
      if (
        entry.target === root.value &&
        entry.contentRect.width > 0 &&
        width.value !== entry.contentRect.width
      ) {
        widthHandle.value?.cancel();
        width.value = entry.contentRect.width;
      }
      if (
        entry.target === stack.value &&
        entry.contentRect.height > 0 &&
        height.value !== entry.contentRect.height
      ) {
        heightHandle.value?.cancel();
        height.value = entry.contentRect.height;
      }
    }
  });
  if (root.value) {
    width.value = root.value.getBoundingClientRect().width;
    observer.observe(root.value);
  }
  if (stack.value) {
    height.value = stack.value.getBoundingClientRect().height;
    observer.observe(stack.value);
  }
});
onBeforeUnmount(() => {
  disposed = true;
  observer?.disconnect();
});
defineExpose({ reset });
</script>

<template>
  <div ref="root" class="workspace-panels">
    <SplitterGroup
      id="workspace-width"
      direction="horizontal"
      :keyboard-resize-by="1"
      @layout="layout('width', $event)"
    >
      <SplitterPanel
        id="workspace-canvas"
        class="workspace-panel"
        :min-size="100 - widthLayout.maxPercent"
      >
        <slot name="canvas" />
      </SplitterPanel>
      <PanelResizeHandle
        id="sidebar-width-handle"
        ref="widthHandle"
        direction="horizontal"
        label="调整画布与右侧面板宽度"
        :value-text="`右侧面板 ${sidebarPixels} 像素`"
        @start="start('width')"
        @commit="commit('width')"
        @cancel="cancel('width')"
        @reset="reset('width')"
      />
      <SplitterPanel
        id="workspace-sidebar"
        ref="sidebar"
        as="aside"
        class="sidebar"
        aria-label="图层与只读属性"
        :default-size="widthLayout.percent"
        :min-size="widthLayout.minPercent"
        :max-size="widthLayout.maxPercent"
      >
        <div v-if="warning" class="layout-warning" role="status">
          <span>{{ warning }}</span
          ><button aria-label="关闭布局提示" @click="warning = null">×</button>
        </div>
        <div ref="stack" class="sidebar-stack">
          <SplitterGroup
            id="workspace-height"
            direction="vertical"
            :keyboard-resize-by="1"
            @layout="layout('height', $event)"
          >
            <SplitterPanel
              id="workspace-inspector"
              ref="inspector"
              class="workspace-panel"
              :default-size="heightLayout.percent"
              :min-size="heightLayout.minPercent"
              :max-size="heightLayout.maxPercent"
            >
              <slot name="inspector" />
            </SplitterPanel>
            <PanelResizeHandle
              id="sidebar-height-handle"
              ref="heightHandle"
              direction="vertical"
              label="调整属性与图层面板高度"
              :value-text="`上方面板 ${Math.round(inspectorPercent)}%`"
              @start="start('height')"
              @commit="commit('height')"
              @cancel="cancel('height')"
              @reset="reset('height')"
            />
            <SplitterPanel
              id="workspace-resources"
              class="workspace-panel"
              :min-size="100 - heightLayout.maxPercent"
            >
              <slot name="resources" />
            </SplitterPanel>
          </SplitterGroup>
        </div>
        <slot name="footer" />
      </SplitterPanel>
    </SplitterGroup>
  </div>
</template>

<style scoped>
.workspace-panels,
.workspace-panel,
.sidebar-stack {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.workspace-panels {
  overflow: hidden;
}
.sidebar-stack {
  flex: 1;
}
.layout-warning {
  display: flex;
  gap: 4px;
  padding: 6px;
  font-size: 11px;
  color: #ebc58b;
}
.layout-warning span {
  flex: 1;
  overflow-wrap: anywhere;
}
.layout-warning button {
  flex: 0 0 auto;
  align-self: start;
}
</style>
