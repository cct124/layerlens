//! 应用信息契约及协议兼容性校验，不读取外部资源。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CommandError, CommandErrorCode};

/// 桌面前端和核心共享的 IPC 契约版本。
pub const IPC_PROTOCOL_VERSION: u32 = 1;

/// 获取应用信息前，调用方声明其理解的协议版本。
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoRequest {
    /// 调用方使用的 IPC 契约版本。
    pub protocol_version: u32,
}

/// 核心返回的应用标识，版本取自 Cargo workspace 包元数据。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// 面向用户的应用名称。
    pub app_name: String,
    /// 当前应用版本，与桌面 crate 共用 workspace 版本。
    pub app_version: String,
    /// 本次响应采用的 IPC 契约版本。
    pub protocol_version: u32,
}

/// 校验调用方协议后返回应用信息，不修改应用状态。
///
/// # Errors
///
/// 调用方版本不匹配时返回 `PROTOCOL_MISMATCH`，不返回不兼容的数据。
pub fn get_app_info(request: AppInfoRequest) -> Result<AppInfo, CommandError> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(CommandError {
            code: CommandErrorCode::ProtocolMismatch,
            message: format!(
                "IPC 协议版本不兼容：客户端为 {}，核心要求 {}。请更新应用后重试。",
                request.protocol_version, IPC_PROTOCOL_VERSION
            ),
        });
    }

    Ok(AppInfo {
        app_name: "LayerLens".to_owned(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol_version: IPC_PROTOCOL_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_protocol_returns_workspace_app_info() {
        let info = get_app_info(AppInfoRequest {
            protocol_version: IPC_PROTOCOL_VERSION,
        })
        .expect("匹配协议必须返回应用信息");

        assert_eq!(info.app_name, "LayerLens");
        assert_eq!(info.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.protocol_version, IPC_PROTOCOL_VERSION);
    }

    #[test]
    fn incompatible_protocol_returns_recoverable_error() {
        let error = get_app_info(AppInfoRequest {
            protocol_version: IPC_PROTOCOL_VERSION + 1,
        })
        .expect_err("不匹配协议不能返回应用信息");

        assert_eq!(error.code, CommandErrorCode::ProtocolMismatch);
        assert!(error.message.contains("请更新应用后重试"));
    }
}
