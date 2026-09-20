import { mount } from '@vue/test-utils';
import { expect, it } from 'vitest';
import DockTabs from './DockTabs.vue';

it('面板标签提供正确关联、单一 Tab 焦点入口及方向键循环切换', async () => {
  const wrapper = mount(DockTabs, {
    attachTo: document.body,
    props: {
      id: 'test',
      label: '测试面板',
      modelValue: 'layers',
      tabs: [
        { id: 'layers', label: '图层' },
        { id: 'tasks', label: '任务', badge: 2 },
      ],
    },
  });
  const tabs = wrapper.findAll('[role=tab]');
  expect(tabs[0]!.attributes('aria-controls')).toBe('test-panel-layers');
  expect(tabs[0]!.attributes('tabindex')).toBe('0');
  expect(tabs[1]!.attributes('tabindex')).toBe('-1');
  await tabs[0]!.trigger('keydown', { key: 'ArrowRight' });
  expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual(['tasks']);
  expect(document.activeElement).toBe(tabs[1]!.element);
  await wrapper.setProps({ modelValue: 'tasks' });
  expect(tabs[1]!.attributes('aria-selected')).toBe('true');
  await tabs[1]!.trigger('keydown', { key: 'ArrowRight' });
  expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual(['layers']);
  await tabs[0]!.trigger('keydown', { key: 'End' });
  expect(document.activeElement).toBe(tabs[1]!.element);
  await tabs[1]!.trigger('keydown', { key: 'Home' });
  expect(document.activeElement).toBe(tabs[0]!.element);
  wrapper.unmount();
});
