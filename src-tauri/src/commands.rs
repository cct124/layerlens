//! IPC 适配层保留核心的输入、输出与错误契约，不复制业务规则。

use layerlens_core::{AppInfo, AppInfoRequest, CommandError};

/// 转发应用信息查询，由核心校验协议兼容性。
#[tauri::command]
pub(crate) fn get_app_info(request: AppInfoRequest) -> Result<AppInfo, CommandError> {
    layerlens_core::get_app_info(request)
}
