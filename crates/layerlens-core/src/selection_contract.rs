//! 桌面选区与任务的有界传输契约；不承担核心授权或生命周期规则。
//! 通知只含轻量摘要，详情按固定快照读取；ID 均为字符串。

use crate::documents::selection::{ContentLayer, SelectionBounds, SelectionWarning};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionRequest {
    pub protocol_version: u32,
    pub session_id: String,
    pub operation: SelectionOperation,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SelectionOperation {
    Layers {
        document_id: String,
        document_revision: String,
        expected_revision: String,
        layer_ids: Vec<u32>,
    },
    Region {
        document_id: String,
        document_revision: String,
        expected_revision: String,
        bounds: SelectionBounds,
    },
    Clear {
        document_id: String,
        document_revision: String,
        expected_revision: String,
    },
    CreateTask {
        snapshot_id: String,
        request_id: String,
        name: String,
    },
    ReleaseTask {
        task_id: String,
    },
    Tasks {
        after: Option<String>,
        limit: u32,
    },
    Content {
        target: SelectionTarget,
        cursor: Option<SelectionCursorDto>,
        limit: u32,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SelectionTarget {
    Snapshot { snapshot_id: String },
    Task { task_id: String },
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionCursorDto {
    pub session_id: String,
    pub snapshot_id: String,
    pub record: u32,
    pub text_start: u32,
}

/// 任务保留固定文档标签与计数；标签可能截短，不是唯一标识。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScopeDto {
    pub snapshot_id: String,
    pub document_id: String,
    pub document_revision: String,
    pub document_name: String,
    pub target_count: u32,
    pub content_count: u32,
    pub text_layer_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSummaryDto {
    pub session_id: String,
    pub selection_revision: String,
    pub document_id: Option<String>,
    pub document_revision: Option<String>,
    pub scope: Option<ScopeDto>,
    pub layer_ids: Vec<u32>,
    pub region: Option<SelectionBounds>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatusDto {
    Active,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub id: String,
    pub name: String,
    pub status: TaskStatusDto,
    pub scope: ScopeDto,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskPageDto {
    pub session_id: String,
    pub tasks: Vec<TaskDto>,
    pub next_after: Option<String>,
    pub total: u32,
    pub capacity: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SelectionContentDto {
    pub session_id: String,
    pub snapshot_id: String,
    pub document_id: String,
    pub document_revision: String,
    pub layer_count: u32,
    pub text_layer_count: u32,
    pub layers: Vec<ContentLayer>,
    pub warnings: Vec<SelectionWarning>,
    pub truncated: bool,
    pub next_cursor: Option<SelectionCursorDto>,
}

/// 变更返回可预留的小型回执；最新选择另由工作区原子摘要通知，不拼接两次采样。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SelectionReply {
    Committed {
        session_id: String,
        selection_revision: String,
        snapshot_id: Option<String>,
    },
    Task {
        session_id: String,
        task: TaskDto,
    },
    Tasks {
        page: TaskPageDto,
    },
    Content {
        target: SelectionTarget,
        page: SelectionContentDto,
    },
}
