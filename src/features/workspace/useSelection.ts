import { computed, onScopeDispose, reactive, ref, shallowRef, watch } from 'vue';
import type { Ref } from 'vue';
import type {
  ScopeDto,
  SelectionBounds,
  SelectionContentDto,
  SelectionCursorDto,
  SelectionSummaryDto,
  SelectionTarget,
  TaskDto,
  TaskStatusDto,
  CleanupStateDto,
  CleanupReceiptDto,
} from '../../shared/api/generated';
import { selectionRequest } from '../../shared/api/selection';
import { workspaceMessage } from '../../shared/api/workspace';

export const taskStatusLabel: Record<TaskStatusDto, string> = {
  active: '已固定',
  released: '已释放',
  invalidated: '已失效',
};

interface ContentView {
  sessionId: string;
  target: SelectionTarget;
  scope: ScopeDto;
  page: SelectionContentDto | null;
}

/** 仅保存已确认的任务投影和展示目标；当前选区始终取工作区权威摘要。所有请求串行。 */
export function useSelection(
  summary: Readonly<Ref<SelectionSummaryDto | null>>,
  sync: () => Promise<void>,
  cleanupState: Readonly<Ref<CleanupStateDto | null>> = ref(null),
  blocked: Readonly<Ref<boolean>> = ref(false),
) {
  const running = ref(false),
    error = ref<string | null>(null),
    notice = ref<string | null>(null);
  const tasks = shallowRef<TaskDto[]>([]),
    total = ref(0),
    capacity = ref(128),
    nextAfter = ref<string | null>(null);
  const content = shallowRef<ContentView | null>(null),
    reference = ref<string | null>(null);
  const acknowledgement = shallowRef<{ sessionId: string; revision: string } | null>(null);
  const busy = computed(
    () =>
      running.value ||
      blocked.value ||
      (acknowledgement.value?.sessionId === summary.value?.sessionId &&
        BigInt(summary.value?.selectionRevision ?? '0') <
          BigInt(acknowledgement.value?.revision ?? '0')),
  );
  let disposed = false,
    refreshWanted = false,
    contentGeneration = 0;
  const invalidatedIds = new Set<string>();
  let pendingCreate: {
    sessionId: string;
    snapshotId: string;
    name: string;
    requestId: string;
  } | null = null;

  async function run(action: () => Promise<void>, stillRelevant = () => true) {
    if (running.value || disposed) return;
    const sessionId = summary.value?.sessionId;
    running.value = true;
    error.value = null;
    try {
      await action();
    } catch (cause: unknown) {
      if (!disposed && summary.value?.sessionId === sessionId && stillRelevant())
        error.value = workspaceMessage(cause);
    } finally {
      running.value = false;
      if (!disposed && refreshWanted) void refreshTasks();
    }
  }
  const live = (session: string) => !disposed && summary.value?.sessionId === session;
  function upsert(task: TaskDto) {
    const previous = tasks.value.find((item) => item.id === task.id);
    // 核心终态不回退；旧列表／创建回执不得绕过清理投影恢复任务。
    const status =
      previous && previous.status !== 'active'
        ? previous.status
        : invalidatedIds.has(task.id)
          ? 'invalidated'
          : task.status;
    task = { ...task, status };
    tasks.value = [...tasks.value.filter((t) => t.id !== task.id), task].sort((a, b) =>
      BigInt(a.id) > BigInt(b.id) ? -1 : 1,
    );
    if (task.status !== 'active') {
      if (
        reference.value === JSON.stringify({ sessionId: summary.value?.sessionId, taskId: task.id })
      )
        reference.value = null;
      if (content.value?.target.kind === 'task' && content.value.target.taskId === task.id) {
        contentGeneration++;
        content.value = null;
      }
    }
  }
  async function loadTasks(sessionId: string, more: boolean) {
    const through = tasks.value.reduce(
      (max, task) => (BigInt(task.id) > max ? BigInt(task.id) : max),
      0n,
    );
    let after = more ? nextAfter.value : null;
    // 刷新覆盖此前已加载范围（最多 128 条／4 页），不只刷新第一页。
    for (let page = 0; page < 4; page++) {
      const result = await selectionRequest(sessionId, { kind: 'tasks', after, limit: 32 });
      if (!live(sessionId) || result.kind !== 'tasks') return;
      for (const task of result.page.tasks) upsert(task);
      total.value = result.page.total;
      capacity.value = result.page.capacity;
      nextAfter.value = result.page.nextAfter;
      if (more || !result.page.nextAfter || BigInt(result.page.nextAfter) >= through) return;
      after = result.page.nextAfter;
    }
  }
  async function refreshTasks(more = false) {
    if (running.value) {
      if (!more) refreshWanted = true;
      return;
    }
    refreshWanted = false;
    const sessionId = summary.value?.sessionId;
    if (sessionId) await run(() => loadTasks(sessionId, more));
  }
  /** 返回核心确认的选择版本；失败、跳过或过时结果返回 null，显示仍等待权威摘要。 */
  async function changeScope(
    change:
      | { kind: 'layers'; layerIds: number[] }
      | { kind: 'region'; bounds: SelectionBounds }
      | { kind: 'clear' },
  ): Promise<string | null> {
    const current = summary.value;
    if (busy.value || !current?.documentId || !current.documentRevision) return null;
    let committedRevision: string | null = null;
    const base = {
      documentId: current.documentId,
      documentRevision: current.documentRevision,
      expectedRevision: current.selectionRevision,
    };
    const sameDocument = () =>
      live(current.sessionId) &&
      summary.value?.documentId === current.documentId &&
      summary.value?.documentRevision === current.documentRevision;
    await run(
      async () => {
        const result = await selectionRequest(current.sessionId, { ...base, ...change });
        if (
          !sameDocument() ||
          result.kind !== 'committed' ||
          BigInt(summary.value?.selectionRevision ?? '0') > BigInt(result.selectionRevision)
        )
          return;
        acknowledgement.value = {
          sessionId: current.sessionId,
          revision: result.selectionRevision,
        };
        committedRevision = result.selectionRevision;
        notice.value =
          change.kind === 'clear'
            ? '已清空当前范围，既有任务不受影响。'
            : '范围已确认，可以查看内容或固定为任务。';
        await sync();
      },
      () => sameDocument() && summary.value?.selectionRevision === current.selectionRevision,
    );
    return committedRevision;
  }
  function changeLayers(ids: number[]) {
    return changeScope(ids.length ? { kind: 'layers', layerIds: ids } : { kind: 'clear' });
  }
  function toggleLayer(id: number) {
    const ids = summary.value?.layerIds ?? [];
    return changeLayers(ids.includes(id) ? ids.filter((item) => item !== id) : [...ids, id]);
  }
  async function createTask(name: string) {
    const current = summary.value;
    if (busy.value || !current?.scope || current.scope.targetCount === 0) return;
    const snapshotId = current.scope.snapshotId;
    await run(async () => {
      // 不确定是否成功时用原请求 ID 重试；名称或目标变化才建立新意图。
      if (
        !pendingCreate ||
        pendingCreate.sessionId !== current.sessionId ||
        pendingCreate.snapshotId !== snapshotId ||
        pendingCreate.name !== name
      ) {
        pendingCreate = {
          sessionId: current.sessionId,
          snapshotId,
          name,
          requestId: crypto.randomUUID(),
        };
      }
      const intent = pendingCreate;
      const result = await selectionRequest(intent.sessionId, {
        kind: 'createTask',
        snapshotId,
        name,
        requestId: intent.requestId,
      });
      if (!live(intent.sessionId) || result.kind !== 'task') return;
      pendingCreate = null;
      upsert(result.task);
      const confirmed = tasks.value.find((task) => task.id === result.task.id) ?? result.task;
      notice.value = `任务 #${result.task.id} 已确认（${taskStatusLabel[confirmed.status]}）。`;
      reference.value =
        confirmed.status === 'active'
          ? JSON.stringify({ sessionId: intent.sessionId, taskId: result.task.id })
          : null;
      try {
        await loadTasks(intent.sessionId, false);
        // 列表是稍后的权威结果，不能用创建回执把已释放／失效任务恢复为 active。
        if (live(intent.sessionId)) {
          const latest = tasks.value.find((task) => task.id === result.task.id);
          if (!latest) upsert(result.task);
          notice.value = `任务 #${result.task.id} 已确认（${taskStatusLabel[(latest ?? result.task).status]}）。`;
        }
      } catch (cause: unknown) {
        if (live(intent.sessionId))
          error.value = `任务已确认，列表刷新失败：${workspaceMessage(cause)}`;
      }
    });
  }
  async function releaseTask(task: TaskDto) {
    const sessionId = summary.value?.sessionId;
    if (busy.value || !sessionId) return;
    await run(async () => {
      const result = await selectionRequest(sessionId, { kind: 'releaseTask', taskId: task.id });
      if (!live(sessionId) || result.kind !== 'task') return;
      upsert(result.task);
      notice.value = `任务 #${task.id} ${taskStatusLabel[result.task.status]}，不再接受新的读取。`;
    });
  }
  async function copyTask(task: TaskDto) {
    const sessionId = summary.value?.sessionId;
    if (busy.value || !sessionId || task.status !== 'active' || invalidatedIds.has(task.id)) return;
    const value = JSON.stringify({ sessionId, taskId: task.id });
    reference.value = value;
    try {
      if (!navigator.clipboard) throw new Error('当前环境不支持剪贴板');
      await navigator.clipboard.writeText(value);
      if (live(sessionId) && !invalidatedIds.has(task.id))
        notice.value = '已复制当前会话的任务引用。';
    } catch (cause: unknown) {
      if (live(sessionId)) error.value = `复制失败，请从引用框手动复制：${workspaceMessage(cause)}`;
    }
  }
  async function readContent(cursor: SelectionCursorDto | null = null) {
    const view = content.value,
      generation = contentGeneration;
    if (!view || running.value || blocked.value) return;
    await run(
      async () => {
        const result = await selectionRequest(view.sessionId, {
          kind: 'content',
          target: view.target,
          cursor,
          limit: 16,
        });
        if (!live(view.sessionId) || generation !== contentGeneration || result.kind !== 'content')
          return;
        if (
          result.page.snapshotId !== view.scope.snapshotId ||
          result.page.documentId !== view.scope.documentId ||
          result.page.documentRevision !== view.scope.documentRevision
        )
          throw new Error('内容与固定任务范围不一致。');
        content.value = { ...view, page: result.page };
      },
      () => generation === contentGeneration,
    );
  }
  function viewScope(task?: TaskDto) {
    if (busy.value || (task && (task.status !== 'active' || invalidatedIds.has(task.id)))) return;
    const current = summary.value,
      scope = task?.scope ?? current?.scope;
    if (!current || !scope) return;
    contentGeneration++;
    content.value = {
      sessionId: current.sessionId,
      scope,
      target: task
        ? { kind: 'task', taskId: task.id }
        : { kind: 'snapshot', snapshotId: scope.snapshotId },
      page: null,
    };
    void readContent();
  }
  watch(
    () => summary.value?.sessionId,
    () => {
      invalidatedIds.clear();
      tasks.value = [];
      total.value = 0;
      nextAfter.value = null;
      content.value = null;
      reference.value = null;
      pendingCreate = null;
      acknowledgement.value = null;
      contentGeneration++;
      if (summary.value) {
        refreshWanted = true;
        void refreshTasks();
      }
    },
    { immediate: true, flush: 'sync' },
  );
  function applyInvalidated(ids: string[]) {
    for (const id of ids) invalidatedIds.add(id);
    for (const task of tasks.value)
      if (invalidatedIds.has(task.id)) upsert({ ...task, status: 'invalidated' });
    const target = content.value?.target;
    if (target?.kind === 'task' && invalidatedIds.has(target.taskId)) {
      contentGeneration++;
      content.value = null;
    }
    for (const id of invalidatedIds) {
      if (reference.value === JSON.stringify({ sessionId: summary.value?.sessionId, taskId: id }))
        reference.value = null;
    }
  }
  /** 来自核心确认回执的局部投影；不等待分页刷新，也不自行推算任务影响。 */
  function applyCleanup(receipt: CleanupReceiptDto) {
    applyInvalidated(receipt.invalidatedTaskIds);
    if (content.value?.scope.documentId === receipt.documentId) {
      contentGeneration++;
      content.value = null;
    }
    notice.value = '文档资源已清理，关联任务已失效；其他文档和普通释放记录不受影响。';
  }
  watch(
    () => [summary.value?.sessionId, cleanupState.value] as const,
    () => {
      applyInvalidated(cleanupState.value?.invalidatedTaskIds ?? []);
    },
    { immediate: true, flush: 'sync' },
  );
  watch(
    () => summary.value?.scope?.snapshotId,
    (id) => {
      if (content.value?.target.kind === 'snapshot' && content.value.scope.snapshotId !== id) {
        contentGeneration++;
        content.value = null;
      }
    },
  );
  onScopeDispose(() => {
    disposed = true;
    contentGeneration++;
    refreshWanted = false;
  });
  return reactive({
    summary,
    running,
    busy,
    error,
    notice,
    tasks,
    total,
    capacity,
    nextAfter,
    content,
    reference,
    toggleLayer,
    selectRegion: (bounds: SelectionBounds) => changeScope({ kind: 'region', bounds }),
    clear: () => changeLayers([]),
    createTask,
    releaseTask,
    copyTask,
    viewScope,
    readContent,
    refreshTasks,
    applyCleanup,
    sync,
  });
}
