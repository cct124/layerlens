//! Windows 自定义标题栏的最小客户区约束；仅复用 Tauri 安全窗口接口。
//! 配置定义客户区逻辑尺寸，系统拖动的最小外框另计实际不可见边距。
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{LogicalSize, PhysicalSize, Runtime, WebviewWindow, WindowEvent};

/// 为无系统装饰的窗口维护客户区下限，跨 DPI／主题变化及还原时重新读取边距。
///
/// Tao 0.35.3 的 set_inner_size 会计入阴影边距，但 WM_GETMINMAXINFO 未计入。
/// 在该依赖修复前，把实际外框与客户区之差补入原生拖动约束，不改变设计下限。
pub(crate) fn install<R: Runtime>(
    window: WebviewWindow<R>,
    minimum: LogicalSize<f64>,
) -> tauri::Result<()> {
    if window.is_decorated()? {
        return Ok(());
    }
    let dirty = Arc::new(AtomicBool::new(true));
    refresh(&window, minimum, &dirty)?;
    let observed = window.clone();
    window.on_window_event(move |event| {
        match event {
            WindowEvent::ScaleFactorChanged { .. } => {
                // Tao 先发布新 DPI，随后才更新窗口／客户区。此时读取边距仍属于旧屏幕。
                // 只标记失效，由实际 Resized 事件完成新几何下的约束更新。
                dirty.store(true, Ordering::Release);
                return;
            }
            WindowEvent::ThemeChanged(_) => {
                dirty.store(true, Ordering::Release);
            }
            WindowEvent::Resized(_) => {}
            _ => return,
        }
        if let Err(error) = refresh(&observed, minimum, &dirty) {
            eprintln!("operation=window_client_minimum error={error}");
        }
    });
    Ok(())
}

fn refresh<R: Runtime>(
    window: &WebviewWindow<R>,
    minimum: LogicalSize<f64>,
    dirty: &AtomicBool,
) -> tauri::Result<()> {
    // set_min_size 会触发尺寸重检；先消费标记，避免回调重复发起同一调整。
    if !dirty.swap(false, Ordering::AcqRel) {
        return Ok(());
    }
    let result = (|| {
        if window.is_maximized()? || window.is_minimized()? || window.is_fullscreen()? {
            // 最大化的外框不能用于普通窗口的边距；保留标记，待还原后应用。
            dirty.store(true, Ordering::Release);
            return Ok(());
        }
        let size = tracking_minimum(
            minimum,
            window.scale_factor()?,
            window.inner_size()?,
            window.outer_size()?,
        )?;
        window.set_min_size(Some(size))
    })();
    if result.is_err() {
        dirty.store(true, Ordering::Release);
    }
    result
}

fn tracking_minimum(
    minimum: LogicalSize<f64>,
    scale: f64,
    inner: PhysicalSize<u32>,
    outer: PhysicalSize<u32>,
) -> std::io::Result<PhysicalSize<u32>> {
    let dimension = |logical: f64, client: u32, frame: u32| {
        let pixels = (logical * scale).round();
        if !logical.is_finite()
            || logical <= 0.0
            || !scale.is_finite()
            || scale <= 0.0
            || !pixels.is_finite()
            || !(1.0..=f64::from(i32::MAX)).contains(&pixels)
        {
            return None;
        }
        let value = (pixels as u32).checked_add(frame.checked_sub(client)?)?;
        (value <= i32::MAX as u32).then_some(value)
    };
    match (
        dimension(minimum.width, inner.width, outer.width),
        dimension(minimum.height, inner.height, outer.height),
    ) {
        (Some(width), Some(height)) => Ok(PhysicalSize::new(width, height)),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "无法从实际窗口几何计算最小客户区约束。",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_insets_cannot_reduce_the_configured_client_minimum() {
        let minimum = LogicalSize::new(900.0, 600.0);
        let inner = PhysicalSize::new(1800, 1200);
        let outer = PhysicalSize::new(1826, 1215);
        let tracking = tracking_minimum(minimum, 2.0, inner, outer).unwrap();
        assert_eq!(tracking, PhysicalSize::new(1826, 1215));
        assert_eq!(tracking.width - (outer.width - inner.width), 1800);
        assert_eq!(tracking.height - (outer.height - inner.height), 1200);
        // DPI 改变后使用新边距，不能复用旧物理下限。
        assert_eq!(
            tracking_minimum(
                minimum,
                1.5,
                PhysicalSize::new(1350, 900),
                PhysicalSize::new(1370, 911)
            )
            .unwrap(),
            PhysicalSize::new(1370, 911)
        );
    }

    #[test]
    fn unshadowed_geometry_preserves_minimum_and_invalid_geometry_is_rejected() {
        let minimum = LogicalSize::new(900.0, 600.0);
        let size = PhysicalSize::new(1280, 800);
        assert_eq!(
            tracking_minimum(minimum, 1.0, size, size).unwrap(),
            PhysicalSize::new(900, 600)
        );
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(tracking_minimum(minimum, scale, size, size).is_err());
        }
        assert!(tracking_minimum(minimum, 1.0, size, PhysicalSize::new(1279, 800)).is_err());
        assert!(tracking_minimum(minimum, 1.0, size, PhysicalSize::new(u32::MAX, 800)).is_err());
    }
}
