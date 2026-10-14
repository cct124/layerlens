//! 选区与清理共用一个后台线程、一个排队名额；确认与选区结果按同一队列发布。

use super::{
    cleanup, error,
    selection::{RESPONSE_BYTES, execute, validate_request},
};
use layerlens_core::{
    cleanup_contract::*,
    documents::{CleanupHandle, selection::SelectionHandle},
    selection_contract::*,
    workspace_contract::*,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

type Reply = oneshot::Sender<Result<SelectionReply, WorkspaceError>>;
enum Operation {
    #[cfg(feature = "dev-mcp")]
    Context(
        Option<SelectionSummaryDto>,
        oneshot::Sender<Result<super::SelectionContext, WorkspaceError>>,
    ),
    Selection(SelectionRequest, Reply),
    Cleanup(
        CleanupRequest,
        oneshot::Sender<Result<CleanupReply, WorkspaceError>>,
    ),
}

pub(crate) struct OperationsClient {
    sender: mpsc::SyncSender<Operation>,
    stopping: Arc<AtomicBool>,
}
pub(crate) struct OperationsWorker {
    thread: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
}
impl OperationsWorker {
    pub(crate) fn start(
        handle: SelectionHandle,
        cleanup_handle: CleanupHandle,
    ) -> Result<(OperationsClient, Self), WorkspaceError> {
        let (sender, receiver) = mpsc::sync_channel::<Operation>(1);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let thread = std::thread::Builder::new()
            .name("layerlens-operations".into())
            .spawn(move || {
                let mut plans = cleanup::Plans::default();
                while !stop.load(Ordering::Acquire) {
                    plans.expire(Instant::now());
                    match receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok(operation) => {
                            if stop.load(Ordering::Acquire) {
                                break;
                            }
                            match operation {
                                #[cfg(feature = "dev-mcp")]
                                Operation::Context(fixed, reply) if !reply.is_closed() => {
                                    let _ =
                                        reply.send(super::selection::read_context(&handle, fixed));
                                }
                                Operation::Selection(request, reply) if !reply.is_closed() => {
                                    let _ = reply.send(execute(&handle, request, RESPONSE_BYTES));
                                }
                                Operation::Cleanup(request, reply) if !reply.is_closed() => {
                                    let _ = reply.send(plans.execute(
                                        &handle,
                                        &cleanup_handle,
                                        request,
                                        Instant::now(),
                                        cleanup::RESPONSE_BYTES,
                                    ));
                                }
                                _ => {}
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
                    format!("无法启动选区与清理后台：{e}"),
                )
            })?;
        Ok((
            OperationsClient {
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
            return Err(error(
                WorkspaceErrorCode::Internal,
                "选区与清理后台异常退出。",
            ));
        }
        Ok(())
    }
}
impl Drop for OperationsWorker {
    fn drop(&mut self) {
        // 正常退出显式处理错误；析构兜底仍必须等待，不能泄漏后台引用。
        if let Err(error) = self.finish() {
            eprintln!("operations_shutdown code={:?}", error.code);
        }
    }
}
impl OperationsClient {
    #[cfg(feature = "dev-mcp")]
    pub(crate) async fn context(
        &self,
        fixed: Option<SelectionSummaryDto>,
    ) -> Result<super::SelectionContext, WorkspaceError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Operation::Context(fixed, sender))?;
        receiver
            .await
            .map_err(|_| error(WorkspaceErrorCode::ShuttingDown, "选区观测后台已停止。"))?
    }
    pub(crate) async fn cleanup(
        &self,
        request: CleanupRequest,
    ) -> Result<CleanupReply, WorkspaceError> {
        cleanup::validate_request(&request)?;
        let (reply, receiver) = oneshot::channel();
        self.send(Operation::Cleanup(request, reply))?;
        receiver.await.map_err(|_| {
            error(
                WorkspaceErrorCode::ShuttingDown,
                "清理请求未完成，后台已停止。",
            )
        })?
    }
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
        let (reply, receiver) = oneshot::channel();
        self.send(Operation::Selection(request, reply))?;
        Ok(receiver)
    }
    fn send(&self, operation: Operation) -> Result<(), WorkspaceError> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(error(
                WorkspaceErrorCode::ShuttingDown,
                "选区与清理服务正在退出。",
            ));
        }
        self.sender.try_send(operation).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => {
                error(WorkspaceErrorCode::Busy, "选区与清理后台已满，请稍后重试。")
            }
            mpsc::TrySendError::Disconnected(_) => {
                error(WorkspaceErrorCode::ShuttingDown, "选区与清理后台已停止。")
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_worker_prepares_cancels_confirms_and_rejects_calls_after_shutdown() {
        let mut service =
            layerlens_core::documents::DocumentService::new(Default::default()).unwrap();
        service
            .open(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../crates/layerlens-core/tests/fixtures/psd/viewer-text.psd"),
            )
            .unwrap()
            .wait_timeout(Duration::from_secs(10))
            .unwrap();
        let lease = service
            .lease(service.snapshot().active_document.unwrap())
            .unwrap();
        let session = service.user_selection().unwrap().session_id;
        let (client, mut worker) =
            OperationsWorker::start(service.selection_handle(), service.cleanup_handle()).unwrap();
        let call = |operation| {
            tauri::async_runtime::block_on(client.cleanup(CleanupRequest {
                protocol_version: layerlens_core::IPC_PROTOCOL_VERSION,
                session_id: session.as_str().to_owned(),
                operation,
            }))
        };
        let prepare = || {
            let CleanupReply::Prepared { plan, .. } = call(CleanupOperation::Prepare {
                target: CleanupTarget::Document {
                    document_id: lease.document_id().get().to_string(),
                    document_revision: lease.revision_id().get().to_string(),
                },
            })
            .unwrap() else {
                panic!("expected plan")
            };
            plan
        };
        let cancelled = prepare();
        assert!(matches!(
            call(CleanupOperation::Cancel {
                plan_id: cancelled.plan_id
            })
            .unwrap(),
            CleanupReply::Cancelled { .. }
        ));
        assert!(lease.ensure_valid().is_ok());
        let plan = prepare();
        let confirm = || CleanupOperation::Confirm {
            plan_id: plan.plan_id.clone(),
        };
        let CleanupReply::Committed { receipt: first, .. } = call(confirm()).unwrap() else {
            panic!("expected receipt")
        };
        let CleanupReply::Committed { receipt: retry, .. } = call(confirm()).unwrap() else {
            panic!("expected receipt")
        };
        assert_eq!(first, retry);
        assert!(lease.ensure_valid().is_err());
        assert!(service.snapshot().documents.is_empty());
        service.request_shutdown();
        assert_eq!(
            call(confirm()).unwrap_err().code,
            WorkspaceErrorCode::ShuttingDown
        );
        worker.finish().unwrap();
        assert_eq!(
            call(confirm()).unwrap_err().code,
            WorkspaceErrorCode::ShuttingDown
        );
        drop(lease);
        service.shutdown().unwrap();
        assert_eq!(service.snapshot().resources.source_bytes, 0);
    }
    #[test]
    fn real_worker_shares_the_owner_session_and_finishes_before_core_release() {
        let mut service =
            layerlens_core::documents::DocumentService::new(Default::default()).unwrap();
        let session = service.user_selection().unwrap().session_id;
        let (client, mut worker) =
            OperationsWorker::start(service.selection_handle(), service.cleanup_handle()).unwrap();
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
        let client = OperationsClient {
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
        let cleanup = || CleanupRequest {
            protocol_version: layerlens_core::IPC_PROTOCOL_VERSION,
            session_id: "a".repeat(32),
            operation: CleanupOperation::Cancel {
                plan_id: "1".into(),
            },
        };
        assert_eq!(
            tauri::async_runtime::block_on(client.cleanup(cleanup()))
                .unwrap_err()
                .code,
            WorkspaceErrorCode::Busy
        );
        client.stop();
        assert_eq!(
            client.enqueue(request()).unwrap_err().code,
            WorkspaceErrorCode::ShuttingDown
        );
        assert_eq!(
            tauri::async_runtime::block_on(client.cleanup(cleanup()))
                .unwrap_err()
                .code,
            WorkspaceErrorCode::ShuttingDown
        );
        drop(receiver);
        assert!(tauri::async_runtime::block_on(pending).is_err());
    }
}
