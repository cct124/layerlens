/** 菜单只描述已有工作区命令，不直接操作桌面接口或业务状态。 */
export type WorkspaceMenuCommand =
  | 'open'
  | 'reload'
  | 'close'
  | 'fit'
  | 'actual'
  | 'properties'
  | 'document'
  | 'layers'
  | 'tasks'
  | 'resetLayout';

export interface WorkspaceMenuItem {
  command: WorkspaceMenuCommand;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  checked?: boolean;
  separatorBefore?: boolean;
}

export interface WorkspaceMenu {
  id: 'file' | 'view' | 'window';
  label: string;
  accessKey: string;
  items: WorkspaceMenuItem[];
}
