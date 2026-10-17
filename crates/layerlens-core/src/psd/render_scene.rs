//! 开发实验的有界场景；仅把真实解析数据转成绘制输入，不推导 CSS 或替换正式能力声明。
use super::*;
use crate::psd::{Bounds, LayerInfo, LayerKind, TextData};
use ag_psd::psd::{BlendMode, Color, FillRule, Layer, VectorContent};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[path = "render_raster.rs"]
mod raster;

const MAX_LAYERS: usize = 64;
const MAX_PIXELS: u64 = 512 * 512;

/// 只用于显式开发实验；不改变解析器和正式文档服务的默认预算。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RenderProfile {
    Simple,
    /// 有界规模回归档位，适用于 Konva 验证窗口。
    Bounded,
    /// 显式大稿实验：仅返回元数据，PNG 通过有界二进制资源入口逐项读取。
    Large,
    /// 源图层诊断投影：原生基础文字／路径或保存的图层像素，不宣称完整组合成。
    LargeLayers,
}
impl RenderProfile {
    fn limits(self) -> (usize, u64, u64) {
        match self {
            Self::Simple => (MAX_LAYERS, MAX_PIXELS, MAX_PIXELS * 2),
            Self::Bounded => (2048, 16 * 1024 * 1024, 32 * 1024 * 1024),
            Self::Large | Self::LargeLayers => (2048, 32 * 1024 * 1024, 96 * 1024 * 1024),
        }
    }
    pub fn deferred(self) -> bool {
        matches!(self, Self::Large | Self::LargeLayers)
    }
}

/// EngineData Photoshop.BoxBounds 的原始文字框；单位解释仍属实验，不由像素边界推测。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderTextBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 单一路径的控制点已是文档像素坐标；每结点为入控制点、锚点、出控制点。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderPath {
    pub knots: Vec<[f64; 6]>,
    pub even_odd: bool,
}

/// 不支持的内容保持诊断；禁止由前端按名称猜测图形。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RenderContent {
    Bitmap {
        /// Large 档位为空，调用 render_resource 读取该层；其他档位包含 PNG。
        png: Vec<u8>,
    },
    Text {
        data: Box<TextData>,
        text_box: Option<RenderTextBox>,
    },
    Path {
        paths: Vec<RenderPath>,
        color: [f64; 4],
    },
    Group,
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderLayer {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    pub bounds: Bounds,
    pub visible: bool,
    pub opacity: u8,
    pub content: RenderContent,
    pub composition_warning: Option<String>,
}

/// 前端比较的同一份输入，图层顺序从上到下；保存时合成图仅作参考。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderScene {
    pub source_sha256: String,
    pub width: u32,
    pub height: u32,
    pub layers: Vec<RenderLayer>,
    pub reference_png: Vec<u8>,
    pub warnings: Vec<String>,
}

fn limited() -> PsdError {
    PsdError::new(
        PsdErrorCode::ResourceLimit,
        "实验场景超过所选 profile 的图层、像素或路径／文字额度",
    )
}
fn unsupported(reason: &str) -> RenderContent {
    RenderContent::Unsupported {
        reason: reason.into(),
    }
}
fn rgb(color: &Color) -> Option<[f64; 4]> {
    let value = match color {
        Color::Rgb(c) => [c.r / 255.0, c.g / 255.0, c.b / 255.0, 1.0],
        Color::Frgb(c) => [c.fr, c.fg, c.fb, 1.0],
        _ => return None,
    };
    value
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        .then_some(value)
}

impl PsdDocument {
    /// 仅开发特性开放的小场景读取；沿用解析结果，不重新访问文件或更改源数据。
    ///
    /// # Errors
    /// 超过实验预算、源像素解码失败或结构不一致时返回错误。
    pub fn render_scene(&self) -> Result<RenderScene, PsdError> {
        self.render_scene_with_profile(RenderProfile::Simple)
    }

    /// 按明确实验档位读取同一源场景，包含合成参考和图层的总像素仍受限。
    ///
    /// # Errors
    /// 超出所选预算或解码失败时拒绝整个场景，不截断图层冒充完整输出。
    pub fn render_scene_with_profile(
        &self,
        profile: RenderProfile,
    ) -> Result<RenderScene, PsdError> {
        let info = self.info();
        let (max_layers, max_document_pixels, max_total_pixels) = profile.limits();
        let total_pixels = info
            .layers
            .iter()
            .try_fold(
                u64::from(info.width) * u64::from(info.height),
                |sum, layer| {
                    sum.checked_add(u64::from(layer.bounds.width) * u64::from(layer.bounds.height))
                },
            )
            .ok_or_else(limited)?;
        if info.layers.len() > max_layers
            || total_pixels > max_total_pixels
            || u64::from(info.width) * u64::from(info.height) > max_document_pixels
        {
            return Err(limited());
        }
        let mut layers = Vec::with_capacity(info.layers.len());
        for (index, layer) in info.layers.iter().enumerate() {
            let raw = self.render_source_layer(index)?;
            // 隔离组不透明度与复杂祖先不能通过简单平铺静默丢失。
            let ancestor_blocked = layer.parent_id.is_some_and(|id| {
                layers.iter().any(|p: &RenderLayer| {
                    p.id == id.0
                        && (p.opacity != 255
                            || p.composition_warning.is_some()
                            || matches!(p.content, RenderContent::Unsupported { .. }))
                })
            });
            let mut content = if ancestor_blocked {
                unsupported("祖先含未实现的组合成语义")
            } else {
                self.render_content(layer, raw, profile.deferred())?
            };
            let mut composition_warning = None;
            if matches!(profile, RenderProfile::LargeLayers) {
                let reason = match &content {
                    RenderContent::Unsupported { reason } => reason.as_str(),
                    _ => "源图层诊断未执行完整组、画板、效果、剪贴和色彩合成",
                };
                composition_warning = Some(reason.to_owned());
                content = self.render_intrinsic(layer, raw, true)?;
                if matches!(content, RenderContent::Unsupported { .. })
                    && layer.kind != LayerKind::Group
                {
                    content = raster::source_bitmap(self, layer, raw, true)?;
                }
            }
            layers.push(RenderLayer {
                id: layer.id.0,
                parent_id: layer.parent_id.map(|id| id.0),
                name: layer.name.chars().take(256).collect(),
                bounds: layer.bounds,
                visible: layer.effective_visible,
                opacity: layer.opacity,
                content,
                composition_warning,
            });
        }
        Ok(RenderScene {
            source_sha256: self.source_sha256().into(),
            width: info.width,
            height: info.height,
            layers,
            reference_png: if profile.deferred() {
                Vec::new()
            } else {
                self.preview_png()?
            },
            warnings: vec![
                "实验重绘不代表 Photoshop 保真；文字引擎单位、排版和颜色管理仍待独立参考验证。"
                    .into(),
                "仅支持普通混合、简单实色闭合路径、文字事实和无视觉依赖位图；不执行 PSD 编辑。"
                    .into(),
            ],
        })
    }

    fn render_source_layer(&self, index: usize) -> Result<&Layer, PsdError> {
        let mut children = self.parsed.children.as_deref().unwrap_or_default();
        let mut target = None;
        for &position in self.paths.get(index).ok_or_else(layer_not_found)? {
            let child = children.get(position).ok_or_else(layer_not_found)?;
            target = Some(child);
            children = child.children.as_deref().unwrap_or_default();
        }
        target.ok_or_else(layer_not_found)
    }

    /// 固定源的大稿实验 PNG；None 读取保存时合成，Some 读取已支持图层像素。
    /// 每次最多 32 MiB 编码输出，解码沿用文档额度，不扩大独立素材导出权限。
    ///
    /// # Errors
    /// 引用无效、内容非位图或超过解码／编码预算时失败。
    pub fn render_resource(
        &self,
        id: Option<LayerId>,
        profile: RenderProfile,
    ) -> Result<Vec<u8>, PsdError> {
        let Some(id) = id else {
            return self.preview_png_bounded(32 * 1024 * 1024).map(|v| v.0);
        };
        let index = self
            .info
            .layers
            .iter()
            .position(|l| l.id == id)
            .ok_or_else(layer_not_found)?;
        let layer = &self.info.layers[index];
        let raw = self.render_source_layer(index)?;
        let content = if matches!(profile, RenderProfile::LargeLayers) {
            raster::source_bitmap(self, layer, raw, false)?
        } else {
            self.render_content(layer, raw, false)?
        };
        match content {
            RenderContent::Bitmap { png } => Ok(png),
            _ => Err(PsdError::new(
                PsdErrorCode::ExportUnsupported,
                "实验资源只读取已支持位图；不将未知内容伪装为像素",
            )),
        }
    }

    fn render_content(
        &self,
        info: &LayerInfo,
        raw: &Layer,
        defer_pixels: bool,
    ) -> Result<RenderContent, PsdError> {
        let a = &raw.additional_info;
        let record = self
            .render_records
            .get(info.source_record_index as usize)
            .ok_or_else(layer_not_found)?;
        // Adobe Global layer mask kind=128 表示使用逐层值；叠加色为蒙版显示元数据。
        // 只在实验合成中解释这一种，正式素材导出仍保留 GlobalMask 拒绝因素。
        if info
            .export_blockers
            .contains(&crate::psd::ExportBlocker::GlobalMask)
            && !self
                .parsed
                .global_layer_mask_info
                .as_ref()
                .is_some_and(|m| m.kind == 128.0)
            || !a.diagnostics.is_empty()
        {
            return Ok(unsupported("全局蒙版或未解释的图层属性尚未实现"));
        }
        if raw.clipping == Some(true)
            || a.real_mask.is_some()
            || a.effects.is_some()
            || a.adjustment.is_some()
            || a.placed_layer.is_some()
            || a.knockout == Some(true)
            || a.fill_opacity.is_some_and(|n| n != 1.0)
            || record.has_nondefault_blending_ranges
            || info.kind == LayerKind::Bitmap && record.flags & 0x10 != 0
            || raw
                .blend_mode
                .is_some_and(|mode| !matches!(mode, BlendMode::Normal | BlendMode::PassThrough))
        {
            return Ok(unsupported("蒙版、效果、混合或智能对象尚未实现"));
        }
        // sn2P 控制原始绘制的像素对齐；普通位图使用已存像素，不能据此允许矢量重绘。
        let unknown: Vec<_> = record
            .tags
            .iter()
            .filter(|key| {
                if info.kind == LayerKind::Bitmap && *key == b"sn2P" {
                    return false;
                }
                !matches!(
                    *key,
                    b"lyid"
                        | b"luni"
                        | b"lsct"
                        | b"lsdk"
                        | b"lspf"
                        | b"lclr"
                        | b"lnsr"
                        | b"lyvr"
                        | b"fxrp"
                        | b"clbl"
                        | b"infx"
                        | b"knko"
                        | b"lmgm"
                        | b"vmgm"
                        | b"iOpa"
                        | b"shmd"
                        | b"TySh"
                        | b"SoCo"
                        | b"vmsk"
                        | b"vsms"
                )
            })
            .map(|key| String::from_utf8_lossy(key).into_owned())
            .collect();
        if !unknown.is_empty() {
            return Ok(unsupported(&format!(
                "实验未验证的图层属性：{}",
                unknown.join(",")
            )));
        }
        self.render_intrinsic(info, raw, defer_pixels)
    }

    fn render_intrinsic(
        &self,
        info: &LayerInfo,
        raw: &Layer,
        defer_pixels: bool,
    ) -> Result<RenderContent, PsdError> {
        let a = &raw.additional_info;
        let record = self
            .render_records
            .get(info.source_record_index as usize)
            .ok_or_else(layer_not_found)?;
        match info.kind {
            LayerKind::Group if a.mask.is_none() => Ok(RenderContent::Group),
            LayerKind::Text => {
                if a.mask.is_some() {
                    return Ok(unsupported("文字蒙版尚未实现"));
                }
                let Some(data) = info.text.as_ref() else {
                    return Ok(unsupported("解析器未提供文字事实"));
                };
                if data.utf16_length > 4096
                    || data.style_runs.as_ref().is_some_and(|r| r.len() > 64)
                    || data.paragraph_runs.as_ref().is_some_and(|r| r.len() > 64)
                {
                    return Err(limited());
                }
                Ok(RenderContent::Text {
                    data: Box::new(data.clone()),
                    text_box: a.text.as_ref().and_then(|text| {
                        if text.shape_type != Some(ag_psd::psd::TextShapeType::Box) {
                            return None;
                        }
                        let [left, top, right, bottom] = text.box_bounds.as_deref()? else {
                            return None;
                        };
                        [left, top, right, bottom]
                            .iter()
                            .all(|v| v.is_finite() && v.abs() <= 100_000.0)
                            .then_some(())?;
                        (right > left && bottom > top).then_some(RenderTextBox {
                            x: *left,
                            y: *top,
                            width: right - left,
                            height: bottom - top,
                        })
                    }),
                })
            }
            LayerKind::Bitmap if record.has_rgb => {
                raster::bitmap(self, info, raw, record, defer_pixels)
            }
            LayerKind::Shape => {
                if a.mask.is_some() {
                    return Ok(unsupported("形状叠加像素蒙版尚未实现"));
                }
                let Some(mask) = &a.vector_mask else {
                    return Ok(unsupported("解析器未提供 vmsk 路径"));
                };
                let Some(VectorContent::Color(color)) = &a.vector_fill else {
                    return Ok(unsupported("解析器未提供 SoCo 实色填充"));
                };
                if mask.invert == Some(true)
                    || mask.disable == Some(true)
                    || mask.fill_starts_with_all_pixels == Some(true)
                    || a.vector_stroke.is_some()
                {
                    return Ok(unsupported("反向路径或矢量描边尚未实现"));
                }
                let Some(color) = rgb(color) else {
                    return Ok(unsupported("路径颜色模式未支持"));
                };
                if mask.paths.len() > 32 {
                    return Err(limited());
                }
                if mask.paths.len() != 1 {
                    return Ok(unsupported("多子路径联合、孔洞和布尔合成尚未验证"));
                }
                let mut paths = Vec::new();
                for path in &mask.paths {
                    if path.open
                        || path
                            .operation
                            .is_some_and(|op| op != ag_psd::psd::BooleanOperation::Combine)
                    {
                        return Ok(unsupported("开放路径或布尔路径尚未实现"));
                    }
                    if path.knots.len() > 256 {
                        return Err(limited());
                    }
                    let knots = path
                        .knots
                        .iter()
                        .map(|k| {
                            let mut points: [f64; 6] =
                                k.points.as_slice().try_into().map_err(|_| limited())?;
                            // 锁定的 ag-psd-rs doc_size 返回 1×1；其路径仍是 PSD 8.24 归一化坐标。
                            // 只在适配边界乘原稿尺寸，独立整数几何样本约束该版本行为。
                            for pair in points.chunks_exact_mut(2) {
                                pair[0] *= f64::from(self.info.width);
                                pair[1] *= f64::from(self.info.height);
                            }
                            if points.iter().any(|n| !n.is_finite() || n.abs() > 100_000.0) {
                                return Err(limited());
                            }
                            Ok(points)
                        })
                        .collect::<Result<Vec<_>, PsdError>>()?;
                    paths.push(RenderPath {
                        knots,
                        even_odd: matches!(path.fill_rule, FillRule::EvenOdd),
                    });
                }
                Ok(RenderContent::Path { paths, color })
            }
            _ => Ok(unsupported("当前图层类型／独立像素读取未获支持")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiple_subpaths_cannot_be_painted_as_independent_translucent_shapes() {
        let mut document = PsdDocument::from_bytes(
            include_bytes!("../../tests/fixtures/psd/render-simple.psd"),
            ParseLimits::default(),
        )
        .unwrap();
        let raw = document
            .parsed
            .children
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|l| l.additional_info.name.as_deref() == Some("blue-card"))
            .unwrap();
        let mask = raw.additional_info.vector_mask.as_mut().unwrap();
        mask.paths.push(mask.paths[0].clone());
        let scene = document.render_scene().unwrap();
        let item = scene.layers.iter().find(|l| l.name == "blue-card").unwrap();
        assert!(
            matches!(&item.content, RenderContent::Unsupported {reason} if reason.contains("多子路径"))
        );
    }
}
