//! 回环 HTTP、鉴权与有界连接；关闭先排空协议请求，再结束共享核心。

use super::{
    config::Instance,
    tools::{self, Handler, Shared},
};
use crate::workspace::WorkspaceHost;
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::{
    future::{Future, IntoFuture},
    io,
    net::{Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

pub(crate) struct Running {
    stopping: CancellationToken,
    stopped: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}
impl Running {
    pub(crate) fn stop(&self) {
        self.stopping.cancel();
    }
    pub(crate) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
struct Access {
    instance: Arc<Instance>,
    stopping: CancellationToken,
}

async fn authenticate(State(access): State<Access>, request: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", access.instance.token);
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .map(|h| h.as_bytes())
        .unwrap_or_default();
    let valid = supplied.len() == expected.len()
        && supplied
            .iter()
            .zip(expected.bytes())
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0;
    if !valid {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // 此开发通道仅服务本机测试客户端；不接受浏览器跨源调用。
    if request.headers().contains_key(header::ORIGIN) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if access.stopping.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let mut response = match tokio::time::timeout(Duration::from_secs(35), next.run(request)).await
    {
        Ok(response) => response,
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    response.headers_mut().insert(
        header::CONNECTION,
        header::HeaderValue::from_static("close"),
    );
    response
}

struct LimitedListener {
    listener: TcpListener,
    slots: Arc<Semaphore>,
    force: CancellationToken,
}
struct Connection {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
    force: Pin<Box<WaitForCancellationFutureOwned>>,
    idle: Pin<Box<tokio::time::Sleep>>,
}
impl Connection {
    fn check(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        if self.force.as_mut().poll(cx).is_ready() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "开发监听正在退出",
            ));
        }
        if self.idle.as_mut().poll(cx).is_ready() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "开发连接空闲超时"));
        }
        Ok(())
    }
    fn touched(&mut self) {
        self.idle
            .as_mut()
            .reset(tokio::time::Instant::now() + Duration::from_secs(45));
    }
}
impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Err(error) = self.check(cx) {
            return Poll::Ready(Err(error));
        }
        let result = Pin::new(&mut self.stream).poll_read(cx, buffer);
        if result.is_ready() {
            self.touched();
        }
        result
    }
}
impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Err(error) = self.check(cx) {
            return Poll::Ready(Err(error));
        }
        let result = Pin::new(&mut self.stream).poll_write(cx, bytes);
        if result.is_ready() {
            self.touched();
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Err(error) = self.check(cx) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
impl axum::serve::Listener for LimitedListener {
    type Io = Connection;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        // 本信号量不调用 close；关闭监听由 serve 的取消分支负责。
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .expect("连接信号量仅由监听器拥有且不关闭");
        let (stream, address) = axum::serve::Listener::accept(&mut self.listener).await;
        (
            Connection {
                stream,
                _permit: permit,
                force: Box::pin(self.force.clone().cancelled_owned()),
                idle: Box::pin(tokio::time::sleep(Duration::from_secs(45))),
            },
            address,
        )
    }
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }
}

pub(crate) async fn start(
    host: Arc<WorkspaceHost>,
    instance: Arc<Instance>,
    finished: impl FnOnce() + Send + 'static,
) -> io::Result<Running> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, instance.port)).await?;
    let address = listener.local_addr()?;
    let stopping = CancellationToken::new();
    let transport_stop = CancellationToken::new();
    let force = CancellationToken::new();
    let shared = Shared::new(host.clone(), instance.clone(), stopping.clone()).await?;
    let mut config = StreamableHttpServerConfig::default().enforce_origin_validation();
    config.legacy_session_mode = false;
    config.json_response = true;
    config.allowed_hosts = vec![address.to_string()];
    config.max_request_body_bytes = tools::REQUEST_BYTES;
    config.cancellation_token = transport_stop.clone();
    let handler = Handler(shared.clone());
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router =
        Router::new()
            .route_service("/mcp", service)
            .layer(middleware::from_fn_with_state(
                Access {
                    instance: instance.clone(),
                    stopping: stopping.clone(),
                },
                authenticate,
            ));
    let listener = LimitedListener {
        listener,
        slots: Arc::new(Semaphore::new(tools::CONNECTIONS)),
        force: force.clone(),
    };
    instance.publish(&format!("http://{address}/mcp"))?;
    eprintln!(
        "operation=dev_mcp_listen instance={} address={address}",
        instance.id
    );
    let stopped = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let done = stopped.clone();
    let failure = failed.clone();
    let stop = stopping.clone();
    tauri::async_runtime::spawn(async move {
        let serve = axum::serve(listener, router)
            .with_graceful_shutdown(stop.clone().cancelled_owned())
            .into_future();
        tokio::pin!(serve);
        let result = tokio::select! {
            result = &mut serve => result,
            _ = stop.cancelled() => {
                host.operations.testing.stop();
                tokio::select! {
                    result = &mut serve => result,
                    _ = tokio::time::sleep(Duration::from_secs(3)) => { force.cancel(); transport_stop.cancel(); serve.await }
                }
            }
        };
        stop.cancel();
        transport_stop.cancel();
        host.operations.testing.stop();
        shared.receipts.finish().await;
        let result = result.and_then(|()| instance.finish());
        if let Err(error) = result {
            failure.store(true, Ordering::Release);
            eprintln!(
                "operation=dev_mcp_shutdown instance={} error={error}",
                instance.id
            );
        }
        done.store(true, Ordering::Release);
        host.stop();
        finished();
    });
    Ok(Running {
        stopping,
        stopped,
        failed,
    })
}
