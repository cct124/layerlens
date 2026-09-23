import { IPC_PROTOCOL_VERSION } from './generated';
import { isSelectionSummary } from './selectionValidation';
import type {
  WorkspaceDocument,
  WorkspaceError,
  WorkspaceErrorCode,
  WorkspaceJobPhase,
  WorkspacePreview,
  WorkspaceSnapshot,
} from './generated';

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
const decimal = (value: unknown): value is string =>
  typeof value === 'string' &&
  /^(0|[1-9][0-9]{0,19})$/.test(value) &&
  BigInt(value) <= 18446744073709551615n;
const id = (value: unknown): value is string => decimal(value) && value !== '0';
const integer = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
const jobId = (value: unknown): value is string =>
  typeof value === 'string' && /^(open|preview)-[1-9][0-9]{0,19}$/.test(value);
const phase = (value: unknown): value is WorkspaceJobPhase =>
  value === 'queued' || value === 'running' || value === 'cancelling' || value === 'finishing';

const errorCodes: Record<WorkspaceErrorCode, true> = {
  PROTOCOL_MISMATCH: true,
  INVALID_INPUT: true,
  NOT_FOUND: true,
  BUSY: true,
  RESOURCE_LIMIT: true,
  OPEN_FAILED: true,
  PREVIEW_FAILED: true,
  STALE_PREVIEW: true,
  STALE_REVISION: true,
  FOREIGN_SESSION: true,
  STALE_SELECTION: true,
  INACTIVE_DOCUMENT: true,
  SNAPSHOT_EXPIRED: true,
  EMPTY_TARGETS: true,
  TASK_RELEASED: true,
  TASK_INVALIDATED: true,
  DOCUMENT_INVALIDATED: true,
  STALE_CLEANUP_PLAN: true,
  REQUEST_CONFLICT: true,
  SHUTTING_DOWN: true,
  INTERNAL: true,
};

export function isWorkspaceError(value: unknown): value is WorkspaceError {
  return (
    record(value) &&
    typeof value.message === 'string' &&
    typeof value.code === 'string' &&
    Object.hasOwn(errorCodes, value.code)
  );
}

function document(value: unknown): value is WorkspaceDocument {
  return (
    record(value) &&
    id(value.id) &&
    id(value.revision) &&
    typeof value.name === 'string' &&
    typeof value.path === 'string' &&
    integer(value.width) &&
    value.width > 0 &&
    integer(value.height) &&
    value.height > 0 &&
    integer(value.layerCount) &&
    typeof value.colorMode === 'string' &&
    integer(value.bitDepth) &&
    typeof value.previewNote === 'string' &&
    (value.selectedLayerId === null ||
      (integer(value.selectedLayerId) && value.selectedLayerId <= 4294967295))
  );
}

function preview(value: unknown): value is WorkspacePreview {
  if (!record(value) || !id(value.documentId) || !id(value.revision) || !record(value.state))
    return false;
  const state = value.state;
  switch (state.phase) {
    case 'pending':
      return (state.jobId === null || jobId(state.jobId)) && phase(state.status);
    case 'ready':
      return typeof state.cacheHit === 'boolean';
    case 'failed':
      return isWorkspaceError(state.error);
    case 'cancelled':
      return true;
    default:
      return false;
  }
}

function resources(value: unknown): boolean {
  return (
    record(value) &&
    decimal(value.sourceBytes) &&
    decimal(value.decodedBytes) &&
    decimal(value.outputBytes) &&
    decimal(value.cacheBytes)
  );
}

/** 校验 IPC 形状和文档／活动预览关联，失配响应不能覆盖最后一份有效状态。 */
export function isWorkspaceSnapshot(value: unknown): value is WorkspaceSnapshot {
  if (
    !record(value) ||
    value.protocolVersion !== IPC_PROTOCOL_VERSION ||
    !decimal(value.sequence) ||
    !isSelectionSummary(value.selection) ||
    !Array.isArray(value.documents) ||
    !value.documents.every(document) ||
    !(value.activeDocumentId === null || id(value.activeDocumentId)) ||
    !Array.isArray(value.jobs) ||
    !value.jobs.every(
      (j) => record(j) && jobId(j.id) && typeof j.label === 'string' && phase(j.phase),
    ) ||
    !Array.isArray(value.notices) ||
    value.notices.length > 16 ||
    !value.notices.every((n) => record(n) && id(n.id) && typeof n.message === 'string') ||
    !resources(value.resources) ||
    typeof value.shuttingDown !== 'boolean' ||
    !(value.preview === null || preview(value.preview))
  )
    return false;
  const active = value.documents.find((d) => d.id === value.activeDocumentId);
  return (
    new Set(value.documents.map((d) => d.id)).size === value.documents.length &&
    value.selection.documentId === value.activeDocumentId &&
    (value.selection.documentRevision === null
      ? !active
      : value.selection.documentRevision === active?.revision) &&
    (value.activeDocumentId === null ? value.documents.length === 0 : !!active) &&
    (value.preview === null ||
      (active?.id === value.preview.documentId && active.revision === value.preview.revision))
  );
}
