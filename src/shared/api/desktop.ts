import { invoke, isTauri } from '@tauri-apps/api/core';
import { IPC_PROTOCOL_VERSION } from './generated';
import type { AppInfo, AppInfoRequest, CommandError } from './generated';

const request = { protocolVersion: IPC_PROTOCOL_VERSION } satisfies AppInfoRequest;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isAppInfo(value: unknown): value is AppInfo {
  return (
    isRecord(value) &&
    typeof value.appName === 'string' &&
    value.appName.trim().length > 0 &&
    typeof value.appVersion === 'string' &&
    value.appVersion.trim().length > 0 &&
    value.protocolVersion === request.protocolVersion
  );
}

function isCommandError(value: unknown): value is CommandError {
  return isRecord(value) && value.code === 'PROTOCOL_MISMATCH' && typeof value.message === 'string';
}

/** 判断当前页面是否由桌面应用提供，浏览器预览不尝试调用桌面接口。 */
export function isDesktopAvailable(): boolean {
  return isTauri();
}

/** 获取并校验桌面应用信息；无效响应和调用失败均转换为可展示的错误。 */
export async function getAppInfo(): Promise<AppInfo> {
  if (!isDesktopAvailable()) {
    throw new Error('请在 LayerLens 桌面应用中检查应用状态。');
  }

  let response: unknown;
  try {
    response = await invoke<unknown>('get_app_info', { request });
  } catch (error: unknown) {
    if (isCommandError(error)) {
      throw new Error('应用界面与桌面服务版本不一致，请重新启动或更新应用。', {
        cause: error,
      });
    }

    throw new Error('暂时无法读取桌面应用状态，请重新检查；若持续失败，请重启应用。', {
      cause: error,
    });
  }

  if (!isAppInfo(response)) {
    throw new Error('桌面应用返回的信息不完整或版本不兼容，请重新启动应用后再试。');
  }

  return response;
}
