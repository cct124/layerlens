<script setup lang="ts">
import { computed, ref } from 'vue';
import type { LayerDetails } from '../../shared/api/generated';
import { layerLabels } from './layerTree';
const props = defineProps<{ details: LayerDetails | null; loading: boolean }>();
const emit = defineEmits<{ locate: []; page: [start: number] }>();
const copyMessage = ref('');
const bounds = computed(() => props.details?.layer.bounds);
async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    copyMessage.value = '已复制';
  } catch {
    copyMessage.value = '复制失败，请手动选择内容复制。';
  }
}
</script>
<template>
  <section class="layer-properties" aria-label="只读图层属性">
    <div class="panel-heading">
      <h2>只读属性</h2>
      <button v-if="details" @click="copy(JSON.stringify(details, null, 2))">复制本页属性</button>
    </div>
    <p v-if="loading" role="status" class="subtle">正在读取属性…</p>
    <div v-else-if="!details" class="panel-empty">
      <p>尚未检查图层</p>
      <span>在下方点击图层名称，查看位置、尺寸和文字样式。勾选框仅用于选择任务范围。</span>
    </div>
    <template v-else>
      <h3>{{ details.layer.name || '（空名称）' }}</h3>
      <p v-if="details.layer.nameTruncated" class="subtle">名称显示前 256 个字符。</p>
      <dl class="property-facts">
        <dt>类型</dt>
        <dd>{{ layerLabels[details.layer.kind] }}</dd>
        <dt>几何位置</dt>
        <dd>{{ bounds?.x }}, {{ bounds?.y }} px</dd>
        <dt>几何尺寸</dt>
        <dd>{{ bounds?.width }} × {{ bounds?.height }} px</dd>
        <dt>不透明度</dt>
        <dd>{{ details.layer.opacity }} / 255</dd>
        <dt>自身 / 有效可见</dt>
        <dd>
          {{ details.layer.visible ? '可见' : '隐藏' }} /
          {{ details.layer.effectiveVisible ? '可见' : '隐藏' }}
        </dd>
      </dl>
      <button
        class="wide-button"
        :disabled="!bounds?.width || !bounds?.height"
        @click="emit('locate')"
      >
        在画布中定位
      </button>
      <p class="subtle">边界为 PSD 几何范围，可能不含效果；零面积图层仅显示属性。</p>
      <template v-if="details.text">
        <div class="panel-heading">
          <h3>文字原文</h3>
          <button @click="copy(details.text.text)">复制本段文字</button>
        </div>
        <pre class="raw-text">{{ details.text.text }}</pre>
        <p class="subtle">
          UTF-16 [{{ details.text.start }}, {{ details.text.end }}) / {{ details.text.totalLength }}
        </p>
        <div class="text-pages">
          <button v-if="details.text.start" @click="emit('page', 0)">回到开头</button
          ><button
            v-if="details.text.nextStart !== null"
            @click="emit('page', details.text.nextStart)"
          >
            下一段
          </button>
        </div>
        <p class="capability-note">{{ details.text.styles.reason }}</p>
        <p class="subtle">
          字号及间距为未验证的引擎单位；RGB 通道未做 ICC 转换，不推导 CSS 或字体可用性。
        </p>
        <details v-if="details.text.transform">
          <summary>原始文字变换矩阵</summary>
          <pre>{{ details.text.transform.join(', ') }}</pre>
        </details>
        <p v-if="details.text.styleRuns === null" class="subtle">字符样式不可用，原因见诊断。</p>
        <details v-for="run in details.text.styleRuns ?? []" :key="run.start" class="style-run">
          <summary>
            字符 [{{ run.start }}, {{ run.end }}) · {{ run.style.font?.value.name ?? '字体缺失' }} ·
            {{ run.style.fontSize?.value.value ?? '字号缺失' }}
          </summary>
          <p v-if="run.style.fillColor">
            RGB {{ run.style.fillColor.value.red }}, {{ run.style.fillColor.value.green }},
            {{ run.style.fillColor.value.blue }} / α {{ run.style.fillColor.value.alpha }}
          </p>
          <pre>{{ JSON.stringify(run.style, null, 2) }}</pre>
        </details>
        <p v-if="details.text.paragraphRuns === null" class="subtle">段落样式不可用。</p>
        <details
          v-for="run in details.text.paragraphRuns ?? []"
          :key="`p${run.start}`"
          class="style-run"
        >
          <summary>
            段落 [{{ run.start }}, {{ run.end }}) · {{ run.style.alignment?.value ?? '对齐缺失' }}
          </summary>
          <pre>{{ JSON.stringify(run.style, null, 2) }}</pre>
        </details>
        <details v-if="details.text.diagnostics.length">
          <summary>文字诊断（{{ details.text.diagnostics.length }}）</summary>
          <p v-for="(item, index) in details.text.diagnostics" :key="index" class="subtle">
            {{ item.path }}：{{ item.message }}
          </p>
        </details>
        <p v-if="details.text.diagnosticsTruncated" class="subtle">仅展示前 32 条文字诊断。</p>
      </template>
      <h3>独立素材能力 · {{ details.export.status }}</h3>
      <p class="capability-note">{{ details.export.reason }}</p>
      <details v-if="details.diagnostics.length">
        <summary>图层诊断（{{ details.diagnostics.length }}）</summary>
        <p v-for="(item, index) in details.diagnostics" :key="index" class="subtle">{{ item }}</p>
      </details>
      <p v-if="details.diagnosticsTruncated" class="subtle">诊断已按显示预算截断。</p>
    </template>
    <p v-if="copyMessage" role="status" class="subtle">{{ copyMessage }}</p>
  </section>
</template>
