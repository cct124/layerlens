import { afterEach, expect, it, vi } from 'vitest';
import { effectScope, shallowRef } from 'vue';
import { flushPromises } from '@vue/test-utils';
import { readLayers } from '../../shared/api/layers';
import { useLayers } from './useLayers';
import type { LayerResponse, WorkspaceDocument } from '../../shared/api/generated';
vi.mock('../../shared/api/layers', () => ({ readLayers: vi.fn() }));
const doc = (id: string): WorkspaceDocument => ({
  id,
  revision: id,
  name: id,
  path: id,
  width: 10,
  height: 10,
  layerCount: 0,
  colorMode: 'RGB',
  bitDepth: 8,
  previewNote: '',
  selectedLayerId: null,
});
const page = (id: string): LayerResponse => ({
  documentId: id,
  revision: id,
  result: { kind: 'list', page: { offset: 0, total: 0, nextOffset: null, layers: [] } },
});
afterEach(() => vi.resetAllMocks());
it('串行读取，快速切换只保留最新目标，旧成功和失败均不覆盖新文档', async () => {
  const pending: { resolve: (value: LayerResponse) => void; reject: (reason: Error) => void }[] =
    [];
  vi.mocked(readLayers).mockImplementation(
    () => new Promise((resolve, reject) => pending.push({ resolve, reject })),
  );
  const active = shallowRef<WorkspaceDocument | null>(doc('1'));
  const scope = effectScope();
  const state = scope.run(() => useLayers(active))!;
  active.value = doc('2');
  active.value = doc('3');
  expect(readLayers).toHaveBeenCalledTimes(1);
  pending[0]!.reject(new Error('old failure'));
  await flushPromises();
  expect(readLayers).toHaveBeenCalledTimes(2);
  expect(readLayers).toHaveBeenLastCalledWith('3', '3', { kind: 'list', offset: 0, limit: 128 });
  expect(state.error.value).toBeNull();
  pending[1]!.resolve(page('3'));
  await flushPromises();
  expect(state.loading.value).toBe(false);
  scope.stop();
});
it('选择变化与关闭会丢弃在途属性，销毁后不发起后续查询', async () => {
  let resolve!: (value: LayerResponse) => void;
  vi.mocked(readLayers)
    .mockResolvedValueOnce(page('1'))
    .mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
  const active = shallowRef<WorkspaceDocument | null>(doc('1'));
  const scope = effectScope();
  const state = scope.run(() => useLayers(active))!;
  await flushPromises();
  active.value = { ...doc('1'), selectedLayerId: 0 };
  await flushPromises();
  active.value = { ...doc('1'), selectedLayerId: 1 };
  active.value = null;
  scope.stop();
  resolve({
    documentId: '1',
    revision: '1',
    result: {
      kind: 'details',
      details: {
        layer: {
          id: 0,
          parentId: null,
          name: 'old',
          nameTruncated: false,
          kind: 'text',
          bounds: { x: 0, y: 0, width: 1, height: 1 },
          visible: true,
          effectiveVisible: true,
          opacity: 255,
        },
        export: { status: 'unsupported', reason: '' },
        exportBlockers: [],
        diagnostics: [],
        diagnosticsTruncated: false,
        text: null,
      },
    },
  });
  await flushPromises();
  expect(state.details.value).toBeNull();
  expect(readLayers).toHaveBeenCalledTimes(2);
});
