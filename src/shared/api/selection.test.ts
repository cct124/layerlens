import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { selectionRequest } from './selection';
import { isSelectionSummary } from './selectionValidation';
import { isWorkspaceError } from './workspaceValidation';
import type { SelectionReply } from './generated';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const sessionId = 'a'.repeat(32);
const page = {
  sessionId,
  snapshotId: '1',
  documentId: '1',
  documentRevision: '1',
  layerCount: 0,
  textLayerCount: 0,
  layers: [],
  warnings: [],
  nextCursor: null,
  truncated: false,
};
beforeEach(() => vi.resetAllMocks());
it('失效任务可列出和幂等释放，但不能把 active 或未知状态当作释放成功', async () => {
  const task = {
    id: '1',
    name: '模块',
    status: 'invalidated',
    scope: {
      snapshotId: '1',
      documentId: '1',
      documentRevision: '1',
      documentName: 'A.psd',
      targetCount: 1,
      contentCount: 1,
      textLayerCount: 0,
    },
  };
  const reply = { kind: 'task', sessionId, task };
  vi.mocked(invoke).mockResolvedValue(reply);
  await expect(selectionRequest(sessionId, { kind: 'releaseTask', taskId: '1' })).resolves.toEqual(
    reply,
  );
  const list = {
    kind: 'tasks',
    page: { sessionId, tasks: [task], total: 1, capacity: 128, nextAfter: null },
  };
  vi.mocked(invoke).mockResolvedValue(list);
  await expect(
    selectionRequest(sessionId, { kind: 'tasks', after: null, limit: 32 }),
  ).resolves.toEqual(list);
  for (const status of ['active', 'unknown']) {
    vi.mocked(invoke).mockResolvedValue({ ...reply, task: { ...task, status } });
    await expect(
      selectionRequest(sessionId, { kind: 'releaseTask', taskId: '1' }),
    ).rejects.toThrow();
  }
  for (const code of ['TASK_INVALIDATED', 'DOCUMENT_INVALIDATED', 'STALE_CLEANUP_PLAN'])
    expect(isWorkspaceError({ code, message: '资源已变化' })).toBe(true);
});
it('拒绝跨会话、错误操作回执与另一任务的内容', async () => {
  vi.mocked(invoke).mockResolvedValue({
    kind: 'committed',
    sessionId: 'b'.repeat(32),
    selectionRevision: '2',
    snapshotId: null,
  });
  await expect(
    selectionRequest(sessionId, {
      kind: 'clear',
      documentId: '1',
      documentRevision: '1',
      expectedRevision: '1',
    }),
  ).rejects.toThrow('不匹配');
  vi.mocked(invoke).mockResolvedValue({
    kind: 'content',
    target: { kind: 'task', taskId: '2' },
    page,
  });
  await expect(
    selectionRequest(sessionId, {
      kind: 'content',
      target: { kind: 'task', taskId: '1' },
      cursor: null,
      limit: 16,
    }),
  ).rejects.toThrow('不匹配');
  vi.mocked(invoke).mockResolvedValue({
    kind: 'committed',
    sessionId,
    selectionRevision: '2',
    snapshotId: null,
  });
  await expect(
    selectionRequest(sessionId, { kind: 'tasks', after: null, limit: 32 }),
  ).rejects.toThrow('不匹配');
});
it('成功内容必须带同一快照和可前进游标', async () => {
  const response: SelectionReply = {
    kind: 'content',
    target: { kind: 'snapshot', snapshotId: '1' },
    page,
  };
  vi.mocked(invoke).mockResolvedValue(response);
  expect(
    await selectionRequest(sessionId, {
      kind: 'content',
      target: { kind: 'snapshot', snapshotId: '1' },
      cursor: null,
      limit: 16,
    }),
  ).toEqual(response);
  vi.mocked(invoke).mockResolvedValue({ ...response, page: { ...page, snapshotId: '2' } });
  await expect(
    selectionRequest(sessionId, {
      kind: 'content',
      target: { kind: 'snapshot', snapshotId: '1' },
      cursor: null,
      limit: 16,
    }),
  ).rejects.toThrow('不匹配');
});
it('摘要不能混合文档、伪造版本或把无范围当整图', () => {
  const empty = {
    sessionId,
    selectionRevision: '0',
    documentId: null,
    documentRevision: null,
    scope: null,
    layerIds: [],
    region: null,
  };
  expect(isSelectionSummary(empty)).toBe(true);
  for (const change of [
    { selectionRevision: '01' },
    { selectionRevision: '18446744073709551616' },
    { documentId: '1' },
    { layerIds: [0] },
    { sessionId: 'a'.repeat(33) },
  ])
    expect(isSelectionSummary({ ...empty, ...change })).toBe(false);
});
