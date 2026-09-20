<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import WorkspaceIcon from './WorkspaceIcon.vue';
import type { LayerSummary } from '../../shared/api/generated';
import { layerLabels, treeRows } from './layerTree';
import type { LayerTreeView } from './layerTree';
const props = withDefaults(
  defineProps<{
    layers: LayerSummary[];
    selected: number | null;
    rangeIds: number[];
    rangeBusy: boolean;
    view: LayerTreeView;
    loading: boolean;
    visible?: boolean;
  }>(),
  { visible: true },
);
const emit = defineEmits<{
  'update:view': [view: LayerTreeView];
  select: [id: number | null];
  toggleRange: [id: number];
}>();
const viewport = ref<HTMLElement | null>(null);
const visibleScroll = ref(0);
const viewportHeight = ref(280);
let observer: ResizeObserver | undefined;
let restoredScroll: number | null = null;
const rows = computed(() => treeRows(props.layers, props.view));
const first = computed(() => Math.max(0, Math.floor(visibleScroll.value / 28) - 3));
// 面板高度随窗口改变；按实际视口补足行数，避免高窗口出现空白。
const windowRows = computed(() =>
  rows.value.slice(first.value, first.value + Math.ceil(viewportHeight.value / 28) + 6),
);
const icons = {
  group: 'folder',
  text: 'text',
  bitmap: 'image',
  shape: 'shape',
  smartObject: 'object',
  adjustment: 'adjustment',
  unknown: 'unknown',
} as const;
const update = (value: Partial<LayerTreeView>) => emit('update:view', { ...props.view, ...value });
function toggleRange(event: Event, id: number) {
  // DOM 默认切换不等于核心已确认；保持旧勾选状态直到权威摘要返回。
  if (event.target instanceof HTMLInputElement) event.target.checked = props.rangeIds.includes(id);
  emit('toggleRange', id);
}
function toggle(id: number) {
  update({
    collapsed: props.view.collapsed.includes(id)
      ? props.view.collapsed.filter((value) => value !== id)
      : [...props.view.collapsed, id],
  });
}
function search(event: Event) {
  if (event.target instanceof HTMLInputElement) update({ search: event.target.value, scroll: 0 });
}
function scroll(event: Event) {
  if (!props.visible || !(event.target instanceof HTMLElement)) return;
  const position = event.target.scrollTop;
  visibleScroll.value = position;
  // 恢复大列表时浏览器会按已加载行数截短位置；这不是用户的新视图。
  if (position === restoredScroll) return;
  restoredScroll = null;
  update({ scroll: position });
}
async function restore() {
  await nextTick();
  const element = viewport.value;
  if (!element || !props.visible) return;
  const maxScroll = Math.max(0, element.scrollHeight - element.clientHeight);
  element.scrollTop = Math.min(props.view.scroll, maxScroll);
  restoredScroll = element.scrollTop;
  visibleScroll.value = restoredScroll;
  // 完整加载或失败结束后才收敛保存值，分页中始终保留原恢复目标。
  if (!props.loading && restoredScroll !== props.view.scroll) update({ scroll: restoredScroll });
}
onMounted(() => {
  void restore();
  observer = new ResizeObserver((entries) => {
    const height = entries[0]?.contentRect.height;
    if (height && props.visible) {
      viewportHeight.value = height;
      void restore();
    }
  });
  if (viewport.value) observer.observe(viewport.value);
});
onUnmounted(() => observer?.disconnect());
watch(
  [() => props.view.scroll, () => rows.value.length, () => props.loading, () => props.visible],
  restore,
);
</script>
<template>
  <section class="layer-tree" aria-label="图层树">
    <div class="panel-heading">
      <h2>图层检查</h2>
      <span>{{ loading ? '读取中…' : `${rangeIds.length} 项已勾选` }}</span
      ><button :disabled="selected === null" @click="emit('select', null)">清空检查</button>
    </div>
    <input
      class="layer-search"
      aria-label="搜索图层"
      placeholder="搜索图层名称"
      :value="view.search"
      @input="search"
    />
    <p class="tree-help">点名称查看属性 · 勾选加入任务范围</p>
    <div ref="viewport" class="tree-viewport" role="tree" aria-label="PSD 图层" @scroll="scroll">
      <div :style="{ height: `${rows.length * 28}px`, position: 'relative' }">
        <div
          v-for="(row, index) in windowRows"
          :key="row.layer.id"
          class="tree-row"
          :class="{
            selected: selected === row.layer.id,
            hidden: !row.layer.effectiveVisible,
            'in-range': rangeIds.includes(row.layer.id),
          }"
          role="treeitem"
          :aria-level="row.depth + 1"
          :aria-selected="selected === row.layer.id"
          :aria-expanded="
            row.layer.kind === 'group' ? !view.collapsed.includes(row.layer.id) : undefined
          "
          :style="{ top: `${(first + index) * 28}px`, paddingLeft: `${8 + row.depth * 12}px` }"
        >
          <span
            class="layer-visibility"
            role="img"
            :aria-label="row.layer.effectiveVisible ? '有效可见（只读）' : '有效隐藏（只读）'"
            title="原稿可见状态，只读不可切换"
            ><WorkspaceIcon :name="row.layer.effectiveVisible ? 'eye' : 'eyeOff'"
          /></span>
          <button
            v-if="row.layer.kind === 'group'"
            class="tree-toggle"
            :aria-label="`${view.collapsed.includes(row.layer.id) ? '展开' : '折叠'} ${row.layer.name}`"
            @click="toggle(row.layer.id)"
          >
            {{ view.collapsed.includes(row.layer.id) ? '▸' : '▾' }}</button
          ><span v-else class="tree-leaf"></span>
          <input
            type="checkbox"
            :aria-label="`加入任务范围：${row.layer.name || '（空名称）'}`"
            :checked="rangeIds.includes(row.layer.id)"
            :disabled="rangeBusy"
            @change="toggleRange($event, row.layer.id)"
          />
          <button
            class="tree-name"
            :title="`${row.layer.name}${row.layer.nameTruncated ? '（名称已截断）' : ''} · ${layerLabels[row.layer.kind]}${row.layer.effectiveVisible ? '' : ' · 有效隐藏'}`"
            @click="emit('select', row.layer.id)"
          >
            <WorkspaceIcon :name="icons[row.layer.kind]" /><span class="layer-name"
              >{{ row.layer.name || '（空名称）' }}{{ row.layer.nameTruncated ? '…' : '' }}</span
            >
          </button>
        </div>
      </div>
      <p v-if="!rows.length && !loading" class="subtle">
        {{ view.search ? '没有匹配图层' : '没有图层记录' }}
      </p>
    </div>
  </section>
</template>
