//! 将候选输出与独立容器记录核对，规范化图层类型、原文及逐项能力。
//!
//! 未验证的附加信息不授予独立素材导出能力；来源图层 ID 和名称均不作为寻址键。

use ag_psd::psd::{BlendMode, Layer, Psd, ReadDiagnostic};
use sha2::{Digest, Sha256};

use super::{
    capability::Capability,
    error::{PsdError, PsdErrorCode},
    model::*,
    preflight::{Header, LayerRecord},
};

pub(super) struct NormalizedDocument {
    pub info: DocumentInfo,
    pub paths: Vec<Vec<usize>>,
}

pub(super) fn normalize(
    parsed: &Psd,
    header: &Header,
    bytes: &[u8],
    limits: ParseLimits,
) -> Result<NormalizedDocument, PsdError> {
    let records: Vec<_> = header
        .layers
        .iter()
        .rev()
        .filter(|record| record.section_kind != Some(3))
        .collect();
    let mut layers = LayerNormalizer {
        records,
        output: Vec::new(),
        paths: Vec::new(),
    };
    layers.visit(
        parsed.children.as_deref().unwrap_or_default(),
        &[],
        None,
        true,
        header.global_mask_offset.is_none(),
    )?;
    if layers.output.len() != layers.records.len() {
        return Err(invalid_candidate("候选图层数量与已校验的 PSD 记录不一致"));
    }
    let color_profile =
        if let Some(resource) = header.resources.iter().find(|resource| resource.id == 1039) {
            let start = usize::try_from(resource.offset)
                .map_err(|_| invalid_candidate("ICC 偏移超出范围"))?;
            let size = usize::try_from(resource.length)
                .map_err(|_| invalid_candidate("ICC 长度超出范围"))?;
            let end = start
                .checked_add(size)
                .ok_or_else(|| invalid_candidate("ICC 长度溢出"))?;
            let data = bytes
                .get(start..end)
                .ok_or_else(|| invalid_candidate("ICC 资源范围与源字节不一致"))?;
            Some(ColorProfileInfo {
                byte_length: resource.length,
                sha256: format!("{:x}", Sha256::digest(data)),
                conversion: Capability::unsupported(
                    "ICC 原始资源已识别，尚未验证配置内容或执行颜色转换",
                ),
            })
        } else {
            None
        };
    let preview = if !header.has_composite {
        Capability::unsupported("没有保存时合成图；不自动重新合成")
    } else if parsed.channels != Some(3.0) && !header.global_alpha {
        Capability::unsupported("额外合成通道没有 global alpha 标记，其透明度语义尚未验证")
    } else if color_profile.is_some() {
        Capability::partial("保存时合成图可解码；未执行 ICC 色彩转换，显示颜色与原稿仍待对照")
    } else {
        Capability::supported("保存时合成图 RAW/RLE 与来源透明度；不重绘图层，不假定 sRGB")
    };
    let resolution_dpi = parsed
        .image_resources
        .as_ref()
        .and_then(|resources| resources.resolution_info.as_ref())
        .map(|resolution| {
            [
                resolution.horizontal_resolution,
                resolution.vertical_resolution,
            ]
        });
    let mut diagnostics: Vec<_> = parsed
        .diagnostics
        .iter()
        .map(|value| {
            diagnostic(
                value,
                match value.scope {
                    ag_psd::psd::DiagnosticScope::ImageResource => DiagnosticScope::ImageResource,
                    ag_psd::psd::DiagnosticScope::AdditionalInfo => DiagnosticScope::Document,
                },
            )
        })
        .collect();
    diagnostics.extend(
        parsed
            .additional_info
            .diagnostics
            .iter()
            .map(|value| diagnostic(value, DiagnosticScope::Document)),
    );
    if let Some(offset) = header.global_mask_offset {
        diagnostics.push(Diagnostic {
            scope: DiagnosticScope::Document,
            kind: DiagnosticKind::Unsupported,
            tag: "globalLayerMask".into(),
            offset,
            message: "全局蒙版的独立素材语义尚未验证".into(),
        });
    }
    if color_profile.is_some() {
        let offset = header
            .resources
            .iter()
            .find(|resource| resource.id == 1039)
            .map_or(0, |r| r.offset);
        diagnostics.push(Diagnostic {
            scope: DiagnosticScope::ImageResource,
            kind: DiagnosticKind::Unverified,
            tag: "1039".into(),
            offset,
            message: "ICC 配置尚未转换；预览和素材保留原通道值".into(),
        });
    }
    let text = if layers.output.iter().any(|layer| layer.text.is_some()) {
        Capability::partial(
            "可读取 TySh 原文、矩阵及经区间校验的 EngineData 样式；逐层限制见文字诊断，单位与视觉保真仍待独立参考",
        )
    } else {
        Capability::unsupported("本次未读取到可验证的 TySh 原文")
    };
    Ok(NormalizedDocument {
        info: DocumentInfo {
            width: header.width,
            height: header.height,
            bit_depth: 8,
            color_mode: "RGB".into(),
            resolution_dpi,
            color_profile,
            layers: layers.output,
            preview,
            text,
            warnings: vec![
                "复杂图层的读取能力与独立素材导出能力分别判断；局部缺口见 diagnostics".into(),
            ],
            diagnostics,
            resources: ResourceUsage {
                source_bytes: bytes.len() as u64,
                canvas_pixels: u64::from(header.width) * u64::from(header.height),
                total_declared_pixels: header.total_pixels,
                layer_records: header.layers.len() as u32,
                max_decoded_bytes: limits.max_decoded_bytes,
                global_additional_blocks: header
                    .global_tags
                    .iter()
                    .map(|tag| String::from_utf8_lossy(tag).into_owned())
                    .collect(),
            },
        },
        paths: layers.paths,
    })
}

struct LayerNormalizer<'a> {
    records: Vec<&'a LayerRecord>,
    output: Vec<LayerInfo>,
    paths: Vec<Vec<usize>>,
}

impl LayerNormalizer<'_> {
    fn visit(
        &mut self,
        children: &[Layer],
        parent_path: &[usize],
        parent_id: Option<LayerId>,
        parent_visible: bool,
        parent_simple: bool,
    ) -> Result<(), PsdError> {
        for (index, layer) in children.iter().enumerate().rev() {
            let position = self.output.len();
            let record = *self
                .records
                .get(position)
                .ok_or_else(|| invalid_candidate("候选额外生成了未校验的图层"))?;
            let id = LayerId(
                u32::try_from(position).map_err(|_| invalid_candidate("图层引用超出范围"))?,
            );
            let mut path = parent_path.to_vec();
            path.push(index);
            verify_bounds(layer, record.bounds)?;
            if record
                .source_id
                .is_some_and(|value| layer.additional_info.id != Some(f64::from(value)))
            {
                return Err(invalid_candidate("候选来源图层 ID 与原始记录不一致"));
            }
            if layer.clipping != Some(record.clipping) {
                return Err(invalid_candidate("候选剪贴关系与原始记录不一致"));
            }
            let visible = layer
                .hidden
                .map(|value| !value)
                .ok_or_else(|| invalid_candidate("缺少图层可见性"))?;
            if visible != (record.flags & 2 == 0) {
                return Err(invalid_candidate("候选可见性与源记录不一致"));
            }
            let effective_visible = parent_visible && visible;
            let opacity = layer
                .opacity
                .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                .ok_or_else(|| invalid_candidate("缺少有效的图层不透明度"))?;
            let kind = classify(layer, record);
            if matches!(record.section_kind, Some(1 | 2)) != (kind == LayerKind::Group) {
                return Err(invalid_candidate("候选分组结构与 PSD 分隔记录不一致"));
            }
            let diagnostics: Vec<_> = layer
                .additional_info
                .diagnostics
                .iter()
                .map(|value| diagnostic(value, DiagnosticScope::Layer))
                .collect();
            let simple =
                parent_simple && simple_compositing(layer, record) && diagnostics.is_empty();
            let export = if kind != LayerKind::Bitmap {
                Capability::unsupported(
                    "仅开放已验证的普通位图独立导出；组、文字、形状、智能对象及未知类型不以通道冒充素材",
                )
            } else if !effective_visible {
                Capability::unsupported("图层或祖先组不可见")
            } else if !simple || !record.has_rgb {
                Capability::unsupported("图层或祖先存在未验证的视觉依赖，或缺少完整 RGB 通道")
            } else if record.bounds.width == 0
                || record.bounds.height == 0
                || layer.raw_data.is_none()
            {
                Capability::unsupported("图层没有正面积位图")
            } else {
                Capability::supported(
                    "简单普通位图的完整通道，保留透明边缘与画布外部分；未执行色彩转换",
                )
            };
            let text = layer
                .additional_info
                .text
                .as_ref()
                .map(super::text::normalize)
                .transpose()?
                .flatten();
            self.output.push(LayerInfo {
                id,
                parent_id,
                source_record_index: record.ordinal,
                name: layer
                    .additional_info
                    .name
                    .clone()
                    .ok_or_else(|| invalid_candidate("缺少图层名称"))?,
                kind,
                bounds: record.bounds,
                visible,
                effective_visible,
                opacity: (opacity * 255.0).round() as u8,
                text,
                diagnostics,
                export,
            });
            self.paths.push(path.clone());
            if let Some(children) = &layer.children {
                self.visit(children, &path, Some(id), effective_visible, simple)?;
            }
        }
        Ok(())
    }
}

fn classify(layer: &Layer, record: &LayerRecord) -> LayerKind {
    let has = |key: &[u8; 4]| record.tags.contains(key);
    let a = &layer.additional_info;
    if layer.children.is_some() {
        LayerKind::Group
    } else if a.text.is_some() || has(b"TySh") {
        LayerKind::Text
    } else if a.placed_layer.is_some() || has(b"SoLd") || has(b"PlLd") {
        LayerKind::SmartObject
    } else if a.vector_fill.is_some()
        || a.vector_mask.is_some()
        || has(b"vscg")
        || has(b"SoCo")
        || has(b"GdFl")
        || has(b"PtFl")
    {
        LayerKind::Shape
    } else if a.adjustment.is_some()
        || [
            b"brit", b"levl", b"curv", b"hue2", b"blnc", b"blwh", b"vibA", b"selc", b"mixr",
            b"grdm", b"phfl", b"expA", b"nvrt", b"post", b"thrs",
        ]
        .iter()
        .any(|key| has(key))
    {
        LayerKind::Adjustment
    } else if a
        .diagnostics
        .iter()
        .any(|d| d.kind == ag_psd::psd::DiagnosticKind::Unknown)
    {
        LayerKind::Unknown
    } else {
        LayerKind::Bitmap
    }
}

fn simple_compositing(layer: &Layer, record: &LayerRecord) -> bool {
    let a = &layer.additional_info;
    layer.opacity == Some(1.0)
        && !record.clipping
        && layer.clipping == Some(false)
        && matches!(&record.blend_mode, b"norm" | b"pass")
        && matches!(layer.blend_mode, Some(BlendMode::Normal | BlendMode::PassThrough))
        && !record.has_mask
        && !record.has_nondefault_blending_ranges
        && record.flags & 0x18 == 0
        && a.mask.is_none()
        && a.real_mask.is_none()
        && a.vector_mask.is_none()
        && a.effects.is_none()
        && a.fill_opacity.is_none_or(|value| value == 1.0)
        && a.knockout != Some(true)
        // 样式/合成语义未验证的块即使候选未报错，也不能获得素材导出能力。
        && record.tags.iter().all(|key| {
            matches!(key, b"lyid" | b"luni" | b"lsct" | b"lsdk" | b"lspf" | b"lclr" | b"lnsr" | b"lyvr" | b"fxrp")
        })
}

fn verify_bounds(layer: &Layer, bounds: Bounds) -> Result<(), PsdError> {
    let expected = [
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.x) + f64::from(bounds.width),
        f64::from(bounds.y) + f64::from(bounds.height),
    ];
    let actual = [layer.left, layer.top, layer.right, layer.bottom];
    if actual
        .iter()
        .zip(expected)
        .any(|(value, expected)| *value != Some(expected))
    {
        return Err(invalid_candidate("候选图层边界与 PSD 原始记录不一致"));
    }
    Ok(())
}

fn diagnostic(value: &ReadDiagnostic, scope: DiagnosticScope) -> Diagnostic {
    Diagnostic {
        scope,
        kind: match value.kind {
            ag_psd::psd::DiagnosticKind::Unknown => DiagnosticKind::Unknown,
            ag_psd::psd::DiagnosticKind::Unsupported => DiagnosticKind::Unsupported,
        },
        tag: value.tag.clone(),
        offset: value.offset as u64,
        message: value.message.clone(),
    }
}

fn invalid_candidate(message: &str) -> PsdError {
    PsdError::new(PsdErrorCode::ParseFailed, message)
}
