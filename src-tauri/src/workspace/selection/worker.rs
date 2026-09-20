//! 一个选区后台线程、一个排队名额；停止准入、丢弃排队请求后等待在途工作退出。

use super::{RESPONSE_BYTES, error, execute, validate_request};
use layerlens_core::{
    documents::selection::SelectionHandle, selection_contract::*, workspace_contract::*,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::oneshot;

type Reply = oneshot::Sender<Result<SelectionReply, WorkspaceError>>;

pub(crate) struct SelectionClient {
    sender: mpsc::SyncSender<(SelectionRequest, Reply)>,
    stopping: Arc<AtomicBool>,
}
pub(crate) struct SelectionWorker {
    thread: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
}
impl SelectionWorker {
    pub(crate) fn start(
        handle: SelectionHandle,
    ) -> Result<(SelectionClient, Self), WorkspaceError> {
        let (sender, receiver) = mpsc::sync_channel::<(SelectionRequest, Reply)>(1);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let thread = std::thread::Builder::new()
            .name("layerlens-selection".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    match receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok((request, reply)) => {
                            if stop.load(Ordering::Acquire) {
                                break;
                            }
                            if !reply.is_closed() {
                                let _ = reply.send(execute(&handle, request, RESPONSE_BYTES));
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                // receiver 与 handle 都在后台析构；所有尚未处理的调用收到明确断连。
            })
            .map_err(|e| {
                error(
                    WorkspaceErrorCode::Internal,
                    format!("无法启动选区后台：{e}"),
                )
            })?;
        Ok((
            SelectionClient {
                sender,
                stopping: stopping.clone(),
            },
            Self {
                thread: Some(thread),
                stopping,
            },
        ))
    }
    pub(crate) fn finish(&mut self) -> Result<(), WorkspaceError> {
        self.stopping.store(true, Ordering::Release);
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            return Err(error(WorkspaceErrorCode::Internal, "选区后台异常退出。"));
        }
        Ok(())
    }
}
impl Drop for SelectionWorker {
    fn drop(&mut self) {
        // 正常退出显式处理错误；析构兜底仍必须等待，不能泄漏后台引用。
        if let Err(error) = self.finish() {
            eprintln!("selection_shutdown code={:?}", error.code);
        }
    }
}
impl SelectionClient {
    pub(crate) fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }
    pub(crate) async fn request(
        &self,
        request: SelectionRequest,
    ) -> Result<SelectionReply, WorkspaceError> {
        self.enqueue(request)?.await.map_err(|_| {
            error(
                WorkspaceErrorCode::ShuttingDown,
                "选区请求未完成，后台已停止。",
            )
        })?
    }
    fn enqueue(
        &self,
        request: SelectionRequest,
    ) -> Result<oneshot::Receiver<Result<SelectionReply, WorkspaceError>>, WorkspaceError> {
        validate_request(&request)?;
        if self.stopping.load(Ordering::Acquire) {
            return Err(error(
                WorkspaceErrorCode::ShuttingDown,
                "选区服务正在退出。",
            ));
        }
        let (reply, receiver) = oneshot::channel();
        self.sender
            .try_send((request, reply))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => {
                    error(WorkspaceErrorCode::Busy, "选区后台已满，请稍后重试。")
                }
                mpsc::TrySendError::Disconnected(_) => {
                    error(WorkspaceErrorCode::ShuttingDown, "选区后台已停止。")
                }
            })?;
        Ok(receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_worker_shares_the_owner_session_and_finishes_before_core_release() {
        let mut service =
            layerlens_core::documents::DocumentService::new(Default::default()).unwrap();
        let session = service.user_selection().unwrap().session_id;
        let (client, mut worker) = SelectionWorker::start(service.selection_handle()).unwrap();
        let request = || SelectionRequest {
            protocol_version: layerlens_core::IPC_PROTOCOL_VERSION,
            session_id: session.as_str().to_owned(),
            operation: SelectionOperation::Tasks {
                after: None,
                limit: 32,
            },
        };
        let result = tauri::async_runtime::block_on(client.request(request())).unwrap();
        let SelectionReply::Tasks { page } = result else {
            panic!("expected tasks")
        };
        assert_eq!(page.session_id, session.as_str());
        assert_eq!(page.total, 0);
        service.request_shutdown();
        assert_eq!(
            tauri::async_runtime::block_on(client.request(request()))
                .unwrap_err()
                .code,
            WorkspaceErrorCode::ShuttingDown
        );
        worker.finish().unwrap();
        service.shutdown().unwrap();
        assert_eq!(service.selection_resources().live_snapshots, 0);
        assert_eq!(
            tauri::async_runtime::block_on(client.request(request()))
                .unwrap_err()
                .code,
            WorkspaceErrorCode::ShuttingDown
        );
    }

    #[test]
    fn bounded_admission_rejects_overflow_and_disconnect_wakes_pending_calls() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let client = SelectionClient {
            sender,
            stopping: Arc::new(AtomicBool::new(false)),
        };
        let request = || SelectionRequest {
            protocol_version: layerlens_core::IPC_PROTOCOL_VERSION,
            session_id: "a".repeat(32),
            operation: SelectionOperation::Tasks {
                after: None,
                limit: 32,
            },
        };
        let pending = client.enqueue(request()).unwrap();
        assert_eq!(
            client.enqueue(request()).unwrap_err().code,
            WorkspaceErrorCode::Busy
        );
        client.stop();
        assert_eq!(
            client.enqueue(request()).unwrap_err().code,
            WorkspaceErrorCode::ShuttingDown
        );
        drop(receiver);
        assert!(tauri::async_runtime::block_on(pending).is_err());
    }
}
