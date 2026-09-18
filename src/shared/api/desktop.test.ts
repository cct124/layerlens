import { beforeEach, expect, it, vi } from 'vitest';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getAppInfo } from './desktop';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(isTauri).mockReturnValue(true);
});
it.each([
  null,
  { appName: 'LayerLens', appVersion: 12, protocolVersion: 1 },
  { appName: 'LayerLens', appVersion: '0.1.0', protocolVersion: 2 },
])('拒绝无效应用信息 %j', async (value) => {
  vi.mocked(invoke).mockResolvedValue(value);
  await expect(getAppInfo()).rejects.toThrow('信息不完整或版本不兼容');
});
it('保留版本失配恢复提示', async () => {
  vi.mocked(invoke).mockRejectedValue({ code: 'PROTOCOL_MISMATCH', message: 'unsupported' });
  await expect(getAppInfo()).rejects.toThrow('版本不一致');
});
