//! ag-psd 的只读适配器：固定数据所有权、解析配置和按需 PNG 输出。
//!
//! 容器检查与规范化分别由 preflight、normalization 负责；候选类型不泄漏给调用方。

use super::{
    capability::Support,
    error::{PsdError, PsdErrorCode},
    model::{DocumentInfo, LayerId, ParseLimits},
    normalization, preflight,
    source::{SourceData, SourceReadError},
};
use ag_psd::psd::{Psd, ReadOptions};
use sha2::{Digest, Sha256};
use std::path::Path;

/// 固定源版本的解析结果；压缩数据由候选拥有，后续解码不再访问源路径。
///
/// 尚无文档注册、任务和缓存语义。同步解析和解码须由未来有界后台作业调用。
pub struct PsdDocument {
    parsed: Psd,
    info: DocumentInfo,
    paths: Vec<Vec<usize>>,
    source_sha256: String,
    source_size_bytes: usize,
    max_decoded_bytes: u64,
}

impl PsdDocument {
    /// 在受保护读取期间固定源字节；解析完成后关闭文件并释放该源缓冲。
    ///
    /// # Errors
    /// 非普通文件、不能稳定读取、不支持的平台、损坏数据或预算不足时返回领域错误。
    pub fn open(path: &Path, limits: ParseLimits) -> Result<Self, PsdError> {
        let source = SourceData::open(path, limits.max_file_bytes).map_err(|cause| {
            let code = match &cause {
                SourceReadError::TooLarge { .. } => PsdErrorCode::ResourceLimit,
                SourceReadError::UnsupportedPlatform => PsdErrorCode::Unsupported,
                SourceReadError::NotAFile => PsdErrorCode::InvalidInput,
                SourceReadError::Io(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                    PsdErrorCode::SourceChanged
                }
                SourceReadError::Io(_) => PsdErrorCode::Io,
            };
            PsdError::with_cause(code, format!("取得稳定 PSD 源数据失败：{cause}"), cause)
        })?;
        Self::from_bytes(&source.bytes, limits)
    }

    /// 从已固定字节解析结构，保留按需解码的压缩数据；未知能力通过逐项诊断返回。
    ///
    /// # Errors
    /// 外部长度、结构或候选读取错误仍导致失败；只有明确的未实现能力可局部跳过。
    pub fn from_bytes(bytes: &[u8], limits: ParseLimits) -> Result<Self, PsdError> {
        if limits.max_decoded_bytes == 0 {
            return Err(PsdError::new(
                PsdErrorCode::ResourceLimit,
                "单次解码预算必须大于零",
            ));
        }
        let header = preflight::inspect(bytes, limits)?;
        let memory_limit = usize::try_from(limits.max_decoded_bytes)
            .map_err(|_| PsdError::new(PsdErrorCode::ResourceLimit, "解码预算超出可表示范围"))?;
        let parsed = ag_psd::read_psd(
            bytes,
            &ReadOptions {
                use_raw_data: Some(true),
                strict: Some(true),
                throw_for_missing_features: Some(true),
                allow_unsupported_features: true,
                skip_thumbnail: Some(true),
                skip_linked_files_data: Some(true),
                total_memory_limit: Some(memory_limit),
                ..ReadOptions::default()
            },
        )
        .map_err(|cause| parser_error("解析 PSD", cause))?;
        let normalized = normalization::normalize(&parsed, &header, bytes, limits)?;
        Ok(Self {
            parsed,
            info: normalized.info,
            paths: normalized.paths,
            source_sha256: format!("{:x}", Sha256::digest(bytes)),
            source_size_bytes: bytes.len(),
            max_decoded_bytes: limits.max_decoded_bytes,
        })
    }

    /// 规范化元数据、来源原文及逐项能力；不含解码像素。
    pub fn info(&self) -> &DocumentInfo {
        &self.info
    }

    /// 固定源字节的 SHA-256，用于重现实验和确认文件版本。
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    /// 固定源文件的字节数。
    pub fn source_size_bytes(&self) -> usize {
        self.source_size_bytes
    }

    /// 按需解码保存时合成图为原尺寸 PNG，不重新渲染图层。
    ///
    /// partial 预览也可获取，但调用方须同时展示 info().preview 的限制。
    ///
    /// # Errors
    /// 缺少预览或通道语义未验证返回 PreviewUnavailable；超预算、解码和编码失败保留原因。
    pub fn preview_png(&self) -> Result<Vec<u8>, PsdError> {
        if self.info.preview.status == Support::Unsupported {
            return Err(PsdError::new(
                PsdErrorCode::PreviewUnavailable,
                &self.info.preview.reason,
            ));
        }
        self.check_decode_budget(self.info.width, self.info.height)?;
        let pixels = ag_psd::get_composite_image_data(&self.parsed)
            .map_err(|cause| parser_error("解码保存时合成图", cause))?
            .ok_or_else(|| PsdError::new(PsdErrorCode::PreviewUnavailable, "候选没有合成图像素"))?;
        encode_png(pixels, self.info.width, self.info.height)
    }

    /// 导出已验证的简单可见位图为 1× 透明 PNG，保留画布外部分及透明边缘。
    ///
    /// 不提供组、文字、形状、智能对象或含未知视觉依赖的独立素材导出，不创建磁盘文件。
    ///
    /// # Errors
    /// 错误引用返回 LayerNotFound；未支持的导出返回 ExportUnsupported；资源及解码失败保留原因。
    pub fn layer_png(&self, id: LayerId) -> Result<Vec<u8>, PsdError> {
        let index = id.0 as usize;
        let info = self.info.layers.get(index).ok_or_else(layer_not_found)?;
        if info.export.status != Support::Supported {
            return Err(PsdError::new(
                PsdErrorCode::ExportUnsupported,
                &info.export.reason,
            ));
        }
        self.check_decode_budget(info.bounds.width, info.bounds.height)?;
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

    fn check_decode_budget(&self, width: u32, height: u32) -> Result<(), PsdError> {
        let rgba = u64::from(width) * u64::from(height) * 4;
        if rgba > self.max_decoded_bytes {
            return Err(PsdError::new(
                PsdErrorCode::ResourceLimit,
                format!(
                    "RGBA 需要 {rgba} 字节，超过单次解码预算 {} 字节",
                    self.max_decoded_bytes
                ),
            ));
        }
        Ok(())
    }
}

fn layer_not_found() -> PsdError {
    PsdError::new(PsdErrorCode::LayerNotFound, "图层引用不属于本次解析结果")
}
fn parser_error(operation: &str, cause: ag_psd::ReadError) -> PsdError {
    let code = match cause {
        ag_psd::ReadError::ExceededMemoryLimit { .. } => PsdErrorCode::ResourceLimit,
        ag_psd::ReadError::UnsupportedFeature { .. } => PsdErrorCode::Unsupported,
        _ => PsdErrorCode::ParseFailed,
    };
    PsdError::with_cause(code, format!("{operation}失败：{cause}"), cause)
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
