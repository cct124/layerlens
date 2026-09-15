//! LayerLens 共享应用核心，不依赖桌面组件或协议适配器。
//!
//! 当前仅提供应用信息与协议版本校验，后续业务能力按职责增加。

mod app_info;
mod error;

pub use app_info::{AppInfo, AppInfoRequest, IPC_PROTOCOL_VERSION, get_app_info};
pub use error::{CommandError, CommandErrorCode};
