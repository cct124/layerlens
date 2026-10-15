//! Tauri 桌面入口，仅装配窗口、权限和共享核心的命令适配。

mod commands;
#[cfg(feature = "dev-mcp")]
mod dev_mcp;
mod workspace;

use std::sync::Arc;
use tauri::{Emitter, Manager};

fn configure<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    let builder = builder.plugin(tauri_plugin_dialog::init());
    #[cfg(feature = "dev-mcp")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::get_app_info,
        commands::workspace_action,
        commands::read_preview,
        commands::read_layers,
        commands::selection_request,
        commands::cleanup_request,
        commands::choose_psd_files,
        dev_mcp::ui::dev_view_report
    ]);
    #[cfg(not(feature = "dev-mcp"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::get_app_info,
        commands::workspace_action,
        commands::read_preview,
        commands::read_layers,
        commands::selection_request,
        commands::cleanup_request,
        commands::choose_psd_files
    ]);
    builder
}

/// 启动桌面事件循环，应用业务能力由共享核心提供。
///
/// # Errors
///
/// 窗口或运行时初始化失败时返回 Tauri 错误，由入口统一处理。
pub fn run() -> tauri::Result<()> {
    #[cfg(not(feature = "dev-mcp"))]
    if std::env::args_os().any(|arg| arg.to_string_lossy().starts_with("--dev-mcp")) {
        return Err(std::io::Error::other("当前构建未启用 dev-mcp 特性。").into());
    }
    let context = tauri::generate_context!();
    #[cfg(feature = "dev-mcp")]
    let mut context = context;
    #[cfg(feature = "dev-mcp")]
    let development = dev_mcp::Instance::from_args()?.map(Arc::new);
    #[cfg(feature = "dev-mcp")]
    if development.is_some() {
        for window in &mut context.config_mut().app.windows {
            window.create = false;
        }
    }
    let app = configure(tauri::Builder::default())
        .setup(move |app| {
            let events = app.handle().clone();
            #[cfg(feature = "dev-mcp")]
            let testing_notifications = development.is_some();
            #[cfg(feature = "dev-mcp")]
            let (notifications, mut notification_receiver) = tokio::sync::watch::channel(None);
            #[cfg(feature = "dev-mcp")]
            let delayed_events = events.clone();
            let exit = app.handle().clone();
            #[cfg(feature = "dev-mcp")]
            let (core_done, core_stopped) = tokio::sync::oneshot::channel();
            let host = workspace::WorkspaceHost::start(
                Default::default(),
                move |snapshot| {
                    #[cfg(feature = "dev-mcp")]
                    if testing_notifications {
                        // 暂停交付时只保留最新待发送投影；核心工作线程和观测不被阻塞。
                        notifications.send_replace(Some(snapshot.clone()));
                        return;
                    }
                    if let Err(error) = events.emit_to("main", workspace::EVENT, snapshot) {
                        eprintln!(
                            "operation=workspace_notify sequence={} error={error}",
                            snapshot.sequence
                        );
                    }
                },
                move |result| {
                    #[cfg(feature = "dev-mcp")]
                    let _ = core_done.send(result.is_err());
                    if let Err(error) = &result {
                        eprintln!("operation=workspace_shutdown error={}", error.message);
                    }
                    let code = i32::from(result.is_err());
                    #[cfg(feature = "dev-mcp")]
                    let code = code.max(i32::from(
                        exit.try_state::<dev_mcp::Running>()
                            .is_some_and(|server| server.failed()),
                    ));
                    exit.exit(code);
                },
            )
            .map_err(|error| std::io::Error::other(error.message))?;
            let host = Arc::new(host);
            #[cfg(feature = "dev-mcp")]
            if testing_notifications {
                let controls = host.operations.testing.clone();
                tauri::async_runtime::spawn(async move {
                    while notification_receiver.changed().await.is_ok() {
                        let snapshot = notification_receiver.borrow_and_update().clone();
                        if let Some(snapshot) = snapshot {
                            if let Err(error) = controls.hit(workspace::testing::Checkpoint::DesktopNotification).await {
                                eprintln!("operation=dev_notification_delivery code={:?}", error.code);
                                if error.code == layerlens_core::workspace_contract::WorkspaceErrorCode::ShuttingDown { break; }
                                continue;
                            }
                            if let Err(error) = delayed_events.emit_to("main", workspace::EVENT, &snapshot) {
                                eprintln!("operation=workspace_notify sequence={} error={error}", snapshot.sequence);
                            }
                        }
                    }
                });
            }
            app.manage(host.clone());
            #[cfg(feature = "dev-mcp")]
            if let Some(instance) = development {
                app.manage(instance.clone());
                let window = app
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|window| window.label == "main")
                    .ok_or_else(|| std::io::Error::other("开发实例缺少 main 窗口配置"))?;
                let mut builder = tauri::WebviewWindowBuilder::from_config(app, window)?
                    .data_directory(instance.directory.join("webview"))
                    .visible(instance.visible)
                    .title("LayerLens · 开发测试实例");
                {
                    // 业务测试也必须能等待真实前端读完预览；驱动输入工具仍由 WebDriver 配置控制。
                    app.add_capability(include_str!("dev_mcp/observation-capability.json"))?;
                    let boot = serde_json::json!({"instanceId":instance.instance_id()});
                    let observation = instance.observations.clone();
                    builder = builder.initialization_script(format!("Object.defineProperty(window, '__LAYERLENS_DEV_BOOT__', {{value:Object.freeze({boot}), configurable:false}});"))
                        .on_page_load(move |_, payload| {
                            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) { observation.invalidate(); }
                        });
                }
                builder.build()?;
                let exit = app.handle().clone();
                let running =
                    tauri::async_runtime::block_on(dev_mcp::start(host, instance, move || {
                        // 核心可能先于监听器停止，也可能仍在释放资源；以其最终结果决定退出码。
                        tauri::async_runtime::spawn(async move {
                            let failed = core_stopped.await.unwrap_or(true)
                                || exit
                                    .try_state::<dev_mcp::Running>()
                                    .is_some_and(|server| server.failed());
                            exit.exit(i32::from(failed));
                        });
                    }))?;
                app.manage(running);
            }
            Ok(())
        })
        .build(context)?;
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            #[cfg(feature = "dev-mcp")]
            if let Some(server) = app.try_state::<dev_mcp::Running>()
                && !server.stopped()
            {
                api.prevent_exit();
                server.stop();
                return;
            }
            let host = app.state::<Arc<workspace::WorkspaceHost>>();
            if !host.stopped() {
                api.prevent_exit();
                host.stop();
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tauri::{
        ipc::{CallbackFn, InvokeBody},
        test::{INVOKE_KEY, get_ipc_response, mock_builder},
        webview::InvokeRequest,
    };

    fn invoke_from(window_label: &str, protocol_version: u32) -> Result<Value, Value> {
        invoke_command(
            window_label,
            "get_app_info",
            json!({ "protocolVersion": protocol_version }),
        )
    }

    fn invoke_command(window_label: &str, command: &str, request: Value) -> Result<Value, Value> {
        invoke_raw(window_label, command, json!({ "request": request }))
    }

    fn invoke_raw(window_label: &str, command: &str, body: Value) -> Result<Value, Value> {
        // 使用正式配置生成的 ACL，验证实际命令派发与窗口授权，而非直调函数。
        let (exited, exit) = std::sync::mpsc::channel();
        let host = crate::workspace::WorkspaceHost::start(
            Default::default(),
            |_| {},
            move |result| {
                let _ = exited.send(result);
            },
        )
        .unwrap();
        let app = super::configure(mock_builder())
            .manage(std::sync::Arc::new(host))
            .build(tauri::generate_context!())
            .expect("测试应用必须能够装配");
        let window =
            tauri::WebviewWindowBuilder::new(&app, window_label, tauri::WebviewUrl::default())
                // MockRuntime 不写 WebView profile，只需现有目录，避免创建真实应用数据目录。
                .data_directory(std::env::temp_dir())
                .build()
                .expect("测试窗口必须能够创建");
        let response = get_ipc_response(
            &window,
            InvokeRequest {
                cmd: command.to_owned(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "http://tauri.localhost".parse().expect("固定测试 URL 有效"),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_owned(),
            },
        );

        use tauri::Manager;
        app.state::<std::sync::Arc<crate::workspace::WorkspaceHost>>()
            .stop();
        exit.recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
            .unwrap();
        response.map(|body| body.deserialize().expect("命令成功结果必须为 JSON"))
    }

    #[test]
    fn main_window_receives_camel_case_app_info() {
        let response = invoke_from("main", layerlens_core::IPC_PROTOCOL_VERSION)
            .expect("主窗口应允许调用应用信息命令");

        assert_eq!(
            response,
            json!({
                "appName": "LayerLens",
                "appVersion": env!("CARGO_PKG_VERSION"),
                "protocolVersion": layerlens_core::IPC_PROTOCOL_VERSION,
            })
        );
    }

    #[cfg(feature = "dev-mcp")]
    #[test]
    fn ordinary_windows_cannot_report_development_observations() {
        for label in ["main", "untrusted"] {
            let error =
                invoke_raw(label, "dev_view_report", json!({"observation":{}})).unwrap_err();
            assert!(
                error
                    .as_str()
                    .is_some_and(|message| message.contains("not allowed"))
            );
        }
    }

    #[test]
    fn protocol_failure_preserves_structured_error() {
        let error = invoke_from("main", layerlens_core::IPC_PROTOCOL_VERSION + 1)
            .expect_err("不兼容协议应拒绝调用");

        assert_eq!(error["code"], "PROTOCOL_MISMATCH");
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
    }

    #[test]
    fn other_windows_cannot_invoke_app_command() {
        let error = invoke_from("untrusted", layerlens_core::IPC_PROTOCOL_VERSION)
            .expect_err("未授权窗口不应能够调用应用命令");

        assert!(
            error.as_str().is_some_and(|text| {
                text.contains("get_app_info") && text.contains("not allowed")
            })
        );
    }

    #[test]
    fn custom_titlebar_commands_are_main_window_only() {
        for command in [
            "is_maximized",
            "minimize",
            "toggle_maximize",
            "start_dragging",
            "close",
        ] {
            let command = format!("plugin:window|{command}");
            let blocked = invoke_raw("untrusted", &command, json!({})).unwrap_err();
            assert!(blocked.as_str().unwrap().contains("not allowed"));
            invoke_raw("main", &command, json!({})).expect("主窗口应具有标题栏所需的最小权限");
        }
        for command in ["destroy", "set_decorations", "set_fullscreen", "create"] {
            let blocked =
                invoke_raw("main", &format!("plugin:window|{command}"), json!({})).unwrap_err();
            assert!(blocked.as_str().unwrap().contains("not allowed"));
        }
    }

    #[test]
    fn workspace_commands_enforce_acl_and_protocol() {
        for (command, request) in [
            (
                "workspace_action",
                json!({ "protocolVersion": 2, "action": { "kind": "snapshot" } }),
            ),
            (
                "read_preview",
                json!({ "protocolVersion": 2, "documentId": "1", "revision": "1" }),
            ),
            ("choose_psd_files", json!({ "protocolVersion": 2 })),
            (
                "selection_request",
                json!({ "protocolVersion": 2, "sessionId": "00000000000000000000000000000000", "operation": { "kind": "tasks", "after": null, "limit": 32 } }),
            ),
            (
                "cleanup_request",
                json!({ "protocolVersion": 2, "sessionId": "00000000000000000000000000000000", "operation": { "kind": "confirm", "planId": "1" } }),
            ),
            (
                "read_layers",
                json!({"protocolVersion":2,"documentId":"1","revision":"1","query":{"kind":"list","offset":0,"limit":128}}),
            ),
        ] {
            let blocked = invoke_command("untrusted", command, request.clone()).unwrap_err();
            assert!(blocked.as_str().unwrap().contains("not allowed"));
            let incompatible = invoke_command("main", command, request).unwrap_err();
            assert_eq!(incompatible["code"], "PROTOCOL_MISMATCH");
        }
    }

    #[test]
    fn workspace_request_validates_shape_path_and_stale_preview() {
        let empty = invoke_command(
            "main",
            "workspace_action",
            json!({ "protocolVersion": 1, "action": { "kind": "snapshot" } }),
        )
        .unwrap();
        assert_eq!(empty["documents"], json!([]));
        assert_eq!(empty["sequence"], "1");
        let path = invoke_command(
            "main",
            "workspace_action",
            json!({ "protocolVersion": 1, "action": { "kind": "open", "path": "relative.psd" } }),
        )
        .unwrap_err();
        assert_eq!(path["code"], "INVALID_INPUT");
        let shape = invoke_command(
            "main",
            "workspace_action",
            json!({ "protocolVersion": 1, "action": { "kind": "snapshot", "extra": 1 } }),
        );
        assert!(shape.is_err());
        let stale = invoke_command(
            "main",
            "read_preview",
            json!({ "protocolVersion": 1, "documentId": "1", "revision": "1" }),
        )
        .unwrap_err();
        assert_eq!(stale["code"], "STALE_PREVIEW");
    }
}
