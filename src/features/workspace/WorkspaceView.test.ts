import { flushPromises, mount } from '@vue/test-utils';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { VueWrapper } from '@vue/test-utils';
import WorkspaceView from './WorkspaceView.vue';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(),
}));

const appInfo = { appName: 'LayerLens', appVersion: '0.1.0', protocolVersion: 1 };
let wrapper: VueWrapper | undefined;

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(isTauri).mockReturnValue(true);
  vi.mocked(invoke).mockResolvedValue(appInfo);
});

afterEach(() => {
  wrapper?.unmount();
  wrapper = undefined;
});

describe('启动工作区', () => {
  it('通过桌面接口确认状态，并保留未开放 PSD 能力的说明', async () => {
    wrapper = mount(WorkspaceView);
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('get_app_info', {
      request: { protocolVersion: 1 },
    });
    expect(wrapper.text()).toContain('桌面应用可用');
    expect(wrapper.text()).toContain('应用版本 0.1.0');
    expect(wrapper.text()).toContain('尚未开放 PSD 文件读取');
    expect(wrapper.find('button').text()).toBe('重新检查');
  });

  it('调用失败时提示并允许重新检查，成功后移除错误', async () => {
    vi.mocked(invoke).mockRejectedValueOnce('transport disconnected');
    wrapper = mount(WorkspaceView);
    await flushPromises();

    expect(wrapper.get('[role="alert"]').text()).toContain('暂时无法读取桌面应用状态');
    expect(wrapper.text()).not.toContain('桌面应用可用');
    await wrapper.get('button').trigger('click');
    await flushPromises();

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.text()).toContain('桌面应用可用');
  });

  it('浏览器预览不调用桌面接口或显示成功状态', async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    wrapper = mount(WorkspaceView);
    await flushPromises();

    expect(invoke).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain('浏览器预览');
    expect(wrapper.text()).toContain('请在桌面应用中继续');
    expect(wrapper.text()).not.toContain('桌面应用可用');
    expect(wrapper.find('button').exists()).toBe(false);
  });

  it.each([null, { ...appInfo, appVersion: 12 }, { ...appInfo, protocolVersion: 2 }])(
    '拒绝不符合接口约定的桌面响应：%j',
    async (response) => {
      vi.mocked(invoke).mockResolvedValue(response);
      wrapper = mount(WorkspaceView);
      await flushPromises();

      expect(wrapper.get('[role="alert"]').text()).toContain('信息不完整或版本不兼容');
      expect(wrapper.text()).not.toContain('桌面应用可用');
    },
  );

  it('将版本错误转换为用户可执行的恢复提示', async () => {
    vi.mocked(invoke).mockRejectedValue({ code: 'PROTOCOL_MISMATCH', message: 'unsupported' });
    wrapper = mount(WorkspaceView);
    await flushPromises();

    expect(wrapper.get('[role="alert"]').text()).toContain('请重新启动或更新应用');
  });
});
