//! LayerLens 共享应用核心，不依赖桌面组件或协议适配器。
//!
//! 提供应用信息与协议校验，以及尚未接入桌面的 PSD 只读验证原型。

mod app_info;
mod error;
pub mod psd;

pub use app_info::{AppInfo, AppInfoRequest, IPC_PROTOCOL_VERSION, get_app_info};
pub use error::{CommandError, CommandErrorCode};
