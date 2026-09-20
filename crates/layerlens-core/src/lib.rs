//! LayerLens 共享应用核心，不依赖桌面组件或协议适配器。
//!
//! 提供应用信息、边界契约、PSD 只读解析和文档生命周期服务。

mod app_info;
pub mod documents;
mod error;
pub mod psd;
pub mod selection_contract;
pub mod workspace_contract;

pub use app_info::{AppInfo, AppInfoRequest, IPC_PROTOCOL_VERSION, get_app_info};
pub use error::{CommandError, CommandErrorCode};
