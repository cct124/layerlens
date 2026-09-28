import { invoke } from '@tauri-apps/api/core';
import { beforeEach, expect, it, vi } from 'vitest';
import { cleanupRequest, isCleanupState } from './cleanup';
import type { CleanupPlanDto, CleanupReceiptDto } from './generated';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const sessionId = 'a'.repeat(32);
const plan: CleanupPlanDto = {
  planId: '1',
  documentId: '2',
  documentName: 'A.psd',
  openRevision: '3',
  selectionId: '4',
  liveRevisions: ['3'],
  tasks: [{ id: '33', name: '任务', documentRevision: '3' }],
  loadJobs: [],
  unresolvedLoadJobs: ['1'],
  previewJobs: [],
  expiresInSeconds: 300,
};
const receipt: CleanupReceiptDto = {
  planId: '1',
  documentId: '2',
  revision: '1',
  invalidatedTaskIds: ['33'],
};
const prepare = {
  kind: 'prepare',
  target: { kind: 'document', documentId: '2', documentRevision: '3' },
} as const;
beforeEach(() => vi.resetAllMocks());
it('传递版本／会话，校验完整预检及同计划确认和取消后已执行回执', async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ kind: 'prepared', sessionId, plan });
  expect(await cleanupRequest(sessionId, prepare)).toEqual({ kind: 'prepared', sessionId, plan });
  expect(invoke).toHaveBeenCalledWith('cleanup_request', {
    request: { protocolVersion: 1, sessionId, operation: prepare },
  });
  for (const kind of ['confirm', 'cancel'] as const) {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: 'committed', sessionId, receipt });
    expect((await cleanupRequest(sessionId, { kind, planId: '1' })).kind).toBe('committed');
  }
});
it.each([
  { ...plan, documentId: '9' },
  { ...plan, openRevision: '9' },
  { ...plan, planId: '01' },
  { ...plan, liveRevisions: [] },
  { ...plan, loadJobs: ['1'] },
  { ...plan, tasks: [plan.tasks[0], plan.tasks[0]] },
  { ...plan, documentName: '长'.repeat(100) },
  { ...plan, expiresInSeconds: 0 },
])('拒绝目标、集合、标签或有效期错误的预检', async (invalid) => {
  vi.mocked(invoke).mockResolvedValueOnce({ kind: 'prepared', sessionId, plan: invalid });
  await expect(cleanupRequest(sessionId, prepare)).rejects.toThrow();
});
it('拒绝跨会话、超预算、错误计划、伪装的取消成功', async () => {
  for (const value of [
    { kind: 'prepared', sessionId: 'b'.repeat(32), plan },
    { kind: 'prepared', sessionId, plan, extra: 'x'.repeat(256 * 1024) },
  ]) {
    vi.mocked(invoke).mockResolvedValueOnce(value);
    await expect(cleanupRequest(sessionId, prepare)).rejects.toThrow();
  }
  for (const value of [
    { kind: 'committed', sessionId, receipt: { ...receipt, planId: '2' } },
    { kind: 'cancelled', sessionId, planId: '1' },
    { kind: 'committed', sessionId, receipt: { ...receipt, revision: '0' } },
  ]) {
    vi.mocked(invoke).mockResolvedValueOnce(value);
    await expect(cleanupRequest(sessionId, { kind: 'confirm', planId: '1' })).rejects.toThrow();
  }
});
it('工作区终态投影有界、不接受重复 ID 或零代次的失效任务', () => {
  expect(isCleanupState({ revision: '1', invalidatedTaskIds: ['1', '33'] })).toBe(true);
  for (const value of [
    { revision: '0', invalidatedTaskIds: ['1'] },
    { revision: '1', invalidatedTaskIds: ['1', '1'] },
    { revision: '1', invalidatedTaskIds: Array.from({ length: 129 }, (_, i) => String(i + 1)) },
  ])
    expect(isCleanupState(value)).toBe(false);
});
