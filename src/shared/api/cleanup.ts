import { invoke } from '@tauri-apps/api/core';
import { IPC_PROTOCOL_VERSION } from './generated';
import type {
  CleanupOperation,
  CleanupPlanDto,
  CleanupReceiptDto,
  CleanupReply,
  CleanupStateDto,
} from './generated';

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null;
const decimal = (value: unknown): value is string =>
  typeof value === 'string' &&
  /^(0|[1-9][0-9]{0,19})$/.test(value) &&
  BigInt(value) <= 18446744073709551615n;
const id = (value: unknown): value is string => decimal(value) && value !== '0';
const nullableId = (value: unknown) => value === null || id(value);
const ids = (value: unknown, limit: number): value is string[] =>
  Array.isArray(value) &&
  value.length <= limit &&
  value.every(id) &&
  new Set(value).size === value.length;
const label = (value: unknown): value is string =>
  typeof value === 'string' && new TextEncoder().encode(value).length <= 256;

export function isCleanupState(value: unknown): value is CleanupStateDto {
  return (
    record(value) &&
    decimal(value.revision) &&
    ids(value.invalidatedTaskIds, 128) &&
    (value.revision !== '0' || value.invalidatedTaskIds.length === 0)
  );
}
function plan(value: unknown): value is CleanupPlanDto {
  if (
    !record(value) ||
    !id(value.planId) ||
    !id(value.documentId) ||
    !label(value.documentName) ||
    !nullableId(value.openRevision) ||
    !nullableId(value.selectionId) ||
    !ids(value.liveRevisions, 8) ||
    !ids(value.loadJobs, 4) ||
    !ids(value.unresolvedLoadJobs, 4) ||
    !ids(value.previewJobs, 4) ||
    !Array.isArray(value.tasks) ||
    value.tasks.length > 128 ||
    !value.tasks.every(
      (task) => record(task) && id(task.id) && label(task.name) && id(task.documentRevision),
    ) ||
    new Set(value.tasks.map((task) => task.id)).size !== value.tasks.length ||
    value.expiresInSeconds !== 300
  )
    return false;
  const { liveRevisions, unresolvedLoadJobs } = value;
  return (
    (value.openRevision === null || liveRevisions.includes(value.openRevision)) &&
    (value.selectionId === null || value.openRevision !== null) &&
    value.tasks.every((task) => liveRevisions.includes(task.documentRevision)) &&
    !value.loadJobs.some((job) => unresolvedLoadJobs.includes(job))
  );
}
function receipt(value: unknown): value is CleanupReceiptDto {
  return (
    record(value) &&
    id(value.planId) &&
    id(value.documentId) &&
    id(value.revision) &&
    ids(value.invalidatedTaskIds, 128)
  );
}

/** 逐字段检查完整预算与请求关联；确认只发送服务端计划标识，不能回传修改后的影响范围。 */
export async function cleanupRequest(
  sessionId: string,
  operation: CleanupOperation,
): Promise<CleanupReply> {
  const value: unknown = await invoke('cleanup_request', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION, sessionId, operation },
  });
  if (
    !record(value) ||
    value.sessionId !== sessionId ||
    new TextEncoder().encode(JSON.stringify(value)).length > 256 * 1024
  )
    throw new Error('清理响应无效、跨会话或超过预算。');
  if (operation.kind === 'prepare' && value.kind === 'prepared' && plan(value.plan)) {
    if (
      operation.target.kind === 'document' &&
      (value.plan.documentId !== operation.target.documentId ||
        value.plan.openRevision !== operation.target.documentRevision)
    )
      throw new Error('清理预检与目标文档修订不匹配。');
    return { kind: 'prepared', sessionId, plan: value.plan };
  }
  if (
    operation.kind !== 'prepare' &&
    value.kind === 'committed' &&
    receipt(value.receipt) &&
    value.receipt.planId === operation.planId
  )
    return { kind: 'committed', sessionId, receipt: value.receipt };
  if (
    operation.kind === 'cancel' &&
    value.kind === 'cancelled' &&
    value.planId === operation.planId
  )
    return { kind: 'cancelled', sessionId, planId: operation.planId };
  throw new Error('清理响应与请求不匹配。');
}
