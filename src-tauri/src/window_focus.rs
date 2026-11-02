//! Windows 最小化恢复后的 WebView 焦点交接；不主动激活后台窗口。
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Runtime, WebviewWindow, WindowEvent};

/// 仅在系统已激活本窗口的最小化恢复过程中，将键盘焦点交还 WebView。
pub(crate) fn install<R: Runtime>(window: WebviewWindow<R>) -> tauri::Result<()> {
    let minimized = Arc::new(AtomicBool::new(window.is_minimized()?));
    let observed = window.clone();
    window.on_window_event(move |event| {
        if !matches!(event, WindowEvent::Resized(_)) {
            return;
        }
        let result = (|| -> tauri::Result<()> {
            let now = observed.is_minimized()?;
            let was = minimized.swap(now, Ordering::AcqRel);
            if was && !now {
                let restored = observed.clone();
                // Tauri 在主线程调用 run_on_main_thread 会立即执行。由后台任务投递，
                // 才能让本次恢复／激活消息先完成，再检查焦点；不定时轮询或重复抢焦点。
                tauri::async_runtime::spawn(async move {
                    let target = restored.clone();
                    let queued = restored.run_on_main_thread(move || {
                        let focus = (|| -> tauri::Result<()> {
                            let minimized = target.is_minimized()?;
                            let active = target.is_focused()?;
                            if !minimized && active {
                                let webview: &tauri::Webview<R> = target.as_ref();
                                webview.set_focus()?;
                            }
                            #[cfg(feature = "dev-mcp")]
                            eprintln!("operation=window_restore_webview_focus minimized={minimized} active={active}");
                            Ok(())
                        })();
                        if let Err(error) = focus {
                            eprintln!("operation=window_restore_webview_focus error={error}");
                        }
                    });
                    if let Err(error) = queued {
                        eprintln!("operation=window_restore_focus_queue error={error}");
                    }
                });
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("operation=window_restore_focus_observe error={error}");
        }
    });
    Ok(())
}
