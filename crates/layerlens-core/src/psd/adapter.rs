//! ag-psd 的实验适配器：拥有解析结果，不持有源路径，也不向调用方泄漏第三方类型。

use std::path::Path;

use ag_psd::psd::{BlendMode, Layer, Psd, ReadOptions};
use sha2::{Digest, Sha256};

use super::{
    error::{PsdError, PsdErrorCode},
    model::{
        Bounds, Capability, DocumentInfo, LayerId, LayerInfo, LayerKind, ParseLimits, Support,
    },
    preflight,
    source::{SourceData, SourceReadError},
};

/// 固定源版本的解析结果，压缩像素由候选库拥有；之后解码不再访问原文件。
///
/// 当前对象无文档注册、任务或缓存语义。同步解析和解码应由未来应用服务在
/// 有界后台作业中调用，不能直接放进桌面事件处理器。
pub struct PsdDocument {
    parsed: Psd,
    info: DocumentInfo,
    paths: Vec<Vec<usize>>,
    source_sha256: String,
    source_size_bytes: usize,
}

impl PsdDocument {
    /// 在受保护读取期间取得稳定源数据，解析完成后关闭文件并释放源字节。
    ///
    /// # Errors
    /// 非普通文件、I/O 失败、不支持的平台或格式、无效数据及超预算时返回领域错误。
    pub fn open(path: &Path, limits: ParseLimits) -> Result<Self, PsdError> {
        let source = SourceData::open(path, limits.max_file_bytes).map_err(|cause| {
            let code = match &cause {
                SourceReadError::TooLarge { .. } => PsdErrorCode::ResourceLimit,
                SourceReadError::UnsupportedPlatform => PsdErrorCode::Unsupported,
                SourceReadError::NotAFile => PsdErrorCode::InvalidInput,
                // Windows sharing/lock violation 表示当前源版本无法稳定读取，不回退为无保护读取。
                SourceReadError::Io(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                    PsdErrorCode::SourceChanged
                }
                SourceReadError::Io(_) => PsdErrorCode::Io,
            };
            PsdError::with_cause(code, format!("取得稳定 PSD 源数据失败：{cause}"), cause)
        })?;
        Self::from_bytes(&source.bytes, limits)
    }

    /// 从调用方已固定的字节解析；返回值拥有后续解码所需的压缩数据。
    ///
    /// # Errors
    /// 输入预检或候选解析失败时返回错误。原型限制见样本与验收记录。
    pub fn from_bytes(bytes: &[u8], limits: ParseLimits) -> Result<Self, PsdError> {
        let header = preflight::inspect(bytes, limits)?;
        let memory_limit = limits
            .max_total_pixels
            .checked_mul(4)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| PsdError::new(PsdErrorCode::ResourceLimit, "像素预算超出可表示范围"))?;
        let parsed = ag_psd::read_psd(
            bytes,
            &ReadOptions {
                use_raw_data: Some(true),
                strict: Some(true),
                skip_thumbnail: Some(true),
                skip_linked_files_data: Some(true),
                total_memory_limit: Some(memory_limit),
                ..ReadOptions::default()
            },
        )
        .map_err(|cause| parser_error("解析 PSD", cause))?;

        let preview = if !header.has_composite {
            Capability::unsupported("没有保存时合成图；不自动重新合成")
        } else if header.global_alpha {
            Capability::unsupported("候选懒解码丢失合成透明度状态，本轮拒绝该预览")
        } else if parsed.channels != Some(3.0) {
            Capability::unsupported("本轮未验证额外合成通道与透明度的区别，仅开放三通道预览")
        } else {
            Capability::supported("保存时合成图的 RAW/RLE 解码；未执行 ICC 色彩转换")
        };
        let resolution_dpi = parsed
            .image_resources
            .as_ref()
            .and_then(|resources| resources.resolution_info.as_ref())
            .map(|resolution| {
                [
                    // ResolutionInfo 的数值始终为每英寸像素；unit 只是显示偏好。
                    resolution.horizontal_resolution,
                    resolution.vertical_resolution,
                ]
            });
        let mut layers = Vec::new();
        let mut paths = Vec::new();
        normalize_layers(
            parsed.children.as_deref().unwrap_or_default(),
            &[],
            None,
            true,
            true,
            &mut layers,
            &mut paths,
        )?;
        let info = DocumentInfo {
            width: header.width,
            height: header.height,
            bit_depth: 8,
            color_mode: "RGB".into(),
            resolution_dpi,
            color_profile: None,
            layers,
            preview,
            text: Capability::unsupported("本轮尚未规范化或验证文字与分段样式"),
            warnings: vec![
                "仅验证简单 RGB/8 位 PSD；复杂附加信息在预检阶段明确拒绝".into(),
                "色彩配置未规范化，PNG 保留源通道值，未宣称 sRGB 或原稿色彩一致".into(),
            ],
        };
        Ok(Self {
            parsed,
            info,
            paths,
            source_sha256: format!("{:x}", Sha256::digest(bytes)),
            source_size_bytes: bytes.len(),
        })
    }

    /// 规范化元数据与逐对象能力说明，不包含解码像素。
    pub fn info(&self) -> &DocumentInfo {
        &self.info
    }

    /// 本次解析源字节的 SHA-256，用于复现实验及核对输入版本。
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    /// 本次源文件的实际字节数。
    pub fn source_size_bytes(&self) -> usize {
        self.source_size_bytes
    }

    /// 按需解码保存时合成图，生成原尺寸 RGBA8 PNG，不重新合成图层。
    ///
    /// # Errors
    /// 无合成图或已知候选限制返回 `PreviewUnavailable`；解码及 PNG 编码错误保留原因。
    pub fn preview_png(&self) -> Result<Vec<u8>, PsdError> {
        if self.info.preview.status != Support::Supported {
            return Err(PsdError::new(
                PsdErrorCode::PreviewUnavailable,
                &self.info.preview.reason,
            ));
        }
        let pixels = ag_psd::get_composite_image_data(&self.parsed)
            .map_err(|cause| parser_error("解码保存时合成图", cause))?
            .ok_or_else(|| {
                PsdError::new(PsdErrorCode::PreviewUnavailable, "候选未提供合成图像素")
            })?;
        encode_png(pixels, self.info.width, self.info.height)
    }

    /// 导出简单、有效可见、不透明度为 255 的普通位图为 1× 透明 PNG。
    ///
    /// 保留完整图层矩形和透明边缘，不与画布相交裁切。不处理蒙版、混合或效果。
    /// 返回字节由调用方拥有，本方法不创建磁盘文件。
    ///
    /// # Errors
    /// 图层引用无效返回 `LayerNotFound`；组、隐藏层或复杂视觉依赖返回 `ExportUnsupported`。
    pub fn layer_png(&self, id: LayerId) -> Result<Vec<u8>, PsdError> {
        let index = id.0 as usize;
        let info = self.info.layers.get(index).ok_or_else(layer_not_found)?;
        if info.export.status != Support::Supported {
            return Err(PsdError::new(
                PsdErrorCode::ExportUnsupported,
                &info.export.reason,
            ));
        }
        let path = self.paths.get(index).ok_or_else(layer_not_found)?;
        let mut children = self.parsed.children.as_deref().unwrap_or_default();
        let mut target = None;
        for index in path {
            let layer = children.get(*index).ok_or_else(layer_not_found)?;
            target = Some(layer);
            children = layer.children.as_deref().unwrap_or_default();
        }
        let pixels = ag_psd::get_layer_image_data(target.ok_or_else(layer_not_found)?)
            .map_err(|cause| parser_error("解码图层位图", cause))?
            .ok_or_else(|| PsdError::new(PsdErrorCode::ExportUnsupported, "图层没有可解码位图"))?;
        encode_png(pixels, info.bounds.width, info.bounds.height)
    }
}

fn layer_not_found() -> PsdError {
    PsdError::new(PsdErrorCode::LayerNotFound, "图层引用不属于本次解析结果")
}

fn parser_error(operation: &str, cause: ag_psd::ReadError) -> PsdError {
    let code = match cause {
        ag_psd::ReadError::ExceededMemoryLimit { .. } => PsdErrorCode::ResourceLimit,
        _ => PsdErrorCode::ParseFailed,
    };
    PsdError::with_cause(code, format!("{operation}失败：{cause}"), cause)
}

fn normalize_layers(
    children: &[Layer],
    parent_path: &[usize],
    parent_id: Option<LayerId>,
    parent_visible: bool,
    parent_simple: bool,
    output: &mut Vec<LayerInfo>,
    paths: &mut Vec<Vec<usize>>,
) -> Result<(), PsdError> {
    // 候选 children 保持 PSD 的从下到上顺序；规范化接口按面板从上到下展开。
    for (index, layer) in children.iter().enumerate().rev() {
        let id = LayerId(
            u32::try_from(output.len())
                .map_err(|_| PsdError::new(PsdErrorCode::ResourceLimit, "图层引用超出范围"))?,
        );
        let mut path = parent_path.to_vec();
        path.push(index);
        let visible = !layer.hidden.ok_or_else(|| missing("可见性"))?;
        let effective_visible = parent_visible && visible;
        let opacity = layer.opacity.ok_or_else(|| missing("不透明度"))?;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(missing("合法不透明度"));
        }
        let bounds = bounds(layer)?;
        let kind = if layer.children.is_some() {
            LayerKind::Group
        } else {
            LayerKind::Bitmap
        };
        let simple = parent_simple
            && opacity == 1.0
            && layer.clipping == Some(false)
            && matches!(
                layer.blend_mode,
                Some(BlendMode::Normal | BlendMode::PassThrough)
            );
        let export = if kind == LayerKind::Group {
            Capability::unsupported("本轮不提供组的独立合成")
        } else if !effective_visible {
            Capability::unsupported("图层或祖先组不可见")
        } else if !simple || layer.blend_mode != Some(BlendMode::Normal) {
            Capability::unsupported("图层或祖先含尚未验证的不透明度、混合或剪贴依赖")
        } else if bounds.width == 0 || bounds.height == 0 || layer.raw_data.is_none() {
            Capability::unsupported("图层无正面积位图")
        } else {
            Capability::supported("简单普通位图的完整 RAW/RLE 通道，保留透明边缘和画布外部分")
        };
        output.push(LayerInfo {
            id,
            parent_id,
            name: layer
                .additional_info
                .name
                .clone()
                .ok_or_else(|| missing("图层名称"))?,
            kind,
            bounds,
            visible,
            effective_visible,
            opacity: (opacity * 255.0).round() as u8,
            export,
        });
        paths.push(path.clone());
        if let Some(children) = &layer.children {
            normalize_layers(
                children,
                &path,
                Some(id),
                effective_visible,
                simple,
                output,
                paths,
            )?;
        }
    }
    Ok(())
}

fn missing(field: &str) -> PsdError {
    PsdError::new(
        PsdErrorCode::ParseFailed,
        format!("候选未返回可用的{field}"),
    )
}

fn bounds(layer: &Layer) -> Result<Bounds, PsdError> {
    let coordinate = |value: Option<f64>| -> Result<i32, PsdError> {
        let value = value.ok_or_else(|| missing("图层坐标"))?;
        if !value.is_finite()
            || value.fract() != 0.0
            || value < i32::MIN as f64
            || value > i32::MAX as f64
        {
            return Err(missing("整数 PSD 图层坐标"));
        }
        Ok(value as i32)
    };
    let x = coordinate(layer.left)?;
    let y = coordinate(layer.top)?;
    let right = coordinate(layer.right)?;
    let bottom = coordinate(layer.bottom)?;
    let width = u32::try_from(i64::from(right) - i64::from(x)).map_err(|_| missing("图层宽度"))?;
    let height =
        u32::try_from(i64::from(bottom) - i64::from(y)).map_err(|_| missing("图层高度"))?;
    Ok(Bounds {
        x,
        y,
        width,
        height,
    })
}

fn encode_png(pixels: ag_psd::PixelData, width: u32, height: u32) -> Result<Vec<u8>, PsdError> {
    let length = u64::from(width) * u64::from(height) * 4;
    if pixels.width != width || pixels.height != height || pixels.data.len() as u64 != length {
        return Err(PsdError::new(
            PsdErrorCode::ParseFailed,
            "解码像素尺寸与原始矩形不一致",
        ));
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(png_error)?;
        writer.write_image_data(&pixels.data).map_err(png_error)?;
        writer.finish().map_err(png_error)?;
    }
    Ok(bytes)
}

fn png_error(cause: png::EncodingError) -> PsdError {
    PsdError::with_cause(
        PsdErrorCode::EncodeFailed,
        format!("编码 PNG 失败：{cause}"),
        cause,
    )
}
