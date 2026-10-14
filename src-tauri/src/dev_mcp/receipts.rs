//! 会话内有界变更回执。先预留再执行，不因断连重做；已接受操作独立于等待它的 RPC。

use crate::workspace::{check_size, error};
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
};
use tokio::sync::{Semaphore, watch};
use tokio_util::task::TaskTracker;

pub(super) const RECORD_LIMIT: usize = 128;
pub(super) const BYTE_LIMIT: usize = 8 * 1024 * 1024;
pub(super) const RESULT_BYTES: usize = 256 * 1024;
type ResultValue = Result<Value, WorkspaceError>;
type ResultReceiver = watch::Receiver<Option<Arc<ResultValue>>>;

struct Entry {
    arguments: Vec<u8>,
    result: ResultReceiver,
}
#[derive(Default)]
struct State {
    entries: BTreeMap<String, Entry>,
    bytes: usize,
}

pub(super) struct Receipts {
    state: Arc<Mutex<State>>,
    execution: Arc<Semaphore>,
    tasks: TaskTracker,
}
impl Default for Receipts {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            execution: Arc::new(Semaphore::new(2)),
            tasks: TaskTracker::new(),
        }
    }
}

impl Receipts {
    pub(super) fn start(
        &self,
        id: &str,
        arguments: Vec<u8>,
        operation: impl Future<Output = ResultValue> + Send + 'static,
    ) -> Result<ResultReceiver, WorkspaceError> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        {
            return Err(error(
                WorkspaceErrorCode::InvalidInput,
                "clientRequestId 需为 1–128 个 ASCII 字母、数字或 -_.:。",
            ));
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = state.entries.get(id) {
            return if entry.arguments == arguments {
                Ok(entry.result.clone())
            } else {
                Err(error(
                    WorkspaceErrorCode::RequestConflict,
                    "此 clientRequestId 已用于不同工具或参数。",
                ))
            };
        }
        if self.tasks.is_closed() {
            return Err(error(
                WorkspaceErrorCode::ShuttingDown,
                "变更后台正在退出；未接受新操作。",
            ));
        }
        let reserved = RESULT_BYTES + arguments.len() + id.len();
        if state.entries.len() >= RECORD_LIMIT || reserved > BYTE_LIMIT.saturating_sub(state.bytes)
        {
            return Err(error(
                WorkspaceErrorCode::ResourceLimit,
                "本次测试会话的变更回执已满；未执行新操作。",
            ));
        }
        let permit = self
            .execution
            .clone()
            .try_acquire_owned()
            .map_err(|_| error(WorkspaceErrorCode::Busy, "开发变更后台已满；未接受新操作。"))?;
        let (sender, receiver) = watch::channel(None);
        state.bytes += reserved;
        state.entries.insert(
            id.to_owned(),
            Entry {
                arguments,
                result: receiver.clone(),
            },
        );
        let shared = self.state.clone();
        // 登记任务与 finish 的关闭共用短锁，防止关闭观察到空后台后又接受新变更。
        self.tasks.spawn(async move {
            let _permit = permit;
            let result = operation.await;
            let result = if check_size(&result, RESULT_BYTES).is_ok() {
                result
            } else {
                Err(error(
                    WorkspaceErrorCode::ResourceLimit,
                    "操作已结束但回执超过预算；请查询工作区，不能换 ID 重做。",
                ))
            };
            let used = serde_json::to_vec(&result).map_or(RESULT_BYTES, |b| b.len());
            shared.lock().unwrap_or_else(|p| p.into_inner()).bytes -=
                RESULT_BYTES.saturating_sub(used);
            sender.send_replace(Some(Arc::new(result)));
        });
        drop(state);
        Ok(receiver)
    }

    pub(super) async fn finish(&self) {
        {
            let _state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            self.tasks.close();
        }
        self.tasks.wait().await;
    }

    pub(super) async fn receive(mut receiver: ResultReceiver) -> ResultValue {
        loop {
            if let Some(result) = receiver.borrow_and_update().clone() {
                return (*result).clone();
            }
            receiver.changed().await.map_err(|_| {
                error(
                    WorkspaceErrorCode::Internal,
                    "变更后台异常结束，结果不确定；请查询状态并仅重试原 ID。",
                )
            })?;
        }
    }
}
