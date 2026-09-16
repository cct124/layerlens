//! PSD 只读验证原型：稳定源读取、输入预检、候选适配和按需 PNG 编码。
//!
//! 尚未接入桌面或 MCP。本轮接纳预检覆盖的 RGB/8 位 PSD 容器；
//! 不提供写回 PSD、效果合成或生产级文档会话。第三方类型仅存在于适配器内。

mod adapter;
mod error;
mod model;
mod normalization;
mod preflight;
mod source;

pub use adapter::PsdDocument;
pub use error::{PsdError, PsdErrorCode};
pub use model::{
    Bounds, Capability, ColorProfileInfo, Diagnostic, DiagnosticKind, DiagnosticScope,
    DocumentInfo, LayerId, LayerInfo, LayerKind, ParseLimits, ResourceUsage, Support, TextData,
};

/// 当前原型使用的候选及仓库补丁标识；补丁来源见 vendor/ag-psd 的维护记录。
pub const PARSER_BUILD: &str = "ag-psd 0.3.0 + LayerLens patch 1";
