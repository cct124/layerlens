//! 桌面主动清理契约。计划对象保留在服务端，仅传递有界影响摘要和不可复用的会话标识。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupRequest {
    pub protocol_version: u32,
    pub session_id: String,
    pub operation: CleanupOperation,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CleanupTarget {
    Document {
        document_id: String,
        document_revision: String,
    },
    Task {
        task_id: String,
    },
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CleanupOperation {
    Prepare { target: CleanupTarget },
    Confirm { plan_id: String },
    Cancel { plan_id: String },
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupTaskDto {
    pub id: String,
    pub name: String,
    pub document_revision: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanDto {
    pub plan_id: String,
    pub document_id: String,
    pub document_name: String,
    pub open_revision: Option<String>,
    pub selection_id: Option<String>,
    pub live_revisions: Vec<String>,
    pub tasks: Vec<CleanupTaskDto>,
    pub load_jobs: Vec<String>,
    pub unresolved_load_jobs: Vec<String>,
    pub preview_jobs: Vec<String>,
    pub expires_in_seconds: u32,
}

/// 清理代次和全部失效任务 ID 与文档列表同锁采样；集合受核心任务记录额度约束。
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupStateDto {
    pub revision: String,
    pub invalidated_task_ids: Vec<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReceiptDto {
    pub plan_id: String,
    pub document_id: String,
    pub revision: String,
    pub invalidated_task_ids: Vec<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CleanupReply {
    Prepared {
        session_id: String,
        plan: CleanupPlanDto,
    },
    Committed {
        session_id: String,
        receipt: CleanupReceiptDto,
    },
    Cancelled {
        session_id: String,
        plan_id: String,
    },
}
