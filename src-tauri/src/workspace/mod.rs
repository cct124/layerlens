//! 桌面工作区适配：一个有界命令邮箱、一份共享核心和轻量状态通知。
//! 所有可能析构大对象的操作在专用线程运行；前端不持有核心对象。

mod engine;
mod runtime;

pub(crate) use runtime::WorkspaceHost;

use layerlens_core::{
    IPC_PROTOCOL_VERSION,
    documents::DocumentError,
    psd::PsdErrorCode,
    workspace_contract::{WorkspaceError, WorkspaceErrorCode},
};

pub(crate) const EVENT: &str = "workspace-changed";

pub(crate) fn error(code: WorkspaceErrorCode, message: impl Into<String>) -> WorkspaceError {
    WorkspaceError {
        code,
        message: message.into(),
    }
}

pub(crate) fn check_version(version: u32) -> Result<(), WorkspaceError> {
    if version != IPC_PROTOCOL_VERSION {
        return Err(error(
            WorkspaceErrorCode::ProtocolMismatch,
            "界面与桌面服务版本不一致，请重新启动或更新应用。",
        ));
    }
    Ok(())
}

pub(super) fn domain_error(value: &DocumentError) -> WorkspaceError {
    let code = match value {
        DocumentError::QueueFull { .. } => WorkspaceErrorCode::Busy,
        DocumentError::ResourceLimit { .. } => WorkspaceErrorCode::ResourceLimit,
        DocumentError::NotFound(_) => WorkspaceErrorCode::NotFound,
        DocumentError::LayerNotFound(_) => WorkspaceErrorCode::NotFound,
        DocumentError::StaleRevision => WorkspaceErrorCode::StaleRevision,
        DocumentError::InvalidQuery(_) => WorkspaceErrorCode::InvalidInput,
        DocumentError::ShuttingDown => WorkspaceErrorCode::ShuttingDown,
        DocumentError::Parse { source, .. } | DocumentError::Preview { source, .. }
            if source.code == PsdErrorCode::ResourceLimit =>
        {
            WorkspaceErrorCode::ResourceLimit
        }
        DocumentError::Path { .. } | DocumentError::Parse { .. } => WorkspaceErrorCode::OpenFailed,
        DocumentError::Preview { .. } => WorkspaceErrorCode::PreviewFailed,
        _ => WorkspaceErrorCode::Internal,
    };
    error(code, value.to_string())
}
