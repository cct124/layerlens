//! 有界桌面邮箱。一个观察线程查询核心作业，不为每个作业创建阻塞等待线程。

use super::{engine::Engine, error};
use layerlens_core::{
    documents::{DocumentServiceConfig, PreviewLease},
    workspace_contract::*,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::sync::oneshot;

const MAILBOX_LIMIT: usize = 16;
const OBSERVE_INTERVAL: Duration = Duration::from_millis(50);

enum Message {
    Action(
        WorkspaceAction,
        oneshot::Sender<Result<WorkspaceSnapshot, WorkspaceError>>,
    ),
    Image(
        PreviewRequest,
        oneshot::Sender<Result<PreviewLease, WorkspaceError>>,
    ),
}

/// 应用持有的适配入口；退出只发信号，由后台释放全部对象后通知事件循环退出。
pub(crate) struct WorkspaceHost {
    sender: mpsc::SyncSender<Message>,
    stopping: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    pub(crate) image_reading: Arc<AtomicBool>,
    pub(crate) picking: Arc<AtomicBool>,
}

impl WorkspaceHost {
    pub(crate) fn start(
        config: DocumentServiceConfig,
        notify: impl Fn(&WorkspaceSnapshot) + Send + 'static,
        on_exit: impl FnOnce(Result<(), WorkspaceError>) + Send + 'static,
    ) -> Result<Self, WorkspaceError> {
        let mut engine = Engine::new(config)?;
        let (sender, receiver) = mpsc::sync_channel(MAILBOX_LIMIT);
        let stopping = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_signal = stopping.clone();
        let exit_signal = stopped.clone();
        std::thread::Builder::new()
            .name("layerlens-desktop-workspace".into())
            .spawn(move || {
                let result = (|| {
                    loop {
                        if stop_signal.load(Ordering::Acquire) {
                            break;
                        }
                        match receiver.recv_timeout(OBSERVE_INTERVAL) {
                            Ok(Message::Action(action, reply)) => {
                                let result = engine.act(action).and_then(|()| engine.refresh());
                                let _ = reply.send(result.map(|(snapshot, changed)| {
                                    if changed {
                                        notify(&snapshot);
                                    }
                                    snapshot
                                }));
                            }
                            Ok(Message::Image(request, reply)) => {
                                // 核心可能刚完成自动激活／重载，先同步再校验请求修订。
                                let result = engine.refresh().and_then(|(snapshot, changed)| {
                                    if changed {
                                        notify(&snapshot);
                                    }
                                    engine.image(&request.document_id, &request.revision)
                                });
                                let _ = reply.send(result);
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                        let (snapshot, changed) = engine.refresh()?;
                        if changed {
                            notify(&snapshot);
                        }
                    }
                    Ok(())
                })();
                stop_signal.store(true, Ordering::Release);
                // 丢弃邮箱会唤醒所有尚未完成的异步调用，不遗留请求。
                drop(receiver);
                let cleanup = engine.shutdown();
                exit_signal.store(true, Ordering::Release);
                on_exit(result.and(cleanup));
            })
            .map_err(|e| {
                error(
                    WorkspaceErrorCode::Internal,
                    format!("无法启动桌面工作区：{e}"),
                )
            })?;
        Ok(Self {
            sender,
            stopping,
            stopped,
            image_reading: Arc::new(AtomicBool::new(false)),
            picking: Arc::new(AtomicBool::new(false)),
        })
    }

    fn send(&self, message: Message) -> Result<(), WorkspaceError> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(error(WorkspaceErrorCode::ShuttingDown, "工作区正在退出。"));
        }
        self.sender.try_send(message).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => {
                error(WorkspaceErrorCode::Busy, "操作过于频繁，请稍后重试。")
            }
            mpsc::TrySendError::Disconnected(_) => {
                error(WorkspaceErrorCode::ShuttingDown, "工作区已停止。")
            }
        })
    }

    pub(crate) async fn action(
        &self,
        action: WorkspaceAction,
    ) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Message::Action(action, sender))?;
        receiver
            .await
            .map_err(|_| error(WorkspaceErrorCode::ShuttingDown, "工作区已停止。"))?
    }

    pub(crate) async fn image(
        &self,
        request: PreviewRequest,
    ) -> Result<PreviewLease, WorkspaceError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Message::Image(request, sender))?;
        receiver
            .await
            .map_err(|_| error(WorkspaceErrorCode::ShuttingDown, "工作区已停止。"))?
    }

    pub(crate) fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }
    pub(crate) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
}

impl Drop for WorkspaceHost {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_mailbox_rejects_overflow_and_shutdown_releases_waiters() {
        let (entered, entrance) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (exited, exit) = mpsc::channel();
        let host = WorkspaceHost::start(
            Default::default(),
            move |_| {
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(10)).unwrap();
            },
            move |result| {
                exited.send(result).unwrap();
            },
        )
        .unwrap();
        let (initial, initial_reply) = oneshot::channel();
        host.send(Message::Action(WorkspaceAction::Snapshot {}, initial))
            .unwrap();
        entrance.recv_timeout(Duration::from_secs(10)).unwrap();
        let mut waiting = Vec::new();
        for _ in 0..MAILBOX_LIMIT {
            let (sender, receiver) = oneshot::channel();
            host.send(Message::Action(WorkspaceAction::Snapshot {}, sender))
                .unwrap();
            waiting.push(receiver);
        }
        let (overflow, _) = oneshot::channel();
        assert_eq!(
            host.send(Message::Action(WorkspaceAction::Snapshot {}, overflow))
                .unwrap_err()
                .code,
            WorkspaceErrorCode::Busy
        );
        host.stop();
        release.send(()).unwrap();
        exit.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        assert!(host.stopped());
        assert!(
            tauri::async_runtime::block_on(initial_reply)
                .unwrap()
                .is_ok()
        );
        for receiver in waiting {
            assert!(tauri::async_runtime::block_on(receiver).is_err());
        }
        let error =
            tauri::async_runtime::block_on(host.action(WorkspaceAction::Snapshot {})).unwrap_err();
        assert_eq!(error.code, WorkspaceErrorCode::ShuttingDown);
    }
}
