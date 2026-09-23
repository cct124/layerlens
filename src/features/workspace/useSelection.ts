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
      (acknowledgement.value?.sessionId === summary.value?.sessionId &&
        BigInt(summary.value?.selectionRevision ?? '0') <
          BigInt(acknowledgement.value?.revision ?? '0')),
  );
  let disposed = false,
    refreshWanted = false,
    contentGeneration = 0;
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
    const result = await selectionRequest(sessionId, {
      kind: 'tasks',
      after: more ? nextAfter.value : null,
      limit: 32,
    });
    if (!live(sessionId) || result.kind !== 'tasks') return;
    if (!more) tasks.value = [];
    for (const task of result.page.tasks) upsert(task);
    total.value = result.page.total;
    capacity.value = result.page.capacity;
    nextAfter.value = result.page.nextAfter;
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
      notice.value = `任务 #${result.task.id} 已确认（${taskStatusLabel[result.task.status]}）。`;
      reference.value =
        result.task.status === 'active'
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
    if (!sessionId || task.status !== 'active') return;
    const value = JSON.stringify({ sessionId, taskId: task.id });
    reference.value = value;
    try {
      if (!navigator.clipboard) throw new Error('当前环境不支持剪贴板');
      await navigator.clipboard.writeText(value);
      if (live(sessionId)) notice.value = '已复制当前会话的任务引用。';
    } catch (cause: unknown) {
      if (live(sessionId)) error.value = `复制失败，请从引用框手动复制：${workspaceMessage(cause)}`;
    }
  }
  async function readContent(cursor: SelectionCursorDto | null = null) {
    const view = content.value,
      generation = contentGeneration;
    if (!view || running.value) return;
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
    if (busy.value || (task && task.status !== 'active')) return;
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
    { immediate: true },
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
    sync,
  });
}
