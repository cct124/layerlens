import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { IPC_PROTOCOL_VERSION } from './generated';
import type { WorkspaceAction, WorkspaceSnapshot } from './generated';
import { isWorkspaceError, isWorkspaceSnapshot } from './workspaceValidation';

export function workspaceMessage(error: unknown): string {
  return error instanceof Error || isWorkspaceError(error)
    ? error.message
    : '桌面通信失败，请重新连接工作区。';
}

/** 状态通知与命令返回使用同一契约，通知可能比命令返回更早到达。 */
export async function workspaceAction(action: WorkspaceAction): Promise<WorkspaceSnapshot> {
  const response: unknown = await invoke('workspace_action', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION, action },
  });
  if (!isWorkspaceSnapshot(response))
    throw new Error('工作区信息不完整或版本不兼容，请重新启动应用。');
  return response;
}

export async function subscribeWorkspace(
  receive: (snapshot: WorkspaceSnapshot) => void,
  invalid: (error: Error) => void,
): Promise<() => void> {
  return listen<unknown>('workspace-changed', (event) => {
    if (isWorkspaceSnapshot(event.payload)) receive(event.payload);
    else invalid(new Error('工作区通知无效，请重新连接。'));
  });
}

export async function choosePsdFiles(): Promise<string[]> {
  const response: unknown = await invoke('choose_psd_files', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION },
  });
  if (
    !Array.isArray(response) ||
    !response.every((path): path is string => typeof path === 'string')
  )
    throw new Error('文件选择器返回了无效路径。');
  return response;
}

/** PNG 经二进制 IPC 按需读取；不复制 PSD，不使用 base64 或通用文件系统权限。 */
export async function readPreview(documentId: string, revision: string): Promise<ArrayBuffer> {
  const response: unknown = await invoke('read_preview', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION, documentId, revision },
  });
  if (
    !(response instanceof ArrayBuffer) ||
    response.byteLength < 8 ||
    response.byteLength > 64 * 1024 * 1024 ||
    ![137, 80, 78, 71, 13, 10, 26, 10].every(
      (byte, index) => new Uint8Array(response, 0, 8)[index] === byte,
    )
  )
    throw new Error('预览图像数据无效，请重试预览。');
  return response;
}
