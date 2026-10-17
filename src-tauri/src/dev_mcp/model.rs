//! 开发工具的类型化契约；JSON Schema 直接来自 Rust 类型，不维护另一套参数定义。

use crate::workspace::SelectionContext;
use layerlens_core::workspace_contract::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request<T> {
    pub instance_id: String,
    pub session_id: String,
    /// 有副作用时必填；未知结果重试原 ID 与原参数，不换 ID 重做。
    pub client_request_id: Option<String>,
    pub request: T,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(super) enum Outcome<T> {
    Ok { value: T },
    Error { error: WorkspaceError },
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum Observation {
    Available(Box<SelectionContext>),
    // 后续观测失败不能抹去此前已发生的业务操作和可恢复回执。
    Unavailable { unavailable: WorkspaceError },
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct Reply<T> {
    pub data: Outcome<T>,
    pub user_selection: Observation,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstanceInfo {
    pub instance_id: String,
    pub session_id: String,
    pub app_version: String,
    pub build_profile: String,
    pub protocol_version: u32,
    pub tools: Vec<String>,
    pub active_waits: usize,
    pub limits: Limits,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct Limits {
    pub request_bytes: usize,
    pub reply_bytes: usize,
    pub preview_png_bytes: usize,
    pub connections: usize,
    pub concurrent_operations: usize,
    pub concurrent_waits: usize,
    pub mutation_records: usize,
    pub mutation_bytes: usize,
    pub max_wait_ms: u32,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WaitRequest {
    pub condition: WaitCondition,
    pub timeout_ms: u32,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum WaitCondition {
    Sequence {
        at_least: String,
    },
    DocumentCount {
        count: u32,
    },
    DocumentRevision {
        document_id: String,
        revision: String,
    },
    Idle {},
    ResourcesReleased {},
    PreviewReady {
        document_id: String,
        revision: String,
    },
    FrontendImageReady {
        document_id: String,
        revision: String,
        at_least: String,
    },
    Cleanup {
        at_least: String,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WaitResult {
    pub satisfied: bool,
    pub timed_out: bool,
    pub condition: WaitCondition,
    pub last_state: WorkspaceSnapshot,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewResult {
    pub document_id: String,
    pub revision: String,
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
    pub source: String,
    pub capability: layerlens_core::psd::Capability,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShutdownResult {
    pub instance_id: String,
    pub accepted: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum WorkspaceResult {
    Snapshot {
        snapshot: Box<WorkspaceSnapshot>,
    },
    /// 仅表示操作被接受；打开／重载是否成功应等待工作区作业结果。
    Accepted {
        sequence: String,
        active_document_id: Option<String>,
        job_ids: Vec<String>,
    },
}
