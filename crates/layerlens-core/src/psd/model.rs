//! 解析实验的规范化输出；不泄漏候选库类型，也不代表已发布的桌面或 MCP 契约。

use serde::Serialize;

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

/// 能力按对象分别报告，限制不能由空值或默认样式掩盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Support {
    Supported,
    Partial,
    Unsupported,
}

/// 描述本次解析或输出的支持程度及已知原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capability {
    pub status: Support,
    pub reason: String,
}

impl Capability {
    pub(super) fn supported(reason: &str) -> Self {
        Self {
            status: Support::Supported,
            reason: reason.into(),
        }
    }

    pub(super) fn unsupported(reason: &str) -> Self {
        Self {
            status: Support::Unsupported,
            reason: reason.into(),
        }
    }
}

/// 图层元数据；顺序遵循规范化结果中的从上到下堆叠顺序。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub id: LayerId,
    pub parent_id: Option<LayerId>,
    pub name: String,
    pub kind: LayerKind,
    pub bounds: Bounds,
    pub visible: bool,
    pub effective_visible: bool,
    /// PSD 原始不透明度，范围 0–255；与像素自身 alpha 分开保留。
    pub opacity: u8,
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
    pub color_profile: Option<String>,
    pub layers: Vec<LayerInfo>,
    pub preview: Capability,
    pub text: Capability,
    pub warnings: Vec<String>,
}

/// 实验入口的保守准入限制；默认值是保护阈值，并非大文件性能承诺。
#[derive(Debug, Clone, Copy)]
pub struct ParseLimits {
    /// 文件字节上限，默认 64 MiB。
    pub max_file_bytes: u64,
    /// 画布与全部图层像素面积之和的上限，默认 16 Mi 像素。
    pub max_total_pixels: u64,
    /// PSD 图层记录数上限（包含组结构记录），默认 4096。
    pub max_layers: u32,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024 * 1024,
            max_total_pixels: 16 * 1024 * 1024,
            max_layers: 4096,
        }
    }
}
