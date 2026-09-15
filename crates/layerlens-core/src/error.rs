//! 可序列化的领域错误，适配层保留代码和可执行的恢复提示。

use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 调用方可稳定判断的错误代码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandErrorCode {
    /// 调用方和核心使用不同的 IPC 契约版本。
    ProtocolMismatch,
}

/// 核心操作失败的公开契约；消息用于提示，分支判断使用错误代码。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// 稳定的机器可读错误代码。
    pub code: CommandErrorCode,
    /// 包含原因及恢复提示的用户消息。
    pub message: String,
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CommandError {}
