<script setup lang="ts">
import { ref } from 'vue';
import { taskStatusLabel, type useSelection } from './useSelection';
import type { TaskDto } from '../../shared/api/generated';
const props = defineProps<{ model: ReturnType<typeof useSelection> }>();
const name = ref('设计任务');
const showReference = ref(false);
function copyTask(task: TaskDto) {
  showReference.value = true;
  void props.model.copyTask(task);
}
</script>

<template>
  <section class="selection-panel" aria-label="选区与固定任务">
    <h2>当前任务范围</h2>
    <p class="subtle">勾选图层，或用区域工具框选模块，再固定为任务。</p>
    <template v-if="model.summary?.scope">
      <p>
        <template v-if="model.summary.region"
          >已框选区域 · {{ Math.round(model.summary.region.width * 100) / 100 }} ×
          {{ Math.round(model.summary.region.height * 100) / 100 }} px</template
        >
        <template v-else>已勾选 {{ model.summary.layerIds.length }} 项</template>
        · 包含 {{ model.summary.scope.targetCount }} 个图层
      </p>
      <p class="subtle">
        {{ model.summary.scope.documentName }} · {{ model.summary.scope.textLayerCount }} 个文字层
      </p>
      <p v-if="model.summary.scope.targetCount === 0" class="subtle">
        区域内没有可用图层，请调整范围后再固定任务。
      </p>
      <details class="technical-details">
        <summary>范围技术信息</summary>
        <p>
          修订 {{ model.summary.scope.documentRevision }} · 快照 #{{
            model.summary.scope.snapshotId
          }}
        </p>
        <p v-if="model.summary.region">
          左上角（{{ model.summary.region.x }}, {{ model.summary.region.y }}），单位为原稿像素。
        </p>
      </details>
      <button :disabled="model.busy" @click="model.clear">清空任务范围</button>
      <button :disabled="model.busy" @click="model.viewScope()">查看当前范围内容</button>
    </template>
    <p v-else class="subtle">尚未选择范围。选择后可以查看内容，并固定为任务。</p>
    <label class="task-name"
      >任务名称<input v-model="name" maxlength="256" :disabled="model.busy"
    /></label>
    <button
      class="primary-button"
      :disabled="model.busy || !model.summary?.scope?.targetCount || !name.trim()"
      @click="model.createTask(name)"
    >
      固定任务
    </button>
    <button :disabled="model.running" @click="model.sync">同步状态</button>
    <p class="subtle">固定后改选不影响任务 · 仅本次运行有效 · 尚未接入 AI 执行</p>
    <details v-if="model.reference" class="technical-details" :open="showReference">
      <summary>任务引用（供后续 Agent 接入）</summary>
      <label class="task-reference"
        >可手动复制<textarea :value="model.reference" readonly aria-label="任务引用" />
      </label>
    </details>
    <div class="panel-heading">
      <h3>固定任务</h3>
      <button :disabled="model.busy" @click="model.refreshTasks()">刷新任务</button>
    </div>
    <p v-if="!model.total" class="subtle">暂无固定任务。</p>
    <details v-else class="technical-details">
      <summary>任务记录 {{ model.total }} / {{ model.capacity }}</summary>
      <p>数量包含已释放和已失效任务。终态记录保留到退出软件；达到上限后需要重启软件。</p>
    </details>
    <ul class="task-list">
      <li v-for="task in model.tasks" :key="task.id">
        <strong>{{ task.name }}</strong>
        <span>{{ taskStatusLabel[task.status] }}</span>
        <p>{{ task.scope.documentName }} · {{ task.scope.targetCount }} 个图层</p>
        <details class="technical-details">
          <summary>任务技术信息</summary>
          <p>
            任务 #{{ task.id }} · 文档 #{{ task.scope.documentId }} · 修订
            {{ task.scope.documentRevision }} · 快照 #{{ task.scope.snapshotId }}
          </p>
        </details>
        <button :disabled="model.busy || task.status !== 'active'" @click="model.viewScope(task)">
          查看任务内容
        </button>
        <button :disabled="task.status !== 'active'" @click="copyTask(task)">复制引用</button>
        <button :disabled="model.busy || task.status !== 'active'" @click="model.releaseTask(task)">
          释放任务
        </button>
      </li>
    </ul>
    <button v-if="model.nextAfter" :disabled="model.busy" @click="model.refreshTasks(true)">
      加载更多任务
    </button>
    <section v-if="model.content" class="scope-content" aria-label="固定范围内容">
      <h3>{{ model.content.target.kind === 'task' ? '任务固定内容' : '当前范围内容' }}</h3>
      <p>
        {{ model.content.scope.documentName }}
      </p>
      <button :disabled="model.busy" @click="model.readContent()">读取首页</button>
      <template v-if="model.content.page">
        <p class="subtle">
          共 {{ model.content.page.layerCount }} 个目标／结构记录、{{
            model.content.page.textLayerCount
          }}
          个文字层；当前页 {{ model.content.page.layers.length }} 条。
        </p>
        <p v-if="model.content.page.warnings.length" class="subtle">
          能力限制：{{ model.content.page.warnings.join(' · ') }}
        </p>
        <article v-for="layer in model.content.page.layers" :key="layer.details.layer.id">
          <strong>{{ layer.details.layer.name || '（空名称）' }}</strong> ·
          {{ layer.role === 'target' ? '目标' : '结构引用（不扩展授权）' }}
          <p v-if="layer.intersection" class="subtle">
            {{ layer.intersection.kind === 'contained' ? '完全位于区域内' : '与区域部分相交' }}
            <template v-if="layer.intersection.kind === 'partial' && layer.details.text"
              >；文字保留完整原文，较长内容可继续翻页。</template
            >
          </p>
          <template v-if="layer.details.text"
            ><p class="subtle">
              UTF-16 {{ layer.details.text.start }}–{{ layer.details.text.end }} /
              {{ layer.details.text.totalLength }}
            </p>
            <pre>{{ layer.details.text.text }}</pre>
          </template>
          <details>
            <summary>原始属性与分段样式</summary>
            <pre>{{ JSON.stringify(layer.details, null, 2) }}</pre>
          </details>
        </article>
        <button
          v-if="model.content.page.nextCursor"
          :disabled="model.busy"
          @click="model.readContent(model.content.page.nextCursor)"
        >
          下一页范围内容
        </button>
      </template>
    </section>
  </section>
</template>

<style scoped>
.selection-panel {
  font-size: 12px;
}
.selection-panel h2 {
  font-size: 12px;
}
.selection-panel button {
  margin: 3px 5px 3px 0;
}
.task-name,
.task-reference {
  display: grid;
  gap: 6px;
  margin: 10px 0;
}
input,
textarea {
  width: 100%;
  box-sizing: border-box;
  padding: 7px;
  border: 1px solid var(--border);
  border-radius: 4px;
  font: inherit;
}
textarea {
  min-height: 66px;
  resize: vertical;
}
.task-list {
  list-style: none;
  padding: 0;
}
.technical-details {
  margin: 8px 0;
  font-size: 12px;
  color: var(--muted);
  overflow-wrap: anywhere;
}
.technical-details summary {
  cursor: pointer;
}
.task-list li,
.scope-content article {
  padding: 10px;
  margin-bottom: 8px;
  border: 1px solid var(--border);
  border-radius: 3px;
  background: var(--surface-input);
  overflow-wrap: anywhere;
}
.task-list li > span {
  display: block;
  font-size: 12px;
  margin-top: 4px;
}
.scope-content pre {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 240px;
  overflow: auto;
  font-size: 12px;
}
.scope-content summary {
  cursor: pointer;
}
</style>
