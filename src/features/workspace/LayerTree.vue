<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import type { LayerSummary } from '../../shared/api/generated';
import { layerLabels, treeRows } from './layerTree';
import type { LayerTreeView } from './layerTree';
const props = defineProps<{
  layers: LayerSummary[];
  selected: number | null;
  view: LayerTreeView;
  loading: boolean;
}>();
const emit = defineEmits<{ 'update:view': [view: LayerTreeView]; select: [id: number | null] }>();
const viewport = ref<HTMLElement | null>(null);
const rows = computed(() => treeRows(props.layers, props.view));
const first = computed(() => Math.max(0, Math.floor(props.view.scroll / 28) - 3));
const windowRows = computed(() => rows.value.slice(first.value, first.value + 18));
const update = (value: Partial<LayerTreeView>) => emit('update:view', { ...props.view, ...value });
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
  if (event.target instanceof HTMLElement) update({ scroll: event.target.scrollTop });
}
async function restore() {
  await nextTick();
  if (viewport.value) viewport.value.scrollTop = props.view.scroll;
}
onMounted(restore);
watch(() => props.view.scroll, restore);
watch(
  () => rows.value.length,
  () => {
    // 分页加载期间保留待恢复滚动位置，最终列表或搜索变化后才收敛到合法范围。
    if (!props.loading && props.view.scroll > Math.max(0, rows.value.length * 28 - 280))
      update({ scroll: Math.max(0, rows.value.length * 28 - 280) });
    else void restore();
  },
);
</script>
<template>
  <section class="layer-tree" aria-label="图层树">
    <div class="panel-heading">
      <h2>图层</h2>
      <span>{{ layers.length }}{{ loading ? ' · 读取中' : '' }}</span
      ><button :disabled="selected === null" @click="emit('select', null)">清空</button>
    </div>
    <input
      class="layer-search"
      aria-label="搜索图层"
      placeholder="搜索图层名称"
      :value="view.search"
      @input="search"
    />
    <div ref="viewport" class="tree-viewport" role="tree" aria-label="PSD 图层" @scroll="scroll">
      <div :style="{ height: `${rows.length * 28}px`, position: 'relative' }">
        <div
          v-for="(row, index) in windowRows"
          :key="row.layer.id"
          class="tree-row"
          :class="{ selected: selected === row.layer.id, hidden: !row.layer.effectiveVisible }"
          role="treeitem"
          :aria-level="row.depth + 1"
          :aria-selected="selected === row.layer.id"
          :aria-expanded="
            row.layer.kind === 'group' ? !view.collapsed.includes(row.layer.id) : undefined
          "
          :style="{ top: `${(first + index) * 28}px`, paddingLeft: `${8 + row.depth * 12}px` }"
        >
          <button
            v-if="row.layer.kind === 'group'"
            class="tree-toggle"
            :aria-label="`${view.collapsed.includes(row.layer.id) ? '展开' : '折叠'} ${row.layer.name}`"
            @click="toggle(row.layer.id)"
          >
            {{ view.collapsed.includes(row.layer.id) ? '▸' : '▾' }}</button
          ><span v-else class="tree-leaf">·</span>
          <button
            class="tree-name"
            :title="`${row.layer.name}${row.layer.nameTruncated ? '（名称已截断）' : ''} · ${layerLabels[row.layer.kind]}${row.layer.effectiveVisible ? '' : ' · 有效隐藏'}`"
            @click="emit('select', row.layer.id)"
          >
            <span>{{ layerLabels[row.layer.kind] }}</span
            >{{ row.layer.name || '（空名称）' }}{{ row.layer.nameTruncated ? '…' : '' }}
          </button>
          <span
            v-if="!row.layer.effectiveVisible"
            class="hidden-mark"
            title="有效隐藏；仅检查，不修改可见性"
            >隐藏</span
          >
        </div>
      </div>
      <p v-if="!rows.length && !loading" class="subtle">
        {{ view.search ? '没有匹配图层' : '没有图层记录' }}
      </p>
    </div>
  </section>
</template>
