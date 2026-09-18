//! LayerLens 共享应用核心，不依赖桌面组件或协议适配器。
//!
//! 提供应用信息、协议校验、PSD 只读解析及尚未接入桌面的文档生命周期服务。

mod app_info;
pub mod documents;
mod error;
pub mod psd;

pub use app_info::{AppInfo, AppInfoRequest, IPC_PROTOCOL_VERSION, get_app_info};
pub use error::{CommandError, CommandErrorCode};
