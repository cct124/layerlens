import { expect, it } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import LayerTree from './LayerTree.vue';
import { initialTree, treeRows } from './layerTree';
import type { LayerSummary } from '../../shared/api/generated';

const layer = (id: number, parentId: number | null, name: string): LayerSummary => ({
  id,
  parentId,
  name,
  nameTruncated: false,
  kind: parentId === null ? 'group' : 'text',
  bounds: { x: -10, y: 20, width: 30, height: 40 },
  visible: true,
  effectiveVisible: true,
  opacity: 255,
});
it('搜索保留祖先并能找到折叠组中的后代，不修改展开状态', () => {
  const layers = [layer(0, null, 'group'), layer(1, 0, '奖品'), layer(2, 0, 'other')];
  const view = { ...initialTree(), collapsed: [0] };
  expect(treeRows(layers, view).map((row) => row.layer.id)).toEqual([0]);
  expect(treeRows(layers, { ...view, search: '奖' }).map((row) => row.layer.id)).toEqual([0, 1]);
  expect(view.collapsed).toEqual([0]);
});
it('4096 项图层只渲染视口附近的行，选择通过事件提交', async () => {
  const layers = Array.from({ length: 4096 }, (_, id) => layer(id, null, `Layer ${id}`));
  const wrapper = mount(LayerTree, {
    props: { layers, selected: 0, view: initialTree(), loading: false },
  });
  expect(wrapper.findAll('[role=treeitem]').length).toBeLessThanOrEqual(18);
  await wrapper.findAll('.tree-name')[1]!.trigger('click');
  expect(wrapper.emitted('select')).toEqual([[1]]);
  expect(wrapper.find('[aria-selected=true]').text()).toContain('Layer 0');
  wrapper.unmount();
});
it('分页恢复滚动时不把浏览器暂时截短的位置保存为文档视图', async () => {
  const layers = Array.from({ length: 512 }, (_, id) => layer(id, null, `Layer ${id}`));
  const wrapper = mount(LayerTree, {
    props: { layers: [], selected: null, view: { ...initialTree(), scroll: 10000 }, loading: true },
  });
  const viewport = wrapper.get<HTMLElement>('.tree-viewport');
  let position = 0;
  // jsdom 没有布局；模拟浏览器按当前已加载行数限制 scrollTop 的行为。
  Object.defineProperties(viewport.element, {
    clientHeight: { get: () => 280 },
    scrollHeight: { get: () => wrapper.props('layers').length * 28 },
    scrollTop: {
      get: () => position,
      set: (value: number) => {
        position = Math.min(value, Math.max(0, viewport.element.scrollHeight - 280));
      },
    },
  });
  await wrapper.setProps({ layers: layers.slice(0, 128) });
  await flushPromises();
  expect(position).toBe(3304);
  await viewport.trigger('scroll');
  expect(wrapper.emitted('update:view')).toBeUndefined();
  expect(wrapper.findAll('[role=treeitem]').length).toBeGreaterThan(0);
  await wrapper.setProps({ layers, loading: false });
  await flushPromises();
  expect(position).toBe(10000);
  await viewport.trigger('scroll');
  expect(wrapper.emitted('update:view')).toBeUndefined();
  viewport.element.scrollTop = 5000;
  await viewport.trigger('scroll');
  expect(wrapper.emitted('update:view')?.at(-1)).toEqual([{ ...initialTree(), scroll: 5000 }]);
  wrapper.unmount();
});
it('分页失败结束加载但行数不变时，仍将过期滚动位置收敛到可见范围', async () => {
  const wrapper = mount(LayerTree, {
    props: {
      layers: [layer(0, null, 'only')],
      selected: null,
      view: { ...initialTree(), scroll: 10000 },
      loading: true,
    },
  });
  await flushPromises();
  await wrapper.setProps({ loading: false });
  await flushPromises();
  expect(wrapper.emitted('update:view')?.at(-1)).toEqual([{ ...initialTree(), scroll: 0 }]);
  wrapper.unmount();
});
