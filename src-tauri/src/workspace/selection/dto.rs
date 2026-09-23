//! 只做有界核心投影到桌面 DTO 的转换，不重新计算授权目标。

use layerlens_core::{documents::selection::*, selection_contract::*};

pub(super) fn scope(value: SnapshotBrief) -> ScopeDto {
    ScopeDto {
        snapshot_id: value.snapshot_id.get().to_string(),
        document_id: value.document_id.get().to_string(),
        document_revision: value.revision_id.get().to_string(),
        document_name: value.document_name,
        target_count: value.target_count,
        content_count: value.content_count,
        text_layer_count: value.text_layer_count,
    }
}

pub(crate) fn summary(value: SelectionSummary) -> SelectionSummaryDto {
    let mut layer_ids = Vec::new();
    let mut region = None;
    for item in value.items {
        match item {
            SelectionItem::Layer { layer_id } => layer_ids.push(layer_id.0),
            SelectionItem::Region { bounds } => region = Some(bounds),
        }
    }
    SelectionSummaryDto {
        session_id: value.session_id.as_str().to_owned(),
        selection_revision: value.revision.get().to_string(),
        document_id: value.document_id.map(|id| id.get().to_string()),
        document_revision: value.document_revision.map(|id| id.get().to_string()),
        scope: value.snapshot.map(scope),
        layer_ids,
        region,
    }
}

pub(super) fn task(value: DesignTask) -> TaskDto {
    TaskDto {
        id: value.id.get().to_string(),
        name: value.name,
        scope: scope(value.scope),
        status: match value.status {
            TaskStatus::Active => TaskStatusDto::Active,
            TaskStatus::Released => TaskStatusDto::Released,
            TaskStatus::Invalidated => TaskStatusDto::Invalidated,
        },
    }
}

pub(super) fn cursor(value: ContentCursor) -> SelectionCursorDto {
    let (session, snapshot, record, text_start) = value.parts();
    SelectionCursorDto {
        session_id: session.as_str().to_owned(),
        snapshot_id: snapshot.get().to_string(),
        record,
        text_start,
    }
}

pub(super) fn content(value: ContentPage) -> SelectionContentDto {
    SelectionContentDto {
        session_id: value.session_id.as_str().to_owned(),
        snapshot_id: value.snapshot_id.get().to_string(),
        document_id: value.document_id.get().to_string(),
        document_revision: value.document_revision.get().to_string(),
        layer_count: value.layer_count,
        text_layer_count: value.text_layer_count,
        layers: value.layers,
        warnings: value.warnings,
        truncated: value.truncated,
        next_cursor: value.next_cursor.map(cursor),
    }
}
