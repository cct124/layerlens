//! 文字只读输出契约：原文 UTF-16 区间、显式属性来源与未验证单位。
//! 不推断字体字重、不执行排版、不把文本引擎数值当作 CSS 值。

use serde::Serialize;
use ts_rs::TS;

use super::capability::Capability;

/// TySh 原文、矩阵及经校验的样式；null 区间表示缺失或不可映射，空数组表示空文本。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextData {
    pub raw_text: String,
    pub utf16_length: u32,
    /// 文字空间到文档空间的 [a,b,c,d,tx,ty]；未应用到字号或再次应用到边界。
    pub transform: Option<[f64; 6]>,
    pub normalization_changed: bool,
    pub styles: Capability,
    pub index_mapping: Option<TextIndexMapping>,
    pub style_runs: Option<Vec<TextStyleRun>>,
    pub paragraph_runs: Option<Vec<ParagraphStyleRun>>,
    pub diagnostics: Vec<TextDiagnostic>,
}

/// 仅允许精确原文或原文后单个引擎段落结束符；不按归一化文本猜测区间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TextIndexMapping {
    Exact,
    TrailingParagraphTerminator,
}

/// 属性值及其完整 EngineData 键路径；继承仅使用源文件显式 DefaultRunData。
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct TextProperty<T> {
    pub value: T,
    pub source: String,
}

/// 尚未有独立参考确认 pt/px 的引擎量；保留数值而不按 DPI 或矩阵换算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TextUnit {
    UnverifiedEngine,
}

/// 原始文字度量值；包含字号、行距、字距和段落间距。
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct TextMetric {
    pub value: f64,
    pub unit: TextUnit,
}

/// 明确 Font 索引引用的 FontSet 项；名称不表示本机可用或确定的 CSS 字重。
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextFont {
    pub index: u32,
    pub name: String,
    pub family: Option<String>,
    pub style: Option<String>,
}

/// 引擎 RGB 颜色的原始 0–1 通道；未执行 ICC 转换，不能直接声明为 sRGB。
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct TextColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

/// 源中已读取的字符属性；null 表示缺失、非法或未支持，原因见文字诊断。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CharacterStyle {
    pub font: Option<TextProperty<TextFont>>,
    pub font_size: Option<TextProperty<TextMetric>>,
    pub fill_color: Option<TextProperty<TextColor>>,
    pub fill_enabled: Option<TextProperty<bool>>,
    pub stroke_enabled: Option<TextProperty<bool>>,
    /// Photoshop 的显式 FauxBold；不转换成 CSS font-weight。
    pub faux_bold: Option<TextProperty<bool>>,
    pub faux_italic: Option<TextProperty<bool>>,
    pub auto_leading: Option<TextProperty<bool>>,
    pub leading: Option<TextProperty<TextMetric>>,
    pub tracking: Option<TextProperty<TextMetric>>,
    pub baseline_shift: Option<TextProperty<TextMetric>>,
    pub horizontal_scale: Option<TextProperty<f64>>,
    pub vertical_scale: Option<TextProperty<f64>>,
}

/// 字符样式的原文 UTF-16 左闭右开区间；边界不拆分代理对。
#[derive(Debug, Clone, Serialize, TS)]
pub struct TextStyleRun {
    pub start: u32,
    pub end: u32,
    pub style: CharacterStyle,
}

/// 引擎 Justification 显式枚举；未知值不回退到左对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ParagraphAlignment {
    Left,
    Right,
    Center,
    JustifyLeft,
    JustifyRight,
    JustifyCenter,
    JustifyAll,
}

/// 已读取的段落属性；自动行距仅保留比率，不推算最终排版行高。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ParagraphStyle {
    pub alignment: Option<TextProperty<ParagraphAlignment>>,
    pub auto_leading: Option<TextProperty<f64>>,
    pub first_line_indent: Option<TextProperty<TextMetric>>,
    pub start_indent: Option<TextProperty<TextMetric>>,
    pub end_indent: Option<TextProperty<TextMetric>>,
    pub space_before: Option<TextProperty<TextMetric>>,
    pub space_after: Option<TextProperty<TextMetric>>,
}

/// 段落属性的原文 UTF-16 左闭右开区间。
#[derive(Debug, Clone, Serialize, TS)]
pub struct ParagraphStyleRun {
    pub start: u32,
    pub end: u32,
    pub style: ParagraphStyle,
}

/// 缺失、非法、未实现与待独立核验的信息可机器区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TextDiagnosticCode {
    Missing,
    Invalid,
    Unsupported,
    Unverified,
    TextMismatch,
    InvalidRange,
}

/// 文字局部诊断；路径指向字段或数组项，不记录文字原文及字体名称。
#[derive(Debug, Clone, Serialize, TS)]
pub struct TextDiagnostic {
    pub code: TextDiagnosticCode,
    pub path: String,
    pub message: String,
}
