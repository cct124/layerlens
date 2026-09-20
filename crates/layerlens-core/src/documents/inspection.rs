//! 固定修订的有界图层读取与单图层检查选择；不解码像素，不依赖桌面协议。
//! 列表维持 PSD 堆叠顺序，文字分页使用原文 UTF-16 区间，不能拆分代理对。

use serde::Serialize;
use ts_rs::TS;

use super::{DocumentError, DocumentLease, DocumentService, model::lock};
use crate::psd::{
    Bounds, Capability, ExportBlocker, LayerId, LayerInfo, LayerKind, ParagraphStyleRun,
    TextDiagnostic, TextIndexMapping, TextStyleRun,
};

const PAGE_LIMIT: u32 = 128;
const TEXT_UNITS: u32 = 2048;
const RUN_LIMIT: usize = 128;
const RESPONSE_BYTES: u64 = 256 * 1024;

/// 图层树摘要。名称最多 256 个 Unicode 标量；完整设计文字仅按需读取。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LayerSummary {
    pub id: LayerId,
    pub parent_id: Option<LayerId>,
    pub name: String,
    pub name_truncated: bool,
    pub kind: LayerKind,
    pub bounds: Bounds,
    pub visible: bool,
    pub effective_visible: bool,
    pub opacity: u8,
}

/// 固定修订内的顺序分页；offset 是规范化图层位置，不是 PSD 原始记录号。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LayerPage {
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub layers: Vec<LayerSummary>,
}

/// 原文切片与相交样式段；样式区间仍使用完整原文的绝对索引。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextSlice {
    pub text: String,
    pub start: u32,
    pub end: u32,
    pub total_length: u32,
    pub next_start: Option<u32>,
    pub transform: Option<[f64; 6]>,
    pub styles: Capability,
    pub index_mapping: Option<TextIndexMapping>,
    pub style_runs: Option<Vec<TextStyleRun>>,
    pub paragraph_runs: Option<Vec<ParagraphStyleRun>>,
    pub diagnostics: Vec<TextDiagnostic>,
    pub diagnostics_truncated: bool,
}

/// 图层只读详情。几何边界直接来自 PSD，不推断视觉范围或 CSS。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LayerDetails {
    pub layer: LayerSummary,
    pub export: Capability,
    pub export_blockers: Vec<ExportBlocker>,
    pub diagnostics: Vec<String>,
    pub diagnostics_truncated: bool,
    pub text: Option<TextSlice>,
}

impl From<&LayerInfo> for LayerSummary {
    fn from(layer: &LayerInfo) -> Self {
        Self {
            id: layer.id,
            parent_id: layer.parent_id,
            name: layer.name.chars().take(256).collect(),
            name_truncated: layer.name.chars().nth(256).is_some(),
            kind: layer.kind,
            bounds: layer.bounds,
            visible: layer.visible,
            effective_visible: layer.effective_visible,
            opacity: layer.opacity,
        }
    }
}

impl DocumentLease {
    /// 按规范化堆叠顺序读取 1–128 项摘要；空文档允许 offset=0。
    ///
    /// # Errors
    /// 越界分页或非法数量返回 InvalidQuery，序列化预算不足返回 ResourceLimit。
    pub fn layers(&self, offset: u32, limit: u32) -> Result<LayerPage, DocumentError> {
        let layers = &self.info().layers;
        if limit == 0 || limit > PAGE_LIMIT || offset as usize > layers.len() {
            return Err(DocumentError::InvalidQuery("分页位置或数量越界"));
        }
        let end = (offset as usize + limit as usize).min(layers.len());
        let page = LayerPage {
            offset,
            total: layers.len() as u32,
            next_offset: (end < layers.len()).then_some(end as u32),
            layers: layers[offset as usize..end]
                .iter()
                .map(Into::into)
                .collect(),
        };
        check_size(&page, RESPONSE_BYTES)?;
        Ok(page)
    }

    /// 读取一个图层及从 text_start 开始的有界文字片段（原文 UTF-16 单位）。
    /// 文字页按样式数量和完整详情的 JSON 字节预算缩短，续页沿用绝对索引。
    ///
    /// # Errors
    /// 图层不存在、起点拆分代理对或超过原文时拒绝；单项属性或不可分页元数据超限时报资源限制。
    pub fn layer_details(
        &self,
        id: LayerId,
        text_start: u32,
    ) -> Result<LayerDetails, DocumentError> {
        let layer = self
            .info()
            .layers
            .iter()
            .find(|layer| layer.id == id)
            .ok_or(DocumentError::LayerNotFound(id))?;
        inspect_layer(layer, text_start)
    }
}

impl DocumentService {
    /// 提交查看器单图层选择；隐藏层可检查，None 清空。重载与提交在同一锁中裁决。
    /// 本选择尚不创建 M2 任务快照，也不授权导出。
    ///
    /// # Errors
    /// 关闭、退出、旧修订或跨修订图层引用被拒绝，原选择保持不变。
    pub fn select_layer(
        &self,
        lease: &DocumentLease,
        layer: Option<LayerId>,
    ) -> Result<(), DocumentError> {
        if let Some(id) = layer
            && !lease.info().layers.iter().any(|item| item.id == id)
        {
            return Err(DocumentError::LayerNotFound(id));
        }
        let mut state = lock(&self.shared.state);
        state.ensure_running()?;
        let entry = state
            .documents
            .iter_mut()
            .find(|entry| entry.revision.document_id == lease.document_id())
            .ok_or(DocumentError::NotFound(lease.document_id()))?;
        if !std::sync::Arc::ptr_eq(&entry.revision, &lease.0) {
            return Err(DocumentError::StaleRevision);
        }
        entry.selected_layer = layer;
        Ok(())
    }
}

fn inspect_layer(layer: &LayerInfo, text_start: u32) -> Result<LayerDetails, DocumentError> {
    let text = match &layer.text {
        Some(text) => Some(text_slice(text, text_start)?),
        None if text_start != 0 => {
            return Err(DocumentError::InvalidQuery("非文字图层不能指定文字偏移"));
        }
        None => None,
    };
    let mut details = LayerDetails {
        layer: layer.into(),
        export: layer.export.clone(),
        export_blockers: layer.export_blockers.clone(),
        diagnostics: layer
            .diagnostics
            .iter()
            .take(32)
            .map(|d| d.message.chars().take(1024).collect())
            .collect(),
        diagnostics_truncated: layer.diagnostics.len() > 32
            || layer
                .diagnostics
                .iter()
                .take(32)
                .any(|d| d.message.chars().nth(1024).is_some()),
        text,
    };
    // 数量上限不能保证序列化字节上限；每次至少减去一个 Unicode 标量，
    // 直到整页可容纳，或不可再分的属性／元数据明确失败，不丢弃样式冒充成功。
    while let Err(error) = check_size(&details, RESPONSE_BYTES) {
        if !details.text.as_mut().is_some_and(shorten_text_slice) {
            return Err(error);
        }
    }
    Ok(details)
}

fn shorten_text_slice(text: &mut TextSlice) -> bool {
    let mut units = (text.end - text.start) / 2;
    let byte = match utf16_byte(&text.text, units) {
        Some(byte) => byte,
        None => {
            // 中点落在代理对内部时向后取整，保留至少一个完整标量。
            units += 1;
            let Some(byte) = utf16_byte(&text.text, units) else {
                return false;
            };
            byte
        }
    };
    if byte == 0 || byte == text.text.len() {
        return false;
    }
    text.text.truncate(byte);
    text.end = text.start + units;
    text.next_start = Some(text.end);
    if let Some(runs) = &mut text.style_runs {
        runs.retain(|run| run.start < text.end);
    }
    if let Some(runs) = &mut text.paragraph_runs {
        runs.retain(|run| run.start < text.end);
    }
    true
}

fn text_slice(text: &crate::psd::TextData, start: u32) -> Result<TextSlice, DocumentError> {
    if start > text.utf16_length {
        return Err(DocumentError::InvalidQuery("文字起点超过原文"));
    }
    let mut end = start.saturating_add(TEXT_UNITS).min(text.utf16_length);
    // 先按两类样式段共同缩短页尾，避免一页文本引出无界的样式数组。
    if let Some(runs) = &text.style_runs
        && let Some(run) = runs
            .iter()
            .filter(|run| run.end > start && run.start < end)
            .nth(RUN_LIMIT)
    {
        end = end.min(run.start);
    }
    if let Some(runs) = &text.paragraph_runs
        && let Some(run) = runs
            .iter()
            .filter(|run| run.end > start && run.start < end)
            .nth(RUN_LIMIT)
    {
        end = end.min(run.start);
    }
    let begin_byte = utf16_byte(&text.raw_text, start)
        .ok_or(DocumentError::InvalidQuery("文字起点拆分 UTF-16 代理对"))?;
    let end_byte = match utf16_byte(&text.raw_text, end) {
        Some(byte) => byte,
        None => {
            end -= 1;
            utf16_byte(&text.raw_text, end).ok_or(DocumentError::InvalidQuery("文字区间无效"))?
        }
    };
    let style_runs = text
        .style_runs
        .as_ref()
        .map(|runs| {
            runs.iter()
                .filter(|run| run.end > start && run.start < end)
                .map(bounded_clone)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let paragraph_runs = text
        .paragraph_runs
        .as_ref()
        .map(|runs| {
            runs.iter()
                .filter(|run| run.end > start && run.start < end)
                .map(bounded_clone)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    Ok(TextSlice {
        text: text.raw_text[begin_byte..end_byte].into(),
        start,
        end,
        total_length: text.utf16_length,
        next_start: (end < text.utf16_length).then_some(end),
        transform: text.transform,
        styles: text.styles.clone(),
        index_mapping: text.index_mapping,
        style_runs,
        paragraph_runs,
        diagnostics: text
            .diagnostics
            .iter()
            .take(32)
            .map(bounded_clone)
            .collect::<Result<_, _>>()?,
        diagnostics_truncated: text.diagnostics.len() > 32,
    })
}

fn utf16_byte(text: &str, target: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units == target {
            return Some(byte);
        }
        units += character.len_utf16() as u32;
        if units > target {
            return None;
        }
    }
    (units == target).then_some(text.len())
}

fn bounded_clone<T: Clone + Serialize>(value: &T) -> Result<T, DocumentError> {
    check_size(value, 32 * 1024)?;
    Ok(value.clone())
}

// 只计数、不先分配完整 JSON；单项在克隆前检查，避免大字体名被重复复制。
fn check_size(value: &impl Serialize, limit: u64) -> Result<(), DocumentError> {
    struct Counter(u64);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() as u64 > self.0 {
                return Err(std::io::Error::other("响应超过预算"));
            }
            self.0 -= bytes.len() as u64;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(limit), value).map_err(|_| DocumentError::ResourceLimit {
        resource: "图层查询响应字节",
        limit,
    })
}

#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;
