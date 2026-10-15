//! 开发 MCP 工具路由；协议只包装真实业务结果，不代替核心授权或伪造前端同步。

use super::control::{ControlRequest, ControlResult};
use super::fixtures::{self, PrepareTasks, PreparedTasks};
use super::model::Request;
use super::ui::model::InputRequest;
use super::ui::{Driver, model::*};
use super::windows::{WindowAction, WindowActionResult, WindowState, WindowWait, WindowWaitResult};
use super::{
    config::Instance,
    model::*,
    operations,
    receipts::{self, Receipts},
};
use crate::workspace::{WorkspaceHost, check_size, error};
use base64::Engine as _;
use layerlens_core::{cleanup_contract::*, selection_contract::*, workspace_contract::*};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, model::*, service::RequestContext};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{borrow::Cow, io, sync::Arc};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub(super) const REQUEST_BYTES: usize = 128 * 1024;
pub(super) const REPLY_BYTES: usize = 2 * 1024 * 1024;
pub(super) const PNG_BYTES: usize = 512 * 1024;
pub(super) const CONNECTIONS: usize = 16;
pub(super) const CALLS: usize = 4;
pub(super) const WAITS: usize = 4;

pub(super) struct Shared {
    pub host: Arc<WorkspaceHost>,
    pub instance: Arc<Instance>,
    pub session: String,
    pub stopping: CancellationToken,
    pub receipts: Receipts,
    calls: Semaphore,
    waits: Semaphore,
    controls: Semaphore,
    tools: Vec<Tool>,
    ui: Option<Arc<Driver>>,
}

#[derive(Clone)]
pub(super) struct Handler(pub Arc<Shared>);

fn definition<I: JsonSchema + 'static, O: JsonSchema + 'static>(
    name: &'static str,
    description: &'static str,
    read_only: bool,
) -> io::Result<Tool> {
    let input = rmcp::handler::server::tool::schema_for_input::<I>().map_err(io::Error::other)?;
    Ok(Tool::new(name, description, input)
        .with_output_schema::<Reply<O>>()
        .with_annotations(
            ToolAnnotations::new()
                .read_only(read_only)
                .open_world(false),
        ))
}

pub(super) fn value<T: Serialize>(value: T) -> Result<Value, WorkspaceError> {
    check_size(&value, receipts::RESULT_BYTES)?;
    serde_json::to_value(value).map_err(|e| {
        error(
            WorkspaceErrorCode::Internal,
            format!("序列化开发工具结果失败：{e}"),
        )
    })
}

impl Shared {
    pub(super) async fn new(
        host: Arc<WorkspaceHost>,
        instance: Arc<Instance>,
        stopping: CancellationToken,
    ) -> io::Result<Arc<Self>> {
        let snapshot = host
            .action(WorkspaceAction::Snapshot {})
            .await
            .map_err(|e| io::Error::other(e.message))?;
        let mut tools = vec![
            definition::<Request<PrepareTasks>, PreparedTasks>(
                "dev_prepare_tasks",
                "在固定快照上准备 1–128 个任务；逐项使用真实服务，完整／部分结果由同一请求回放，不提高预算。",
                false,
            )?,
            definition::<Request<ControlRequest>, ControlResult>(
                "dev_test_control",
                "开发实例计划时钟与命名检查点；arm/advancePlanClock 需请求 ID，按票据释放幂等，等待不占业务执行名额。",
                false,
            )?,
            definition::<Empty, InstanceInfo>(
                "dev_instance",
                "查询当前开发测试实例、会话及已实现能力；先读取此工具再操作。",
                true,
            )?,
            definition::<Request<WorkspaceRequest>, WorkspaceResult>(
                "dev_workspace",
                "查询或操作测试工作区；变更须带 clientRequestId，打开仅允许受控样本。accepted 不代表解析成功。",
                false,
            )?,
            definition::<Request<SelectionRequest>, SelectionReply>(
                "dev_selection",
                "复用核心图层／矩形选择、快照内容分页及固定任务；变更须带 clientRequestId。",
                false,
            )?,
            definition::<Request<CleanupRequest>, CleanupReply>(
                "dev_cleanup",
                "预检／确认／取消清理计划，保留全部影响和原请求回执；须带 clientRequestId。",
                false,
            )?,
            definition::<Request<LayerRequest>, LayerResponse>(
                "dev_layers",
                "按明确文档与修订读取有界图层分页或文字详情。",
                true,
            )?,
            definition::<Request<PreviewRequest>, PreviewResult>(
                "dev_preview",
                "读取当前固定修订的已就绪原尺寸合成 PNG；传输超过限额时明确拒绝，不生成缩略假图。",
                true,
            )?,
            definition::<Request<WaitRequest>, WaitResult>(
                "dev_wait",
                "按事件有界等待核心状态，超时返回最后观测；不代表前端或物理显示已同步。",
                true,
            )?,
            definition::<Request<Empty>, ShutdownResult>(
                "dev_shutdown",
                "结束匹配身份的开发测试实例；等待已接受请求完成并走正常核心退出。",
                false,
            )?,
        ];
        let ui = instance
            .web_driver_url
            .clone()
            .map(|endpoint| Driver::new(instance.clone(), endpoint).map(Arc::new))
            .transpose()?;
        if ui.is_some() {
            tools.extend([
                definition::<Request<AttachRequest>,DriverInfo>("dev_ui_attach","绑定配置的回环 WebDriver 会话；核验前端实例身份，需 clientRequestId。",false)?,
                definition::<Request<Empty>,UiObservation>("dev_ui_state","读取真实前端最新投影：已应用核心序号、视图代次、草稿、图像解码及弹窗。",true)?,
                definition::<Request<InputRequest>,InputResult>("dev_ui_action","通过 WebDriver 进行语义点击、键盘或 CSS 像素指针输入；需前端代次和 clientRequestId。完成输入不代表核心已同步。",false)?,
                definition::<Request<UiWaitRequest>,UiWaitResult>("dev_ui_wait","按前端真实报告有界等待，不占用驱动执行名额；导航／代次变化终止旧等待。",true)?,
                definition::<Request<ScreenshotRequest>,ScreenshotResult>("dev_ui_screenshot","采集真实 WebView PNG，并附捕获前后前端版本；unchanged 仅比较观测，不代表物理显示验收。",true)?,
            ]);
        }
        if instance.windows.get().is_some() {
            tools.extend([
                definition::<Request<Empty>,WindowState>("dev_window_state", "读取本测试实例 main／辅助窗口的真实焦点、尺寸、DPI 及事件计数。", true)?,
                definition::<Request<WindowAction>,WindowActionResult>("dev_window_action", "只操作测试实例自有窗口：焦点转移、最小化、最大化、还原、预设尺寸、正常关闭；需请求 ID。dispatched 后须等待实际状态。", false)?,
                definition::<Request<WindowWait>,WindowWaitResult>("dev_window_wait", "有界等待原生窗口状态及事件序号；超时保留末态，不占输入名额。", true)?,
            ]);
        }
        host.operations.testing.enable();
        Ok(Arc::new(Self {
            host,
            instance,
            session: snapshot.selection.session_id,
            stopping,
            receipts: Receipts::default(),
            calls: Semaphore::new(CALLS),
            waits: Semaphore::new(WAITS),
            controls: Semaphore::new(1),
            tools,
            ui,
        }))
    }

    fn parse<T: DeserializeOwned>(&self, arguments: Value) -> Result<Request<T>, McpError> {
        let request: Request<T> = serde_json::from_value(arguments)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        if request.instance_id != self.instance.id || request.session_id != self.session {
            return Err(McpError::invalid_params(
                "实例或会话不匹配；请重新读取 dev_instance。",
                None,
            ));
        }
        Ok(request)
    }

    async fn reply(
        &self,
        result: Result<Value, WorkspaceError>,
        fixed: Option<SelectionSummaryDto>,
        image: Option<String>,
    ) -> Result<CallToolResponse, McpError> {
        let failed = result.is_err();
        let data = match result {
            Ok(value) => Outcome::Ok { value },
            Err(error) => Outcome::Error { error },
        };
        let user_selection = match self.host.operations.context(fixed).await {
            Ok(context) => Observation::Available(Box::new(context)),
            Err(unavailable) => Observation::Unavailable { unavailable },
        };
        let body = serde_json::to_value(Reply {
            data,
            user_selection,
        })
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let mut reply = if failed {
            CallToolResult::structured_error(body)
        } else {
            CallToolResult::structured(body)
        };
        if let Some(png) = image {
            reply.content.push(ContentBlock::image(png, "image/png"));
        }
        check_size(&reply, REPLY_BYTES).map_err(|e| McpError::internal_error(e.message, None))?;
        Ok(reply.into())
    }

    fn mutation<T: Serialize>(
        &self,
        tool: &str,
        request: &Request<T>,
        operation: impl std::future::Future<Output = Result<Value, WorkspaceError>> + Send + 'static,
    ) -> Result<impl std::future::Future<Output = Result<Value, WorkspaceError>>, WorkspaceError>
    {
        let id = request.client_request_id.as_deref().ok_or_else(|| {
            error(
                WorkspaceErrorCode::InvalidInput,
                "此变更必须提供 clientRequestId。",
            )
        })?;
        let arguments = serde_json::to_vec(&(tool, request))
            .map_err(|e| error(WorkspaceErrorCode::InvalidInput, e.to_string()))?;
        let receiver = self.receipts.start(id, arguments, operation)?;
        Ok(Receipts::receive(receiver))
    }
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("layerlens-dev-mcp", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_instructions("仅用于受控开发测试实例。先读取 dev_instance；所有操作核对实例和会话。变更重试复用原 clientRequestId 与原参数。工具结果不是界面验收结论。")
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![ProtocolVersion::V_2025_11_25])
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.0.tools.iter().find(|tool| tool.name == name).cloned()
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        if request.is_some_and(|p| p.cursor.is_some()) {
            return Err(McpError::invalid_params("工具清单没有续页游标。", None));
        }
        Ok(ListToolsResult {
            tools: self.0.tools.clone(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let shared = &self.0;
        if shared.stopping.is_cancelled() {
            return Err(McpError::internal_error("测试实例正在退出。", None));
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        check_size(&arguments, 64 * 1024).map_err(|e| McpError::invalid_params(e.message, None))?;
        let permits = if matches!(
            request.name.as_ref(),
            "dev_wait" | "dev_ui_wait" | "dev_window_wait"
        ) || (request.name == "dev_test_control"
            && arguments["request"]["kind"] == "wait")
        {
            &shared.waits
        } else if request.name == "dev_test_control" {
            &shared.controls
        } else {
            &shared.calls
        };
        let _permit = permits
            .try_acquire()
            .map_err(|_| McpError::internal_error("开发请求名额已满；未接受新操作。", None))?;
        let work = async {
            match request.name.as_ref() {
                "dev_window_state" | "dev_window_action" | "dev_window_wait" => {
                    let windows = shared.instance.windows.get().cloned().ok_or_else(|| {
                        McpError::invalid_params("此实例未启用 nativeWindows。", None)
                    })?;
                    let result = match request.name.as_ref() {
                        "dev_window_state" => {
                            let _: Request<Empty> = shared.parse(arguments)?;
                            windows.read().await.and_then(value)
                        }
                        "dev_window_wait" => {
                            let args: Request<WindowWait> = shared.parse(arguments)?;
                            windows
                                .wait(args.request, &shared.stopping)
                                .await
                                .and_then(value)
                        }
                        _ => {
                            let args: Request<WindowAction> = shared.parse(arguments)?;
                            let input = args.request.clone();
                            let ui = shared.ui.as_ref().ok_or_else(|| {
                                McpError::invalid_params("未配置 WebDriver。", None)
                            })?;
                            match ui.acquire() {
                                Ok(guard) => {
                                    match shared.mutation("window_action", &args, async move {
                                        let _guard = guard;
                                        windows.action(input).await.and_then(value)
                                    }) {
                                        Ok(wait) => wait.await,
                                        Err(error) => Err(error),
                                    }
                                }
                                Err(error) => Err(error),
                            }
                        }
                    };
                    shared.reply(result, None, None).await
                }
                "dev_prepare_tasks" => {
                    let args: Request<PrepareTasks> = shared.parse(arguments)?;
                    let host = shared.host.clone();
                    let session = shared.session.clone();
                    let input = args.request.clone();
                    let result = match shared.mutation("prepare_tasks", &args, async move {
                        fixtures::prepare(&host, session, input)
                            .await
                            .and_then(value)
                    }) {
                        Ok(wait) => wait.await,
                        Err(error) => Err(error),
                    };
                    shared.reply(result, None, None).await
                }
                "dev_test_control" => {
                    let args: Request<ControlRequest> = shared.parse(arguments)?;
                    let controls = shared.host.operations.testing.clone();
                    let input = args.request.clone();
                    let result = match input {
                        ControlRequest::Release { id } => controls
                            .release(id)
                            .map(|gate| ControlResult::Gate { gate })
                            .and_then(value),
                        ControlRequest::State { id } => controls
                            .state(id)
                            .map(|gate| ControlResult::Gate { gate })
                            .and_then(value),
                        ControlRequest::Wait {
                            id,
                            phase,
                            timeout_ms,
                        } => controls
                            .wait(id, phase, timeout_ms)
                            .await
                            .map(|(satisfied, gate)| ControlResult::Wait { satisfied, gate })
                            .and_then(value),
                        _ => match shared.mutation("test_control", &args, async move {
                            match input {
                                ControlRequest::AdvancePlanClock { seconds } => controls
                                    .advance(seconds)
                                    .map(|offset_seconds| ControlResult::Clock { offset_seconds }),
                                ControlRequest::Arm {
                                    checkpoint,
                                    mode,
                                    timeout_ms,
                                } => controls
                                    .arm(checkpoint, mode, timeout_ms)
                                    .map(|gate| ControlResult::Gate { gate }),
                                _ => unreachable!("只有时钟／arm 使用变更回执"),
                            }
                            .and_then(value)
                        }) {
                            Ok(wait) => wait.await,
                            Err(error) => Err(error),
                        },
                    };
                    shared.reply(result, None, None).await
                }
                "dev_ui_attach" | "dev_ui_state" | "dev_ui_action" | "dev_ui_wait"
                | "dev_ui_screenshot" => {
                    let ui = shared.ui.clone().ok_or_else(|| {
                        McpError::invalid_params("此实例未配置 WebDriver。", None)
                    })?;
                    match request.name.as_ref() {
                        "dev_ui_state" => {
                            let _: Request<Empty> = shared.parse(arguments)?;
                            shared
                                .reply(ui.observations().read().and_then(value), None, None)
                                .await
                        }
                        "dev_ui_wait" => {
                            let args: Request<UiWaitRequest> = shared.parse(arguments)?;
                            shared
                                .reply(
                                    ui.observations()
                                        .wait(args.request, &shared.stopping)
                                        .await
                                        .and_then(value),
                                    None,
                                    None,
                                )
                                .await
                        }
                        "dev_ui_screenshot" => {
                            let args: Request<ScreenshotRequest> = shared.parse(arguments)?;
                            let result = async {
                                let _guard = ui.acquire()?;
                                ui.screenshot(args.request).await
                            }
                            .await;
                            match result {
                                Ok((data, png)) => shared.reply(value(data), None, Some(png)).await,
                                Err(error) => shared.reply(Err(error), None, None).await,
                            }
                        }
                        "dev_ui_attach" => {
                            let args: Request<AttachRequest> = shared.parse(arguments)?;
                            let input = args.request.clone();
                            let result = match ui.acquire() {
                                Ok(guard) => match shared.mutation("ui_attach", &args, async move {
                                    let _guard = guard;
                                    ui.attach(input).await.and_then(value)
                                }) {
                                    Ok(wait) => wait.await,
                                    Err(error) => Err(error),
                                },
                                Err(error) => Err(error),
                            };
                            shared.reply(result, None, None).await
                        }
                        _ => {
                            let args: Request<InputRequest> = shared.parse(arguments)?;
                            let input = args.request.clone();
                            let result = match ui.acquire() {
                                Ok(guard) => match shared.mutation("ui_action", &args, async move {
                                    let _guard = guard;
                                    ui.input(input).await.and_then(value)
                                }) {
                                    Ok(wait) => wait.await,
                                    Err(error) => Err(error),
                                },
                                Err(error) => Err(error),
                            };
                            shared.reply(result, None, None).await
                        }
                    }
                }
                "dev_instance" => {
                    let _: Empty = serde_json::from_value(arguments)
                        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                    let info = InstanceInfo {
                        instance_id: shared.instance.id.clone(),
                        session_id: shared.session.clone(),
                        app_version: env!("CARGO_PKG_VERSION").into(),
                        protocol_version: layerlens_core::IPC_PROTOCOL_VERSION,
                        tools: shared.tools.iter().map(|t| t.name.to_string()).collect(),
                        active_waits: WAITS - shared.waits.available_permits(),
                        limits: Limits {
                            request_bytes: REQUEST_BYTES,
                            reply_bytes: REPLY_BYTES,
                            preview_png_bytes: PNG_BYTES,
                            connections: CONNECTIONS,
                            concurrent_operations: CALLS,
                            concurrent_waits: WAITS,
                            mutation_records: receipts::RECORD_LIMIT,
                            mutation_bytes: receipts::BYTE_LIMIT,
                            max_wait_ms: 30_000,
                        },
                    };
                    shared.reply(value(info), None, None).await
                }
                "dev_workspace" => {
                    let args: Request<WorkspaceRequest> = shared.parse(arguments)?;
                    let host = shared.host.clone();
                    let instance = shared.instance.clone();
                    if matches!(args.request.action, WorkspaceAction::Snapshot {}) {
                        let result = operations::workspace(host, instance, args.request).await;
                        let fixed = match &result {
                            Ok(WorkspaceResult::Snapshot { snapshot }) => {
                                Some(snapshot.selection.clone())
                            }
                            _ => None,
                        };
                        shared.reply(result.and_then(value), fixed, None).await
                    } else {
                        let request = args.request.clone();
                        let result = match shared.mutation("workspace", &args, async move {
                            operations::workspace(host, instance, request)
                                .await
                                .and_then(value)
                        }) {
                            Ok(wait) => wait.await,
                            Err(error) => Err(error),
                        };
                        shared.reply(result, None, None).await
                    }
                }
                "dev_selection" => {
                    let args: Request<SelectionRequest> = shared.parse(arguments)?;
                    if args.request.session_id != shared.session {
                        return Err(McpError::invalid_params("选择请求会话不匹配。", None));
                    }
                    let read_only = matches!(
                        args.request.operation,
                        SelectionOperation::Content { .. } | SelectionOperation::Tasks { .. }
                    );
                    let result = if read_only {
                        shared
                            .host
                            .operations
                            .request(args.request)
                            .await
                            .and_then(value)
                    } else {
                        let host = shared.host.clone();
                        let request = args.request.clone();
                        match shared.mutation("selection", &args, async move {
                            host.operations.request(request).await.and_then(value)
                        }) {
                            Ok(wait) => wait.await,
                            Err(error) => Err(error),
                        }
                    };
                    shared.reply(result, None, None).await
                }
                "dev_cleanup" => {
                    let args: Request<CleanupRequest> = shared.parse(arguments)?;
                    if args.request.session_id != shared.session {
                        return Err(McpError::invalid_params("清理请求会话不匹配。", None));
                    }
                    let host = shared.host.clone();
                    let request = args.request.clone();
                    let result = match shared.mutation("cleanup", &args, async move {
                        host.operations.cleanup(request).await.and_then(value)
                    }) {
                        Ok(wait) => wait.await,
                        Err(error) => Err(error),
                    };
                    shared.reply(result, None, None).await
                }
                "dev_layers" => {
                    let args: Request<LayerRequest> = shared.parse(arguments)?;
                    shared
                        .reply(
                            shared.host.read_layers(args.request).await.and_then(value),
                            None,
                            None,
                        )
                        .await
                }
                "dev_preview" => {
                    let args: Request<PreviewRequest> = shared.parse(arguments)?;
                    let result = shared
                        .host
                        .read_image(args.request, |image| {
                            if image.png().len() > PNG_BYTES {
                                return Err(error(
                                    WorkspaceErrorCode::ResourceLimit,
                                    "PNG 超过开发协议的 512 KiB 传输上限；原预览仍可在桌面查看。",
                                ));
                            }
                            let data = PreviewResult {
                                document_id: image.document_id().get().to_string(),
                                revision: image.revision_id().get().to_string(),
                                width: image.width(),
                                height: image.height(),
                                mime_type: "image/png".into(),
                                source: "savedComposite".into(),
                                capability: image.capability().clone(),
                            };
                            Ok((
                                value(data)?,
                                base64::engine::general_purpose::STANDARD.encode(image.png()),
                            ))
                        })
                        .await;
                    match result {
                        Ok((data, image)) => shared.reply(Ok(data), None, Some(image)).await,
                        Err(error) => shared.reply(Err(error), None, None).await,
                    }
                }
                "dev_wait" => {
                    let args: Request<WaitRequest> = shared.parse(arguments)?;
                    let result = operations::wait(
                        &shared.host,
                        &shared.instance.observations,
                        args.request,
                        &shared.stopping,
                    )
                    .await;
                    let fixed = result.as_ref().ok().map(|r| r.last_state.selection.clone());
                    shared.reply(result.and_then(value), fixed, None).await
                }
                "dev_shutdown" => {
                    let _: Request<Empty> = shared.parse(arguments)?;
                    let reply = shared
                        .reply(
                            value(ShutdownResult {
                                instance_id: shared.instance.id.clone(),
                                accepted: true,
                            }),
                            None,
                            None,
                        )
                        .await?;
                    shared.stopping.cancel();
                    Ok(reply)
                }
                _ => Err(McpError::invalid_params("未知或尚未实现的开发工具。", None)),
            }
        };
        // 取消 RPC 只结束等待；已由回执注册表接受的变更继续完成并可用原 ID 恢复。
        tokio::select! {
            _ = context.ct.cancelled() => Err(McpError::internal_error("请求等待已取消；已接受变更可用原 ID 恢复。", None)),
            result = work => result,
        }
    }
}
