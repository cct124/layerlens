//! 固定快照投影的字节／条数双重分页；文字切片复用查看器的原文和样式规则。

use serde::Serialize;

use super::{
    ContentCursor, LayerIntersection, LayerRole, SelectionError, SelectionWarning, SessionId,
    SnapshotId, SnapshotLease,
};
use crate::documents::{
    DocumentId, LayerDetails, RevisionId,
    inspection::{check_size, shorten_text_slice},
};

/// 原始详情和选区交集分开保存；同一文字图层可跨页重复，计数始终是唯一图层数。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentLayer {
    pub stack_index: u32,
    pub role: LayerRole,
    pub intersection: Option<LayerIntersection>,
    pub details: LayerDetails,
}

/// 当前核心内容页的序列化预算覆盖整个对象；协议适配层仍需另预留外层信封空间。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentPage {
    pub session_id: SessionId,
    pub snapshot_id: SnapshotId,
    pub document_id: DocumentId,
    pub document_revision: RevisionId,
    pub layer_count: u32,
    pub text_layer_count: u32,
    pub layers: Vec<ContentLayer>,
    pub warnings: Vec<SelectionWarning>,
    pub truncated: bool,
    pub next_cursor: Option<ContentCursor>,
}

impl SnapshotLease {
    /// 按固定范围和堆叠顺序读取 1–128 条；结构组随同分页，不扩大读取授权。
    ///
    /// # Errors
    /// 跨快照／会话游标或非法数量拒绝。单条元数据仍无法容纳时明确报资源限制，
    /// 不将缺失能力伪装成待续内容；可分页文字至少保留一个完整 Unicode 标量。
    pub fn content_page(
        &self,
        cursor: Option<&ContentCursor>,
        limit: u32,
    ) -> Result<ContentPage, SelectionError> {
        if limit == 0 || limit > 128 {
            return Err(SelectionError::InvalidInput("内容页条数应为 1–128"));
        }
        if cursor.is_some_and(|cursor| {
            cursor.session_id != *self.session_id() || cursor.snapshot_id != self.id()
        }) {
            return Err(SelectionError::InvalidInput("游标不属于本快照"));
        }
        let records = &self.0.scope.records;
        let mut index = cursor.map_or(0, |cursor| cursor.record) as usize;
        let mut text_start = cursor.map_or(0, |cursor| cursor.text_start);
        if index > records.len() || (index == records.len() && text_start != 0) {
            return Err(SelectionError::InvalidInput("内容游标越界"));
        }
        let mut page = ContentPage {
            session_id: self.session_id().clone(),
            snapshot_id: self.id(),
            document_id: self.document_id(),
            document_revision: self.revision_id(),
            layer_count: records.len() as u32,
            text_layer_count: self.0.scope.text_layer_count,
            layers: Vec::new(),
            warnings: self.scope().warnings.clone(),
            truncated: false,
            next_cursor: None,
        };
        while index < records.len() && page.layers.len() < limit as usize {
            let record = &records[index];
            let details = self.0.document.layer_details(record.layer_id, text_start)?;
            page.layers.push(ContentLayer {
                stack_index: record.stack_index,
                role: record.role,
                intersection: record.intersection,
                details,
            });
            loop {
                let next_text = page
                    .layers
                    .last()
                    .and_then(|layer| layer.details.text.as_ref())
                    .and_then(|text| text.next_start);
                let next_index = if next_text.is_some() {
                    index
                } else {
                    index + 1
                };
                page.next_cursor = (next_index < records.len()).then(|| ContentCursor {
                    session_id: self.session_id().clone(),
                    snapshot_id: self.id(),
                    record: next_index as u32,
                    text_start: next_text.unwrap_or(0),
                });
                page.truncated = page.next_cursor.is_some();
                match check_size(&page, self.0.page_bytes) {
                    Ok(()) => {
                        index = next_index;
                        text_start = next_text.unwrap_or(0);
                        break;
                    }
                    Err(error) if page.layers.len() == 1 => {
                        if !page
                            .layers
                            .last_mut()
                            .and_then(|layer| layer.details.text.as_mut())
                            .is_some_and(shorten_text_slice)
                        {
                            return Err(error.into());
                        }
                    }
                    Err(_) => {
                        page.layers.pop();
                        page.next_cursor = Some(ContentCursor {
                            session_id: self.session_id().clone(),
                            snapshot_id: self.id(),
                            record: index as u32,
                            text_start,
                        });
                        page.truncated = true;
                        check_size(&page, self.0.page_bytes)?;
                        return Ok(page);
                    }
                }
            }
            // 一页中每层至多出现一次；长文字在下一页继续，不暗示字符选区授权。
            if text_start != 0 {
                break;
            }
        }
        check_size(&page, self.0.page_bytes)?;
        Ok(page)
    }
}
