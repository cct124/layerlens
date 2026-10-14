//! 受限 W3C WebDriver 客户端。脚本均为源码内固定只读探针，输入经过真实 WebView 事件。
use super::{model::*, observation::Observations, targets};
use crate::{dev_mcp::config::Instance, workspace::error};
use base64::Engine as _;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode as Code};
use reqwest::{Client, Method, Url};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

pub(crate) struct Driver {
    client: Client,
    endpoint: Url,
    instance: Arc<Instance>,
    session: Mutex<Option<DriverInfo>>,
    operations: Arc<AsyncMutex<()>>,
    held: AtomicBool,
}
impl Driver {
    pub(crate) fn new(instance: Arc<Instance>, endpoint: Url) -> std::io::Result<Self> {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(12))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(std::io::Error::other)?;
        Ok(Self {
            client,
            endpoint,
            instance,
            session: Mutex::new(None),
            operations: Arc::new(AsyncMutex::new(())),
            held: AtomicBool::new(false),
        })
    }
    pub(crate) fn acquire(&self) -> Result<OwnedMutexGuard<()>, WorkspaceError> {
        self.operations
            .clone()
            .try_lock_owned()
            .map_err(|_| error(Code::Busy, "另一界面输入或截图尚未结束；未接受新操作。"))
    }
    pub(crate) fn observations(&self) -> &Observations {
        &self.instance.observations
    }
    fn session(&self) -> Result<String, WorkspaceError> {
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|v| v.driver_session_id.clone())
            .ok_or_else(|| {
                error(
                    Code::NotFound,
                    "请先通过 dev_ui_attach 绑定本实例的 WebDriver 会话。",
                )
            })
    }
    async fn command(
        &self,
        method: Method,
        route: &str,
        body: Option<Value>,
    ) -> Result<Value, WorkspaceError> {
        let url = self
            .endpoint
            .join(route)
            .map_err(|e| error(Code::InvalidInput, e.to_string()))?;
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|e| error(Code::Internal, format!("WebDriver 请求失败：{e}")))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| error(Code::Internal, format!("WebDriver 响应中断：{e}")))?
        {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err(error(Code::ResourceLimit, "WebDriver 响应超过 2 MiB。"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes)
            .map_err(|e| error(Code::Internal, format!("WebDriver 响应无效：{e}")))?;
        if !status.is_success() || body["value"].get("error").is_some() {
            let reason = body["value"]["message"].as_str().unwrap_or("驱动拒绝请求");
            return Err(error(
                Code::Internal,
                format!(
                    "WebDriver {status}：{}",
                    reason.chars().take(1024).collect::<String>()
                ),
            ));
        }
        body.get("value")
            .cloned()
            .ok_or_else(|| error(Code::Internal, "WebDriver 响应缺少 value。"))
    }
    async fn session_command(
        &self,
        method: Method,
        route: &str,
        body: Option<Value>,
    ) -> Result<Value, WorkspaceError> {
        let session = self.session()?;
        self.command(method, &format!("session/{session}/{route}"), body)
            .await
    }
    async fn probe_session(&self, session: &str) -> Result<UiObservation, WorkspaceError> {
        let value = self
            .command(
                Method::POST,
                &format!("session/{session}/execute/sync"),
                Some(json!({
                    "script":"const bridge=window.__LAYERLENS_DEV_VIEW__; if(bridge?.error) throw new Error(bridge.error); return bridge?.read() ?? null;","args":[]
                })),
            )
            .await?;
        let observation: UiObservation = serde_json::from_value(value)
            .map_err(|_| error(Code::NotFound, "目标 WebView 尚无开发观测桥。"))?;
        if observation.instance_id != self.instance.id {
            return Err(error(
                Code::ForeignSession,
                "驱动会话不属于当前 LayerLens 实例，未执行界面操作。",
            ));
        }
        if self.observations().read()?.frontend_id != observation.frontend_id {
            return Err(error(Code::StaleRevision, "前端正在更换代次，请重新观测。"));
        }
        Ok(observation)
    }
    pub(crate) async fn attach(
        &self,
        request: AttachRequest,
    ) -> Result<DriverInfo, WorkspaceError> {
        let session = request.driver_session_id;
        if session.is_empty()
            || session.len() > 128
            || !session
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(error(Code::InvalidInput, "WebDriver 会话标识无效。"));
        }
        if self
            .session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|v| v.driver_session_id != session)
        {
            return Err(error(Code::RequestConflict, "实例已经绑定另一驱动会话。"));
        }
        self.probe_session(&session).await?;
        let status = self.command(Method::GET, "status", None).await?;
        // User-Agent 会缩减为主版本.0.0.0；由本机 WebView 运行时查询完整版本。
        let browser_version = tauri::webview_version()
            .map_err(|e| error(Code::Internal, format!("读取 WebView 版本失败：{e}")))?;
        let info = DriverInfo {
            driver_session_id: session,
            browser_version,
            driver_version: status["build"]["version"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 256)
                .ok_or_else(|| error(Code::Internal, "驱动未提供有效版本信息。"))?
                .to_owned(),
        };
        *self.session.lock().unwrap_or_else(|p| p.into_inner()) = Some(info.clone());
        Ok(info)
    }
    async fn element(&self, target: &str) -> Result<String, WorkspaceError> {
        let found = self
            .session_command(
                Method::POST,
                "element",
                Some(json!({"using":"css selector","value":targets::selector(target)?})),
            )
            .await?;
        found["element-6066-11e4-a52e-4f735466cecf"]
            .as_str()
            .map(str::to_owned)
            .filter(|s| {
                !s.is_empty()
                    && *s != "."
                    && *s != ".."
                    && s.len() < 256
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            })
            .ok_or_else(|| error(Code::Internal, "驱动返回无效元素标识。"))
    }
    pub(crate) async fn input(&self, request: InputRequest) -> Result<InputResult, WorkspaceError> {
        let before = self.probe_session(&self.session()?).await?;
        if request.frontend_id != before.frontend_id
            || request
                .expected_view_revision
                .as_ref()
                .is_some_and(|r| r != &before.view_revision)
        {
            return Err(error(
                Code::StaleRevision,
                "界面代次或版本已变化，未执行输入。",
            ));
        }
        if self.held.load(Ordering::Acquire)
            && !matches!(
                request.action,
                UiAction::Key { .. } | UiAction::ReleaseInputs {}
            )
        {
            return Err(error(
                Code::Busy,
                "已有保留或结果不确定的输入，请先按 Esc 并 releaseInputs。",
            ));
        }
        match request.action {
            UiAction::Click { target } => {
                let element = self.element(&target).await?;
                self.session_command(
                    Method::POST,
                    &format!("element/{element}/click"),
                    Some(json!({})),
                )
                .await?;
            }
            UiAction::FillTaskName { text } => {
                if text.len() > 256 || text.contains(['\r', '\n', '\0']) {
                    return Err(error(
                        Code::InvalidInput,
                        "任务名称须为单行且不超过 256 UTF-8 字节。",
                    ));
                }
                let element = self.element("task:name").await?;
                self.session_command(
                    Method::POST,
                    &format!("element/{element}/click"),
                    Some(json!({})),
                )
                .await?;
                // WebDriver Send Keys 产生键盘输入；不调用 DOM value setter 或合成 input 事件。
                self.session_command(
                    Method::POST,
                    &format!("element/{element}/value"),
                    Some(json!({"text":format!("\u{e009}a\u{e000}{text}")})),
                )
                .await?;
            }
            UiAction::Drag {
                start_x,
                start_y,
                end_x,
                end_y,
                hold,
            } => {
                let canvas = before
                    .canvas
                    .as_ref()
                    .ok_or_else(|| error(Code::InvalidInput, "当前没有可拖动的画布。"))?;
                let inside = |x: i32, y: i32| {
                    f64::from(x) >= canvas.viewport.x
                        && f64::from(x) < canvas.viewport.x + canvas.viewport.width
                        && f64::from(y) >= canvas.viewport.y
                        && f64::from(y) < canvas.viewport.y + canvas.viewport.height
                };
                if !canvas.image_decoded
                    || before.cleanup.opened
                    || !inside(start_x, start_y)
                    || !inside(end_x, end_y)
                {
                    return Err(error(
                        Code::InvalidInput,
                        "拖动须在已显示的画布内，且不能穿过模态窗口。",
                    ));
                }
                let mut actions = vec![
                    json!({"type":"pointerMove","origin":"viewport","x":start_x,"y":start_y,"duration":0}),
                    json!({"type":"pointerDown","button":0}),
                    json!({"type":"pointerMove","origin":"viewport","x":end_x,"y":end_y,"duration":100}),
                ];
                if !hold {
                    actions.push(json!({"type":"pointerUp","button":0}));
                }
                // 此 WebView2／EdgeDriver 的分段 Actions 会丢失捕获，完整手势必须一次派发。
                // 网络失败可能发生在按下之后，先保留输入状态，要求显式恢复。
                self.held.store(true, Ordering::Release);
                self.session_command(Method::POST,"actions",Some(json!({"actions":[{"type":"pointer","id":"layerlens-pointer","parameters":{"pointerType":"mouse"},"actions":actions}]}))).await?;
                self.held.store(hold, Ordering::Release);
            }
            UiAction::Key { key, phase } => {
                let value = match key {
                    UiKey::Escape => "\u{e00c}",
                    UiKey::Alt => "\u{e00a}",
                    UiKey::Control => "\u{e009}",
                    UiKey::Shift => "\u{e008}",
                    UiKey::Tab => "\u{e004}",
                    UiKey::Enter => "\u{e007}",
                    UiKey::ArrowLeft => "\u{e012}",
                    UiKey::ArrowUp => "\u{e013}",
                    UiKey::ArrowRight => "\u{e014}",
                    UiKey::ArrowDown => "\u{e015}",
                    UiKey::V => "v",
                    UiKey::M => "m",
                };
                let mut actions = Vec::new();
                if !matches!(phase, KeyPhase::Up) {
                    actions.push(json!({"type":"keyDown","value":value}));
                }
                if !matches!(phase, KeyPhase::Down) {
                    actions.push(json!({"type":"keyUp","value":value}));
                }
                self.session_command(Method::POST,"actions",Some(json!({"actions":[{"type":"key","id":"layerlens-keyboard","actions":actions}]}))).await?;
            }
            UiAction::ReleaseInputs {} => {
                self.session_command(Method::DELETE, "actions", None)
                    .await?;
                self.held.store(false, Ordering::Release);
            }
        }
        Ok(InputResult {
            before,
            input_completed: true,
        })
    }
    pub(crate) async fn screenshot(
        &self,
        request: ScreenshotRequest,
    ) -> Result<(ScreenshotResult, String), WorkspaceError> {
        let session = self.session()?;
        let before = self.probe_session(&session).await?;
        if before.frontend_id != request.frontend_id {
            return Err(error(Code::StaleRevision, "前端代次已变化，未截图。"));
        }
        let image = self
            .session_command(Method::GET, "screenshot", None)
            .await?;
        let image = image
            .as_str()
            .ok_or_else(|| error(Code::Internal, "截图不是 base64 字符串。"))?;
        if image.len() > 700 * 1024 {
            return Err(error(Code::ResourceLimit, "截图超过传输预算。"));
        }
        let png = base64::engine::general_purpose::STANDARD
            .decode(image)
            .map_err(|_| error(Code::Internal, "截图 base64 无效。"))?;
        if png.len() > super::super::tools::PNG_BYTES {
            return Err(error(Code::ResourceLimit, "截图 PNG 超过 512 KiB。"));
        }
        if png.len() < 24 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(error(Code::Internal, "驱动返回的截图不是 PNG。"));
        }
        let width = u32::from_be_bytes(
            png[16..20]
                .try_into()
                .map_err(|_| error(Code::Internal, "PNG 宽度无效。"))?,
        );
        let height = u32::from_be_bytes(
            png[20..24]
                .try_into()
                .map_err(|_| error(Code::Internal, "PNG 高度无效。"))?,
        );
        let after = self.probe_session(&session).await?;
        let unchanged =
            before.frontend_id == after.frontend_id && before.view_revision == after.view_revision;
        Ok((
            ScreenshotResult {
                before,
                after,
                unchanged,
                width,
                height,
                source: "webdriverWebview".into(),
            },
            image.to_owned(),
        ))
    }
}
