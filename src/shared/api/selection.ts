import { invoke } from '@tauri-apps/api/core';
import { IPC_PROTOCOL_VERSION } from './generated';
import type { SelectionOperation, SelectionReply, SelectionTarget } from './generated';
import { isSelectionReply } from './selectionValidation';

export function sameTarget(a: SelectionTarget, b: SelectionTarget): boolean {
  return a.kind === 'snapshot' && b.kind === 'snapshot'
    ? a.snapshotId === b.snapshotId
    : a.kind === 'task' && b.kind === 'task' && a.taskId === b.taskId;
}
/** 校验完整回复和请求关联；不能把跨会话或另一任务的内容作为当前结果。 */
export async function selectionRequest(
  sessionId: string,
  operation: SelectionOperation,
): Promise<SelectionReply> {
  const value: unknown = await invoke('selection_request', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION, sessionId, operation },
  });
  if (
    !isSelectionReply(value) ||
    new TextEncoder().encode(JSON.stringify(value)).length > 256 * 1024
  )
    throw new Error('选区响应无效或超过预算。');
  const responseSession =
    value.kind === 'tasks' || value.kind === 'content' ? value.page.sessionId : value.sessionId;
  let matches = responseSession === sessionId;
  switch (operation.kind) {
    case 'layers':
    case 'region':
    case 'clear':
      matches &&=
        value.kind === 'committed' &&
        BigInt(value.selectionRevision) >= BigInt(operation.expectedRevision) &&
        (operation.kind === 'clear' ? value.snapshotId === null : value.snapshotId !== null);
      break;
    case 'createTask':
      matches &&=
        value.kind === 'task' &&
        value.task.name === operation.name &&
        value.task.scope.snapshotId === operation.snapshotId;
      break;
    case 'releaseTask':
      matches &&=
        value.kind === 'task' &&
        value.task.id === operation.taskId &&
        value.task.status === 'released';
      break;
    case 'tasks':
      matches &&=
        value.kind === 'tasks' &&
        value.page.tasks.length <= operation.limit &&
        value.page.tasks.every(
          (task) => operation.after === null || BigInt(task.id) > BigInt(operation.after),
        );
      break;
    case 'content': {
      matches &&=
        value.kind === 'content' &&
        sameTarget(value.target, operation.target) &&
        value.page.layers.length <= operation.limit;
      if (value.kind === 'content') {
        const next = value.page.nextCursor,
          before = operation.cursor;
        if (operation.target.kind === 'snapshot')
          matches &&= value.page.snapshotId === operation.target.snapshotId;
        if (before)
          matches &&=
            value.page.snapshotId === before.snapshotId &&
            before.sessionId === sessionId &&
            (!next ||
              next.record > before.record ||
              (next.record === before.record && next.textStart > before.textStart));
      }
      break;
    }
  }
  if (!matches) throw new Error('选区响应与会话、任务或分页请求不匹配。');
  return value;
}
