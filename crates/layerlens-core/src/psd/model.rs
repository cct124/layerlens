//! 解析实验的规范化输出；不泄漏候选库类型，也不代表已发布的桌面或 MCP 契约。

use serde::Serialize;

use super::{capability::Capability, text_model::TextData};

/// 仅在所属解析结果中有效的图层引用；不使用图层名称或 PSD 内部 ID 寻址。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LayerId(pub u32);

/// 文档像素坐标，原点位于画布左上角；保留负坐标及零面积。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 原型能识别的来源类型；识别类型不表示支持其渲染或样式读取。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerKind {
    Group,
    Bitmap,
    Text,
    Shape,
    SmartObject,
    Adjustment,
    Unknown,
}

/// 诊断所属边界；文件偏移始终相对于固定的原始 PSD 字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticScope {
    ImageResource,
    Document,
    Layer,
}

/// 未识别数据、明确缺失的能力和有待独立对照的行为分别报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticKind {
    Unknown,
    Unsupported,
    Unverified,
}

/// 可机器读取的局部诊断；不包含设计文字、图层名称或原始有效载荷。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub scope: DiagnosticScope,
    pub kind: DiagnosticKind,
    pub tag: String,
    pub offset: u64,
    pub message: String,
}

/// 已存在的 ICC 仅记录来源与指纹；尚未执行颜色转换。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorProfileInfo {
    pub byte_length: u64,
    pub sha256: String,
    pub conversion: Capability,
}

/// 读取阶段的源与结构计量，与单次像素解码预算分开。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    pub source_bytes: u64,
    pub canvas_pixels: u64,
    pub total_declared_pixels: u64,
    pub layer_records: u32,
    pub max_decoded_bytes: u64,
    pub global_additional_blocks: Vec<String>,
}

/// 图层元数据；顺序遵循规范化结果中的从上到下堆叠顺序。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub id: LayerId,
    pub parent_id: Option<LayerId>,
    /// 从零开始的 PSD 原始记录位置；只用于诊断，不替代本次结果中的 id。
    pub source_record_index: u32,
    pub name: String,
    pub kind: LayerKind,
    pub bounds: Bounds,
    pub visible: bool,
    pub effective_visible: bool,
    /// PSD 原始不透明度，范围 0–255；与像素自身 alpha 分开保留。
    pub opacity: u8,
    pub text: Option<TextData>,
    pub diagnostics: Vec<Diagnostic>,
    pub export: Capability,
}

/// 本轮实验读取的文档信息；没有读取到的分辨率与配置明确为 null。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u16,
    pub color_mode: String,
    pub resolution_dpi: Option<[f64; 2]>,
    pub color_profile: Option<ColorProfileInfo>,
    pub layers: Vec<LayerInfo>,
    pub preview: Capability,
    pub text: Capability,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub resources: ResourceUsage,
}

/// 实验入口的保守准入限制；默认值是保护阈值，并非大文件性能承诺。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseLimits {
    /// 文件字节上限，默认 64 MiB。
    pub max_file_bytes: u64,
    /// 画布、全部图层与蒙版像素面积之和的上限，默认 16 Mi 像素。
    pub max_total_pixels: u64,
    /// PSD 图层记录数上限（包含组结构记录），默认 4096。
    pub max_layers: u32,
    /// 每次候选解码的像素／临时缓冲预算，默认 128 MiB，不包含源字节和输出 PNG。
    pub max_decoded_bytes: u64,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024 * 1024,
            max_total_pixels: 16 * 1024 * 1024,
            max_layers: 4096,
            max_decoded_bytes: 128 * 1024 * 1024,
        }
    }
}
