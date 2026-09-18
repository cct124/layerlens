//! PSD 只读验证原型：稳定源读取、输入预检、候选适配和按需 PNG 编码。
//!
//! 尚未接入桌面或 MCP。本轮接纳预检覆盖的 RGB/8 位 PSD 容器；
//! 不提供写回 PSD 或效果合成；多文档调度由 documents 服务负责。
//! 本模块保留同步实验入口，第三方类型仅存在于适配器内。

mod adapter;
mod capability;
mod error;
mod measurement;
mod model;
mod normalization;
mod png_output;
mod preflight;
mod source;
mod text;
mod text_model;

pub use adapter::PsdDocument;
pub use capability::{Capability, Support};
pub use error::{PsdError, PsdErrorCode};
pub use measurement::{OpenTimings, PngTimings};
pub use model::{
    Bounds, ColorProfileInfo, Diagnostic, DiagnosticKind, DiagnosticScope, DocumentInfo,
    ExportBlocker, LayerId, LayerInfo, LayerKind, ParseLimits, ResourceUsage,
};
pub use text_model::{
    CharacterStyle, ParagraphAlignment, ParagraphStyle, ParagraphStyleRun, TextColor, TextData,
    TextDiagnostic, TextDiagnosticCode, TextFont, TextIndexMapping, TextMetric, TextProperty,
    TextStyleRun, TextUnit,
};

/// 当前原型使用的候选及仓库补丁标识；补丁来源见 vendor/ag-psd 的维护记录。
pub const PARSER_BUILD: &str = "ag-psd 0.3.0 + LayerLens patch 3";
