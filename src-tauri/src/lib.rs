//! Tauri 桌面入口，仅装配窗口、权限和共享核心的命令适配。

mod commands;

fn configure<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![commands::get_app_info])
}

/// 启动桌面事件循环，应用业务能力由共享核心提供。
///
/// # Errors
///
/// 窗口或运行时初始化失败时返回 Tauri 错误，由入口统一处理。
pub fn run() -> tauri::Result<()> {
    configure(tauri::Builder::default()).run(tauri::generate_context!())
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
        // 使用正式配置生成的 ACL，验证实际命令派发与窗口授权，而非直调函数。
        let app = super::configure(mock_builder())
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
                cmd: "get_app_info".to_owned(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "http://tauri.localhost".parse().expect("固定测试 URL 有效"),
                body: InvokeBody::Json(json!({
                    "request": { "protocolVersion": protocol_version }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_owned(),
            },
        );

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
}
