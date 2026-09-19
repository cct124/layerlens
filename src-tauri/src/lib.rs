//! Tauri 桌面入口，仅装配窗口、权限和共享核心的命令适配。

mod commands;
mod workspace;

use tauri::{Emitter, Manager};

fn configure<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::workspace_action,
            commands::read_preview,
            commands::read_layers,
            commands::choose_psd_files
        ])
}

/// 启动桌面事件循环，应用业务能力由共享核心提供。
///
/// # Errors
///
/// 窗口或运行时初始化失败时返回 Tauri 错误，由入口统一处理。
pub fn run() -> tauri::Result<()> {
    let app = configure(tauri::Builder::default())
        .setup(|app| {
            let events = app.handle().clone();
            let exit = app.handle().clone();
            let host = workspace::WorkspaceHost::start(
                Default::default(),
                move |snapshot| {
                    if let Err(error) = events.emit_to("main", workspace::EVENT, snapshot) {
                        eprintln!(
                            "operation=workspace_notify sequence={} error={error}",
                            snapshot.sequence
                        );
                    }
                },
                move |result| {
                    if let Err(error) = &result {
                        eprintln!("operation=workspace_shutdown error={}", error.message);
                    }
                    exit.exit(i32::from(result.is_err()));
                },
            )
            .map_err(|error| std::io::Error::other(error.message))?;
            app.manage(host);
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let host = app.state::<workspace::WorkspaceHost>();
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
            .manage(host)
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
                body: InvokeBody::Json(json!({ "request": request })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_owned(),
            },
        );

        use tauri::Manager;
        app.state::<crate::workspace::WorkspaceHost>().stop();
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
