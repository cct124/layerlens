<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { useWorkspace } from './useWorkspace';
import { useAppStatus } from './useAppStatus';
import PreviewCanvas from './PreviewCanvas.vue';
import LayerTree from './LayerTree.vue';
import LayerProperties from './LayerProperties.vue';
import './inspection.css';
import { useLayers } from './useLayers';
import { initialTree } from './layerTree';
import type { LayerTreeView } from './layerTree';
import { initialView } from './canvasView';
import type { CanvasView } from './canvasView';
import type { WorkspaceJobPhase } from '../../shared/api/generated';
const { desktop, snapshot, active, error, connecting, picking, preview, act, connect, open } =
  useWorkspace();
const { url, error: imageError, loading, retry, imageFailed } = preview;
const { status } = useAppStatus();
const views = reactive(new Map<string, CanvasView>());
const trees = reactive(new Map<string, { revision: string; view: LayerTreeView }>());
const {
  layers,
  details,
  error: layerError,
  loading: layersLoading,
  detailLoading,
  readText,
  reload: reloadLayers,
} = useLayers(active);
const canvas = ref<InstanceType<typeof PreviewCanvas> | null>(null);
const selectedBounds = computed(
  () => layers.value.find((layer) => layer.id === active.value?.selectedLayerId)?.bounds ?? null,
);
function selectLayer(layerId: number | null) {
  if (active.value)
    void act({
      kind: 'selectLayer',
      documentId: active.value.id,
      revision: active.value.revision,
      layerId,
    });
}
watch(
  snapshot,
  (value) => {
    const ids = new Set(value?.documents.map((d) => d.id));
    for (const id of views.keys()) if (!ids.has(id)) views.delete(id);
    for (const id of ids) if (!views.has(id)) views.set(id, initialView());
    for (const id of trees.keys()) if (!ids.has(id)) trees.delete(id);
    for (const doc of value?.documents ?? [])
      if (trees.get(doc.id)?.revision !== doc.revision)
        trees.set(doc.id, { revision: doc.revision, view: initialTree() });
  },
  { flush: 'sync' },
);
const phases: Record<WorkspaceJobPhase, string> = {
  queued: '排队中',
  running: '处理中',
  cancelling: '正在取消，等待计算结束',
  finishing: '正在释放资源',
};
function retryPreview() {
  if (imageError.value) retry();
  else void act({ kind: 'retryPreview' });
}
</script>
<template>
  <div class="app-shell">
    <header class="app-header">
      <div class="brand"><span class="brand-mark" aria-hidden="true">◈</span> LayerLens</div>
      <span class="header-label">设计的细节，实现的起点</span
      ><span class="readonly-label">PSD 只读</span
      ><button
        class="primary-button"
        :disabled="!desktop || picking || !snapshot || snapshot.shuttingDown"
        @click="open"
      >
        {{ picking ? '选择文件中…' : '打开 PSD' }}
      </button>
    </header>
    <div v-if="error" class="notice error-notice" role="alert">
      <span>{{ error }}</span
      ><button @click="connect">重新连接</button
      ><button aria-label="关闭提示" @click="error = null">×</button>
    </div>
    <div v-if="snapshot?.notices.length" class="notice-list" aria-label="工作区通知">
      <div v-for="notice in snapshot.notices" :key="notice.id" class="notice" role="status">
        <span>{{ notice.message }}</span>
        <button aria-label="关闭通知" @click="act({ kind: 'dismissNotice', noticeId: notice.id })">
          ×
        </button>
      </div>
    </div>
    <nav class="document-tabs" aria-label="已打开文档">
      <div
        v-for="doc in snapshot?.documents"
        :key="doc.id"
        class="document-tab"
        :class="{ active: doc.id === active?.id }"
      >
        <button
          class="tab-select"
          :aria-pressed="doc.id === active?.id"
          :title="doc.path"
          @click="act({ kind: 'activate', documentId: doc.id })"
        >
          {{ doc.name }}</button
        ><button
          class="tab-close"
          :aria-label="`关闭 ${doc.name}`"
          @click="act({ kind: 'close', documentId: doc.id })"
        >
          ×
        </button>
      </div>
      <span v-if="!snapshot?.documents.length" class="tab-placeholder">工作区</span>
    </nav>
    <div v-if="snapshot?.jobs.length" class="job-list" aria-live="polite">
      <div v-for="job in snapshot.jobs" :key="job.id" class="job">
        <span>{{ job.label }} · {{ phases[job.phase] }}</span
        ><button
          :disabled="job.phase === 'cancelling' || job.phase === 'finishing'"
          @click="act({ kind: 'cancel', jobId: job.id })"
        >
          取消
        </button>
      </div>
    </div>
    <main class="workspace-layout">
      <aside class="tool-rail" aria-label="画布工具">
        <span class="tool-selected" title="平移画布" aria-label="平移画布">✥</span
        ><span class="tool-rail-label">只读</span>
      </aside>
      <section class="workspace" aria-label="文档工作区">
        <PreviewCanvas
          v-if="active"
          ref="canvas"
          :key="active.id"
          :width="active.width"
          :height="active.height"
          :name="active.name"
          :url="url"
          :view="views.get(active.id) ?? initialView()"
          :bounds="selectedBounds"
          @update:view="views.set(active.id, $event)"
          @image-error="imageFailed"
        >
          <template v-if="imageError"
            ><h2>预览未能显示</h2>
            <p role="alert">{{ imageError }}</p>
            <button @click="retryPreview">重试预览</button></template
          >
          <template v-else-if="snapshot?.preview?.state.phase === 'failed'"
            ><h2>当前预览不可用</h2>
            <p role="alert">{{ snapshot.preview.state.error.message }}</p>
            <button @click="retryPreview">重试预览</button></template
          >
          <template v-else-if="snapshot?.preview?.state.phase === 'cancelled'"
            ><h2>预览已取消</h2>
            <button @click="retryPreview">重试预览</button></template
          >
          <template v-else
            ><span class="loading-dot" aria-hidden="true"></span>
            <h2>{{ loading ? '正在传输预览' : '正在准备预览' }}</h2>
            <template v-if="snapshot?.preview?.state.phase === 'pending'"
              ><p>{{ phases[snapshot.preview.state.status] }}</p>
              <button
                v-if="snapshot.preview.state.jobId"
                :disabled="
                  snapshot.preview.state.status === 'cancelling' ||
                  snapshot.preview.state.status === 'finishing'
                "
                @click="act({ kind: 'cancel', jobId: snapshot.preview.state.jobId })"
              >
                取消预览
              </button></template
            ></template
          >
        </PreviewCanvas>
        <div v-else class="empty-state">
          <div class="empty-art" aria-hidden="true">◈</div>
          <p class="eyebrow">从原稿开始</p>
          <h1>为设计留一个工作台</h1>
          <p>打开 PSD，查看保存时的画面与原稿信息。<br />多份设计可以同时保留，随时切换。</p>
          <button
            v-if="desktop"
            class="primary-button"
            :disabled="picking || !snapshot || snapshot.shuttingDown"
            @click="open"
          >
            打开 PSD 文件
          </button>
          <p v-else class="subtle">浏览器预览 · 请在桌面应用中继续</p>
          <p v-if="connecting" role="status">正在连接桌面工作区…</p>
        </div>
      </section>
      <aside class="sidebar" aria-label="图层与只读属性">
        <template v-if="active">
          <LayerTree
            :key="`${active.id}:${active.revision}`"
            :layers="layers"
            :selected="active.selectedLayerId"
            :loading="layersLoading"
            :view="trees.get(active.id)?.view ?? initialTree()"
            @select="selectLayer"
            @update:view="trees.set(active.id, { revision: active.revision, view: $event })"
          />
          <div v-if="layerError" class="layer-error" role="alert">
            {{ layerError }} <button @click="reloadLayers">重新读取</button>
          </div>
          <LayerProperties
            :details="details"
            :loading="detailLoading"
            @page="readText"
            @locate="canvas?.locate()"
          />
          <details class="document-details">
            <summary>文档信息 · {{ active.name }}</summary>
            <dl class="document-facts">
              <dt>画布</dt>
              <dd>{{ active.width }} × {{ active.height }} px</dd>
              <dt>颜色</dt>
              <dd>{{ active.colorMode }} / {{ active.bitDepth }} 位</dd>
              <dt>图层记录</dt>
              <dd>{{ active.layerCount }}</dd>
              <dt>修订</dt>
              <dd>{{ active.revision }}</dd>
            </dl>
            <p class="document-path" :title="active.path">{{ active.path }}</p>
            <button class="wide-button" @click="act({ kind: 'reload', documentId: active.id })">
              从磁盘重新加载
            </button>
            <p class="subtle">文件改变后请主动重载；加载失败时保留已有内容。</p>
            <div class="sidebar-divider"></div>
            <h3>预览范围</h3>
            <p class="preview-note">{{ active.previewNote }}</p>
          </details>
        </template>
        <template v-else
          ><p class="subtle">打开文档后，在这里查看画布尺寸、颜色模式和预览能力。</p>
          <div class="sidebar-divider"></div>
          <h3>忠于 PSD 原稿</h3>
          <p class="subtle">保留原始数据与单位，为后续判断提供依据。</p></template
        >
        <div class="sidebar-footer">
          <span class="status-dot" :class="{ ready: !!snapshot }"></span
          >{{ snapshot ? '桌面工作区已连接' : desktop ? '工作区未连接' : '浏览器预览'
          }}<span v-if="status.phase === 'ready'" class="app-version"
            >v{{ status.info.appVersion }}</span
          >
        </div>
      </aside>
    </main>
  </div>
</template>
