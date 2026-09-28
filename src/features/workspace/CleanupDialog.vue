<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue';
import type { useCleanup } from './useCleanup';
const props = defineProps<{ model: ReturnType<typeof useCleanup> }>();
const dialog = ref<HTMLDialogElement | null>(null);
const viewError = ref<string | null>(null);
watch(
  [() => props.model.opened, dialog],
  ([opened, element]) => {
    if (!element) return;
    if (opened && !element.open) {
      try {
        element.showModal();
        viewError.value = null;
      } catch (cause: unknown) {
        viewError.value = `无法打开确认窗口：${cause instanceof Error ? cause.message : '对话框不可用'}`;
      }
    } else if (!opened && element.open) element.close();
  },
  { flush: 'post' },
);
onBeforeUnmount(() => {
  if (dialog.value?.open) dialog.value.close();
});
</script>

<template>
  <p v-if="viewError && model.opened" role="alert">
    {{ viewError }} <button :disabled="model.busy" @click="model.cancel">取消清理</button>
  </p>
  <dialog
    ref="dialog"
    class="cleanup-dialog"
    aria-labelledby="cleanup-title"
    aria-describedby="cleanup-description"
    @cancel.prevent="model.cancel"
    @keydown.stop
  >
    <h2 id="cleanup-title" tabindex="-1" autofocus>清理文档资源</h2>
    <p id="cleanup-description">
      这是整份文档的资源清理，不是只释放某一个任务。PSD 源文件和已导出文件不会被修改或删除。
    </p>
    <p v-if="model.state?.kind === 'preparing'" role="status">正在预检受影响范围…</p>
    <template v-if="model.plan">
      <p>
        <strong>{{ model.plan.documentName || '未命名文档' }}</strong> · 文档 #{{
          model.plan.documentId
        }}
      </p>
      <ul class="cleanup-summary">
        <li>
          {{
            model.plan.openRevision
              ? `将关闭此文档标签（修订 ${model.plan.openRevision}）`
              : '标签已关闭，清理其保留资源'
          }}
        </li>
        <li>
          存活修订：{{ model.plan.liveRevisions.join('、') || '无' }}；{{
            model.plan.selectionId ? '当前范围将失效' : '没有当前范围'
          }}
        </li>
        <li>将失效 {{ model.plan.tasks.length }} 个固定任务</li>
        <li>关联加载／重载作业：{{ model.plan.loadJobs.join('、') || '无' }}</li>
        <li>关联预览作业：{{ model.plan.previewJobs.join('、') || '无' }}</li>
      </ul>
      <details v-if="model.plan.tasks.length" open>
        <summary>全部受影响任务（{{ model.plan.tasks.length }}）</summary>
        <ul class="cleanup-tasks">
          <li v-for="task in model.plan.tasks" :key="task.id">
            {{ task.name }} · #{{ task.id }} · 修订 {{ task.documentRevision }}
          </li>
        </ul>
      </details>
      <p v-if="model.plan.unresolvedLoadJobs.length" class="cleanup-warning">
        潜在路径别名作业：{{
          model.plan.unresolvedLoadJobs.join('、')
        }}。路径尚未解析，只有最终命中此文档的作业才会取消；无关文件不受影响。
      </p>
      <p class="subtle">
        计划有效期为 5
        分钟。范围变化需要重新确认；不会自动扩大清理范围。其他文档和同路径重新打开的新文档不受旧文档清理影响。
      </p>
    </template>
    <p v-if="model.state?.kind === 'complete'" role="status">
      清理已生效，相关任务已失效。正在执行的同步计算和外部引用释放后才会归还资源，内存不保证立即归零。
    </p>
    <p v-if="model.error" role="alert" class="cleanup-warning">{{ model.error }}</p>
    <footer>
      <button :disabled="model.busy" @click="model.cancel">
        {{ model.state?.kind === 'complete' ? '关闭' : '取消' }}
      </button>
      <button
        v-if="model.state?.kind === 'failed' || model.state?.kind === 'stale'"
        :disabled="model.busy"
        @click="model.recheck"
      >
        重新预检
      </button>
      <button
        v-if="model.state?.kind === 'complete' && model.error"
        :disabled="model.busy"
        @click="model.synchronize"
      >
        重试同步
      </button>
      <button
        v-if="
          model.state?.kind === 'ready' ||
          model.state?.kind === 'uncertain' ||
          model.state?.kind === 'confirming'
        "
        class="cleanup-confirm"
        :disabled="model.busy"
        @click="model.confirm"
      >
        {{
          model.state.kind === 'uncertain'
            ? '重试原计划'
            : model.state.kind === 'confirming'
              ? '正在清理…'
              : '确认清理并使任务失效'
        }}
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.cleanup-dialog {
  width: min(560px, calc(100vw - 48px));
  max-height: calc(100vh - 64px);
  overflow: auto;
  padding: 22px;
  color: #e4e5e9;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  font-size: 13px;
  line-height: 1.7;
}
.cleanup-dialog::backdrop {
  background: rgb(0 0 0 / 55%);
}
.cleanup-dialog h2 {
  margin-top: 0;
  font-size: 17px;
}
.cleanup-summary,
.cleanup-tasks {
  padding-left: 22px;
}
.cleanup-tasks {
  max-height: 180px;
  overflow: auto;
  overflow-wrap: anywhere;
}
.cleanup-warning {
  color: #f0c88f;
}
.cleanup-dialog footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 20px;
  flex-wrap: wrap;
}
.cleanup-confirm {
  border-color: #aa7956;
}
</style>
