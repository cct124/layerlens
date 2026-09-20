import { afterEach, expect, it, vi } from 'vitest';
import { effectScope, shallowRef } from 'vue';
import { flushPromises } from '@vue/test-utils';
import { readLayers } from '../../shared/api/layers';
import { useLayers } from './useLayers';
import type { LayerResponse, LayerSummary, WorkspaceDocument } from '../../shared/api/generated';
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
const layer = (id: number): LayerSummary => ({
  id,
  parentId: null,
  name: `Layer ${id}`,
  nameTruncated: false,
  kind: 'bitmap',
  bounds: { x: 0, y: 0, width: 1, height: 1 },
  visible: true,
  effectiveVisible: true,
  opacity: 255,
});
const listPage = (offset: number, total: number): LayerResponse => {
  const end = Math.min(offset + 128, total);
  return {
    documentId: '1',
    revision: '1',
    result: {
      kind: 'list',
      page: {
        offset,
        total,
        nextOffset: end < total ? end : null,
        layers: Array.from({ length: end - offset }, (_, index) => layer(offset + index)),
      },
    },
  };
};
const detail = (id: number): LayerResponse => ({
  documentId: '1',
  revision: '1',
  result: {
    kind: 'details',
    details: {
      layer: layer(id),
      export: { status: 'supported', reason: '' },
      exportBlockers: [],
      diagnostics: [],
      diagnosticsTruncated: false,
      text: null,
    },
  },
});
it('按固定修订串行续读超过 128 项的图层，不遗漏最后一页', async () => {
  vi.mocked(readLayers).mockImplementation(async (_id, _revision, query) => {
    if (query.kind !== 'list') throw new Error('unexpected query');
    return listPage(query.offset, 300);
  });
  const scope = effectScope();
  const state = scope.run(() => useLayers(shallowRef(doc('1'))))!;
  await flushPromises();
  expect(vi.mocked(readLayers).mock.calls.map((call) => call[2])).toEqual([
    { kind: 'list', offset: 0, limit: 128 },
    { kind: 'list', offset: 128, limit: 128 },
    { kind: 'list', offset: 256, limit: 128 },
  ]);
  expect(state.layers.value.map((item) => item.id)).toEqual(
    Array.from({ length: 300 }, (_, id) => id),
  );
  expect(state.loading.value).toBe(false);
  scope.stop();
});
it('续页失败后检查已有图层不清除列表错误，重新读取才能恢复完整列表', async () => {
  vi.mocked(readLayers)
    .mockResolvedValueOnce(listPage(0, 129))
    .mockRejectedValueOnce(new Error('第二页读取失败'))
    .mockResolvedValueOnce(detail(0));
  const active = shallowRef<WorkspaceDocument | null>(doc('1'));
  const scope = effectScope();
  const state = scope.run(() => useLayers(active))!;
  await flushPromises();
  expect(state.layers.value).toHaveLength(128);
  expect(state.error.value).toContain('第二页读取失败');
  active.value = { ...doc('1'), selectedLayerId: 0 };
  await flushPromises();
  expect(state.details.value?.layer.id).toBe(0);
  expect(state.error.value).toContain('第二页读取失败');
  vi.mocked(readLayers).mockImplementation(async (_id, _revision, query) =>
    query.kind === 'list' ? listPage(query.offset, 129) : detail(query.layerId),
  );
  state.reload();
  await flushPromises();
  expect(state.layers.value).toHaveLength(129);
  expect(state.error.value).toBeNull();
  scope.stop();
});
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
