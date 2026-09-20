<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useWorkspace } from './useWorkspace';
import { useAppStatus } from './useAppStatus';
import PreviewCanvas from './PreviewCanvas.vue';
import LayerTree from './LayerTree.vue';
import SelectionPanel from './SelectionPanel.vue';
import { useSelection } from './useSelection';
import { useCanvasTool } from './useCanvasTool';
import LayerProperties from './LayerProperties.vue';
import DockTabs from './DockTabs.vue';
import WorkspaceIcon from './WorkspaceIcon.vue';
import WorkspaceMenuBar from './WorkspaceMenuBar.vue';
import WorkspaceTitleBar from './WorkspaceTitleBar.vue';
import WorkspacePanels from './WorkspacePanels.vue';
import type { WorkspaceMenu, WorkspaceMenuCommand } from './workspaceMenu';
import { workspaceShortcut } from './workspaceShortcuts';
import './inspection.css';
import { useLayers } from './useLayers';
import { initialTree } from './layerTree';
import type { LayerTreeView } from './layerTree';
import { initialView } from './canvasView';
import type { CanvasView } from './canvasView';
import type {
  SelectionBounds,
  WorkspaceAction,
  WorkspaceJobPhase,
} from '../../shared/api/generated';
const {
  desktop,
  snapshot,
  active,
  error,
  connecting,
  picking,
  preview,
  act: workspaceAct,
  connect,
  open: openFiles,
} = useWorkspace();
const selection = useSelection(
  computed(() => snapshot.value?.selection ?? null),
  connect,
);
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
const panels = ref<InstanceType<typeof WorkspacePanels> | null>(null);
const inspectorTab = ref('properties');
const resourceTab = ref('layers');
const canOpen = computed(
  () => desktop && !picking.value && !!snapshot.value && !snapshot.value.shuttingDown,
);
const toolsBusy = computed(
  () => selection.busy || !!pendingTool.value || !!snapshot.value?.shuttingDown || picking.value,
);
const scopeLabel = computed(() => {
  const scope = selection.summary?.scope;
  if (!scope) return '未选择任务范围';
  return `${selection.summary?.region ? '区域' : '图层'}范围 · ${scope.targetCount} 个图层`;
});
const {
  tool,
  pending: pendingTool,
  chooseTool,
} = useCanvasTool(selection, () => canvas.value?.cancel());
const menus = computed<WorkspaceMenu[]>(() => [
  {
    id: 'file',
    label: '文件',
    accessKey: 'F',
    items: [
      { command: 'open', label: '打开 PSD', shortcut: 'Ctrl+O', disabled: !canOpen.value },
      { command: 'reload', label: '重新载入', disabled: !active.value || toolsBusy.value },
      {
        command: 'close',
        label: '关闭当前文档',
        disabled: !active.value || toolsBusy.value,
        separatorBefore: true,
      },
    ],
  },
  {
    id: 'view',
    label: '视图',
    accessKey: 'V',
    items: [
      {
        command: 'fit',
        label: '适应窗口',
        shortcut: 'Ctrl+0',
        disabled: !active.value || toolsBusy.value,
      },
      {
        command: 'actual',
        label: '实际像素（100%）',
        shortcut: 'Ctrl+1',
        disabled: !active.value || toolsBusy.value,
      },
    ],
  },
  {
    id: 'window',
    label: '窗口',
    accessKey: 'W',
    items: [
      { command: 'properties', label: '属性', checked: inspectorTab.value === 'properties' },
      { command: 'document', label: '文档信息', checked: inspectorTab.value === 'document' },
      {
        command: 'layers',
        label: '图层',
        checked: resourceTab.value === 'layers',
        separatorBefore: true,
      },
      { command: 'tasks', label: '任务', checked: resourceTab.value === 'tasks' },
      { command: 'resetLayout', label: '重置面板布局', separatorBefore: true },
    ],
  },
]);
const selectionContext = computed(() => {
  const current = snapshot.value?.selection;
  return current
    ? `${current.sessionId}/${current.documentId}/${current.documentRevision}/${current.selectionRevision}`
    : '';
});
function commitRegion(bounds: SelectionBounds, contextKey: string) {
  if (selectionContext.value === contextKey) void selection.selectRegion(bounds);
}
function clearCanvasSelection(contextKey: string) {
  if (tool.value === 'region' && selectionContext.value === contextKey && selection.summary?.scope)
    void selection.clear();
}
function act(action: WorkspaceAction) {
  if (action.kind === 'activate' || action.kind === 'close' || action.kind === 'reload')
    canvas.value?.cancel();
  return workspaceAct(action);
}
function open() {
  canvas.value?.cancel();
  return openFiles();
}
const selectedBounds = computed(
  () => layers.value.find((layer) => layer.id === active.value?.selectedLayerId)?.bounds ?? null,
);
function selectLayer(layerId: number | null) {
  inspectorTab.value = 'properties';
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
function runMenuCommand(command: WorkspaceMenuCommand) {
  if (command === 'resetLayout') {
    panels.value?.reset();
  } else if (command === 'open') {
    if (canOpen.value) void open();
  } else if (command === 'properties' || command === 'document') {
    inspectorTab.value = command;
  } else if (command === 'layers' || command === 'tasks') {
    resourceTab.value = command;
  } else if (active.value && !toolsBusy.value) {
    if (command === 'reload' || command === 'close')
      void act({ kind: command, documentId: active.value.id });
    else if (command === 'fit') canvas.value?.fit();
    else canvas.value?.actualSize();
  }
}
function shortcut(event: KeyboardEvent) {
  const action = workspaceShortcut(event);
  if (!action) return;
  // 即使当前不可用，也不能让已识别的应用快捷键触发 WebView 文件选择或页面缩放。
  event.preventDefault();
  if (action === 'pan' || action === 'region') {
    if (active.value && !toolsBusy.value) void chooseTool(action);
  } else runMenuCommand(action);
}
onMounted(() => window.addEventListener('keydown', shortcut));
onUnmounted(() => window.removeEventListener('keydown', shortcut));
</script>
<template>
  <div class="app-shell">
    <WorkspaceTitleBar :desktop="desktop" :connected="!!snapshot" :picking="picking">
      <WorkspaceMenuBar :menus="menus" @command="runMenuCommand" />
    </WorkspaceTitleBar>
    <div class="tool-options" aria-label="工具选项">
      <WorkspaceIcon :name="tool === 'pan' ? 'move' : 'region'" />
      <strong>{{ tool === 'pan' ? '移动工具' : '区域选择' }}</strong>
      <span class="tool-help">{{
        tool === 'pan' ? '拖动画布 · 滚轮缩放' : '左键框选 / 调整 · 中键平移 · Esc 清空'
      }}</span>
      <span class="scope-indicator" :class="{ 'has-scope': !!selection.summary?.scope }">{{
        scopeLabel
      }}</span>
      <button :disabled="!snapshot" @click="resourceTab = 'tasks'">
        查看任务<span v-if="selection.total" class="tab-badge">{{ selection.total }}</span>
      </button>
    </div>
    <div v-if="error" class="notice error-notice" role="alert">
      <span>{{ error }}</span
      ><button @click="connect">重新连接</button
      ><button aria-label="关闭提示" @click="error = null">×</button>
    </div>
    <div v-if="snapshot?.notices.length" class="notice-list" aria-label="工作区通知">
      <div v-for="notice in snapshot.notices" :key="notice.id" class="notice" role="status">
        <details class="notice-detail">
          <summary :title="notice.message">{{ notice.message }}</summary>
          <p>{{ notice.message }}</p>
        </details>
        <button aria-label="关闭通知" @click="act({ kind: 'dismissNotice', noticeId: notice.id })">
          ×
        </button>
      </div>
    </div>
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
        <button
          class="tool-button"
          :class="{ 'tool-selected': tool === 'pan' }"
          title="移动工具（V）：清空当前选区并平移；保留选区请按住中键拖动"
          aria-label="平移画布"
          :aria-pressed="tool === 'pan'"
          :disabled="!active || toolsBusy"
          :aria-busy="!!pendingTool"
          @click="chooseTool('pan')"
        >
          <WorkspaceIcon name="move" /><span class="tool-key">V</span>
        </button>
        <button
          class="tool-button"
          :class="{ 'tool-selected': tool === 'region' }"
          title="区域选择（M）：框选任务范围，不裁剪原稿"
          aria-label="区域选择"
          :aria-pressed="tool === 'region'"
          :disabled="!active || toolsBusy"
          @click="chooseTool('region')"
        >
          <WorkspaceIcon name="region" /><span class="tool-key">M</span>
        </button>
        <span class="rail-divider"></span>
        <WorkspaceIcon name="lock" />
      </aside>
      <WorkspacePanels ref="panels">
        <template #canvas>
          <section class="workspace" aria-label="文档工作区">
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
                  {{ doc.name }}
                </button>
                <button
                  class="tab-close"
                  :aria-label="`关闭 ${doc.name}`"
                  @click="act({ kind: 'close', documentId: doc.id })"
                >
                  ×
                </button>
              </div>
              <span v-if="!snapshot?.documents.length" class="tab-placeholder">文档工作区</span>
            </nav>
            <PreviewCanvas
              v-if="active"
              ref="canvas"
              :key="`${active.id}:${active.revision}`"
              :width="active.width"
              :height="active.height"
              :name="active.name"
              :url="url"
              :view="views.get(active.id) ?? initialView()"
              :bounds="selectedBounds"
              :tool="tool"
              :region="snapshot?.selection.region ?? null"
              :context-key="selectionContext"
              :disabled="selection.busy || !!snapshot?.shuttingDown || picking"
              @update:view="views.set(active.id, $event)"
              @region="commitRegion"
              @clear-selection="clearCanvasSelection"
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
              <div class="empty-art"><WorkspaceIcon name="brand" /></div>
              <p class="eyebrow">只读设计工作区</p>
              <h1>从设计稿，开始实现</h1>
              <p>检查图层与样式，框选要处理的模块。<br />固定设计范围，继续探索下一处细节。</p>
              <button v-if="desktop" class="primary-button" :disabled="!canOpen" @click="open">
                打开 PSD 文件
              </button>
              <p v-else class="subtle">浏览器预览 · 请在桌面应用中继续</p>
              <p v-if="connecting" role="status">正在连接桌面工作区…</p>
              <div class="empty-steps">
                <span><b>01</b> 打开 PSD</span><span><b>02</b> 选择范围</span
                ><span><b>03</b> 固定任务</span>
              </div>
            </div>
          </section>
        </template>
        <template #inspector>
          <section class="inspector-dock" aria-label="检查面板">
            <DockTabs
              id="inspector"
              v-model="inspectorTab"
              label="属性与文档"
              :tabs="[
                { id: 'properties', label: '属性' },
                { id: 'document', label: '文档' },
              ]"
            />
            <div
              v-show="inspectorTab === 'properties'"
              id="inspector-panel-properties"
              class="dock-scroll"
              role="tabpanel"
              aria-labelledby="inspector-tab-properties"
              tabindex="0"
            >
              <LayerProperties
                :details="details"
                :loading="detailLoading"
                @page="readText"
                @locate="canvas?.locate()"
              />
            </div>
            <div
              v-show="inspectorTab === 'document'"
              id="inspector-panel-document"
              class="dock-scroll"
              role="tabpanel"
              aria-labelledby="inspector-tab-document"
              tabindex="0"
            >
              <section v-if="active" class="document-details">
                <h2 class="document-name">{{ active.name }}</h2>
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
                <button
                  class="wide-button"
                  :disabled="toolsBusy"
                  @click="act({ kind: 'reload', documentId: active.id })"
                >
                  从磁盘重新加载
                </button>
                <p class="subtle">文件改变后请主动重载；加载失败时保留已有内容。</p>
                <div class="sidebar-divider"></div>
                <h3>预览范围</h3>
                <p class="preview-note">{{ active.previewNote }}</p>
              </section>
              <p v-else class="panel-empty">
                打开 PSD 后查看尺寸、颜色模式与预览能力。原稿始终只读。
              </p>
            </div>
          </section>
        </template>
        <template #resources>
          <section class="resource-dock" aria-label="图层与任务面板">
            <DockTabs
              id="resources"
              v-model="resourceTab"
              label="图层与任务"
              :tabs="[
                { id: 'layers', label: '图层', badge: active?.layerCount ?? 0 },
                { id: 'tasks', label: '任务', badge: selection.total },
              ]"
            />
            <div class="selection-feedback" aria-live="polite">
              <p v-if="selection.busy" role="status">正在确认选区／任务操作…</p>
              <p v-else-if="selection.notice" role="status">{{ selection.notice }}</p>
              <p v-if="selection.error" class="layer-error" role="alert">{{ selection.error }}</p>
            </div>
            <div
              v-show="resourceTab === 'layers'"
              id="resources-panel-layers"
              class="layers-pane"
              role="tabpanel"
              aria-labelledby="resources-tab-layers"
            >
              <div v-if="layerError" class="layer-error" role="alert">
                {{ layerError }} <button @click="reloadLayers">重新读取</button>
              </div>
              <LayerTree
                v-if="active"
                :key="`${active.id}:${active.revision}`"
                :layers="layers"
                :selected="active.selectedLayerId"
                :range-ids="snapshot?.selection.layerIds ?? []"
                :range-busy="toolsBusy"
                :loading="layersLoading"
                :visible="resourceTab === 'layers'"
                :view="trees.get(active.id)?.view ?? initialTree()"
                @select="selectLayer"
                @toggle-range="selection.toggleLayer"
                @update:view="trees.set(active.id, { revision: active.revision, view: $event })"
              />
              <div v-else class="panel-empty">
                <WorkspaceIcon name="folder" />
                <p>还没有打开的文档</p>
                <span>打开 PSD 后，在此浏览和搜索图层。</span>
              </div>
            </div>
            <div
              v-show="resourceTab === 'tasks'"
              id="resources-panel-tasks"
              class="dock-scroll"
              role="tabpanel"
              aria-labelledby="resources-tab-tasks"
              tabindex="0"
            >
              <SelectionPanel v-if="snapshot" :model="selection" />
              <p v-else class="panel-empty">请在桌面应用中连接工作区后管理任务。</p>
            </div>
          </section>
        </template>
        <template #footer>
          <div class="sidebar-footer">
            <WorkspaceIcon name="lock" />不修改 PSD 原稿<span
              v-if="status.phase === 'ready'"
              class="app-version"
              >v{{ status.info.appVersion }}</span
            >
          </div>
        </template>
      </WorkspacePanels>
    </main>
  </div>
</template>
