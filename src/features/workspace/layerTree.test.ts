import { expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
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
