import type {
  ScopeDto,
  SelectionContentDto,
  SelectionReply,
  SelectionSummaryDto,
  TaskDto,
  TaskPageDto,
  SelectionCursorDto,
  SelectionTarget,
} from './generated';
import { isLayerDetails } from './layerValidation';

const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null;
const uint = (v: unknown): v is number =>
  typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= 4294967295;
const decimal = (v: unknown): v is string =>
  typeof v === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(v) && BigInt(v) <= 18446744073709551615n;
const id = (v: unknown): v is string => decimal(v) && v !== '0';
const nullableId = (v: unknown) => v === null || id(v);
const session = (v: unknown): v is string => typeof v === 'string' && /^[0-9a-f]{32}$/.test(v);
const shortString = (v: unknown): v is string =>
  typeof v === 'string' && new TextEncoder().encode(v).length <= 256;
const bounds = (v: unknown) =>
  record(v) &&
  [v.x, v.y, v.width, v.height].every((n) => typeof n === 'number' && Number.isFinite(n)) &&
  typeof v.width === 'number' &&
  v.width > 0 &&
  typeof v.height === 'number' &&
  v.height > 0;

export function isScope(v: unknown): v is ScopeDto {
  return (
    record(v) &&
    id(v.snapshotId) &&
    id(v.documentId) &&
    id(v.documentRevision) &&
    shortString(v.documentName) &&
    uint(v.targetCount) &&
    uint(v.contentCount) &&
    uint(v.textLayerCount) &&
    v.textLayerCount <= v.targetCount &&
    v.targetCount <= v.contentCount &&
    v.contentCount <= 4096
  );
}
export function isSelectionSummary(v: unknown): v is SelectionSummaryDto {
  if (
    !record(v) ||
    !session(v.sessionId) ||
    !decimal(v.selectionRevision) ||
    !nullableId(v.documentId) ||
    !nullableId(v.documentRevision) ||
    !Array.isArray(v.layerIds) ||
    v.layerIds.length > 4096 ||
    !v.layerIds.every(uint) ||
    new Set(v.layerIds).size !== v.layerIds.length ||
    (v.region !== null && !bounds(v.region))
  )
    return false;
  if ((v.documentId === null) !== (v.documentRevision === null)) return false;
  if (v.scope === null) return v.layerIds.length === 0 && v.region === null;
  return (
    isScope(v.scope) &&
    v.scope.documentId === v.documentId &&
    v.scope.documentRevision === v.documentRevision &&
    (v.region === null ? v.layerIds.length > 0 : v.layerIds.length === 0)
  );
}
function task(v: unknown): v is TaskDto {
  return (
    record(v) &&
    id(v.id) &&
    shortString(v.name) &&
    v.name.trim().length > 0 &&
    (v.status === 'active' || v.status === 'released' || v.status === 'invalidated') &&
    isScope(v.scope) &&
    v.scope.targetCount > 0
  );
}
function tasks(v: unknown): v is TaskPageDto {
  if (
    !record(v) ||
    !session(v.sessionId) ||
    !Array.isArray(v.tasks) ||
    v.tasks.length > 32 ||
    !v.tasks.every(task) ||
    !nullableId(v.nextAfter) ||
    !uint(v.total) ||
    !uint(v.capacity) ||
    v.total > v.capacity ||
    v.tasks.length > v.total
  )
    return false;
  const items = v.tasks;
  return (
    items.every((item, i) => i === 0 || BigInt(item.id) > BigInt(items[i - 1]!.id)) &&
    (v.nextAfter === null || (items.length > 0 && items.at(-1)?.id === v.nextAfter))
  );
}
function cursor(v: unknown): v is SelectionCursorDto {
  return (
    record(v) && session(v.sessionId) && id(v.snapshotId) && uint(v.record) && uint(v.textStart)
  );
}
function content(v: unknown): v is SelectionContentDto {
  if (
    !record(v) ||
    !session(v.sessionId) ||
    !id(v.snapshotId) ||
    !id(v.documentId) ||
    !id(v.documentRevision) ||
    !uint(v.layerCount) ||
    v.layerCount > 4096 ||
    !uint(v.textLayerCount) ||
    v.textLayerCount > v.layerCount ||
    !Array.isArray(v.layers) ||
    v.layers.length > 128 ||
    typeof v.truncated !== 'boolean' ||
    !Array.isArray(v.warnings) ||
    v.warnings.length > 3 ||
    !v.warnings.every((w) =>
      ['geometricBoundsOnly', 'noReferenceBounds', 'pixelDependenciesUnresolved'].includes(w),
    )
  )
    return false;
  if (
    !(v.nextCursor === null
      ? !v.truncated
      : cursor(v.nextCursor) &&
        v.truncated &&
        v.nextCursor.sessionId === v.sessionId &&
        v.nextCursor.snapshotId === v.snapshotId &&
        v.nextCursor.record < v.layerCount)
  )
    return false;
  const seen = new Set<number>();
  let previous = -1;
  for (const layer of v.layers) {
    if (
      !record(layer) ||
      !uint(layer.stackIndex) ||
      layer.stackIndex <= previous ||
      !['target', 'structure'].includes(String(layer.role)) ||
      !isLayerDetails(layer.details) ||
      seen.has(layer.details.layer.id)
    )
      return false;
    if (
      layer.intersection !== null &&
      !(
        record(layer.intersection) &&
        ['contained', 'partial'].includes(String(layer.intersection.kind)) &&
        bounds(layer.intersection.bounds)
      )
    )
      return false;
    seen.add(layer.details.layer.id);
    previous = layer.stackIndex;
  }
  return v.layers.length <= v.layerCount && (!v.truncated || v.layers.length > 0);
}
function target(v: unknown): v is SelectionTarget {
  return (
    record(v) && (v.kind === 'snapshot' ? id(v.snapshotId) : v.kind === 'task' && id(v.taskId))
  );
}
export function isSelectionReply(v: unknown): v is SelectionReply {
  if (!record(v)) return false;
  switch (v.kind) {
    case 'committed':
      return session(v.sessionId) && decimal(v.selectionRevision) && nullableId(v.snapshotId);
    case 'task':
      return session(v.sessionId) && task(v.task);
    case 'tasks':
      return tasks(v.page);
    case 'content':
      return target(v.target) && content(v.page);
    default:
      return false;
  }
}
