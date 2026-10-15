//! 命名测试控制的协议；控制身份和业务状态分离，恢复释放无需等待变更执行名额。
use crate::workspace::testing::{Checkpoint, Gate, Mode, Phase};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum ControlRequest {
    AdvancePlanClock {
        seconds: u32,
    },
    Arm {
        checkpoint: Checkpoint,
        mode: Mode,
        timeout_ms: u32,
    },
    Release {
        id: u32,
    },
    State {
        id: u32,
    },
    Wait {
        id: u32,
        phase: Phase,
        timeout_ms: u32,
    },
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum ControlResult {
    Clock { offset_seconds: u64 },
    Gate { gate: Gate },
    Wait { satisfied: bool, gate: Gate },
}
