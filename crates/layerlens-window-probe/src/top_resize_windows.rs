//! 固定顶部手势的只读阶段采样；输入复用主模块边界，诊断完成不等同于缩放验收。
use super::*;
use crate::{
    WindowTarget,
    system::{InputGeometry, TopResizeDiagnosis, TopResizeSample, TopResizeStage},
};

static LAST_DIAGNOSIS: Mutex<Option<(usize, TopResizeDiagnosis)>> = Mutex::new(None);

pub(super) fn observe(main: usize) -> Result<Option<TopResizeDiagnosis>, SystemError> {
    owned_thread(main as HWND)?;
    Ok(LAST_DIAGNOSIS
        .lock()
        .map_err(|_| SystemError::from(("topResizeObservationUnavailable", None)))?
        .as_ref()
        .filter(|(owner, _)| *owner == main)
        .map(|(_, diagnosis)| diagnosis.clone()))
}

fn sample(main: HWND, stage: TopResizeStage) -> Result<TopResizeSample, SystemError> {
    let thread = owned_thread(main)?;
    let mut state = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let mut pointer = POINT::default();
    let mut outer = RECT::default();
    let mut client = RECT::default();
    // SAFETY: 所有缓冲区完整初始化且存活于调用期间；仅查询自有窗口、线程和指针。
    if unsafe { GetGUIThreadInfo(thread, &mut state) } == 0 {
        return Err(error("topResizeGuiThread"));
    }
    // SAFETY: 初始化的 POINT，仅查询当前指针。
    if unsafe { GetCursorPos(&mut pointer) } == 0 {
        return Err(error("topResizeCursor"));
    }
    // SAFETY: 自有窗口与初始化的可写 RECT。
    if unsafe { GetWindowRect(main, &mut outer) } == 0 {
        return Err(error("topResizeWindowRect"));
    }
    // SAFETY: 同上；读取物理客户区尺寸。
    if unsafe { GetClientRect(main, &mut client) } == 0 {
        return Err(error("topResizeClientRect"));
    }
    let mouse_capture = if state.hwndCapture.is_null() {
        WindowTarget::None
    } else if state.hwndCapture == main {
        WindowTarget::Main
    }
    // SAFETY: 只做系统句柄父子关系判断，不返回句柄。
    else if unsafe { IsChild(main, state.hwndCapture) } != 0 {
        WindowTarget::MainChild
    } else {
        WindowTarget::Other
    };
    Ok(TopResizeSample {
        stage,
        pointer: Point {
            x: pointer.x,
            y: pointer.y,
        },
        hit: None,
        geometry: InputGeometry {
            x: outer.left,
            y: outer.top,
            width: outer.right - outer.left,
            height: outer.bottom - outer.top,
            client_width: client.right - client.left,
            client_height: client.bottom - client.top,
        },
        in_move_size: state.flags & GUI_INMOVESIZE != 0,
        mouse_capture,
        // SAFETY: 仅读取当前按钮高位与前台窗口归属。
        left_button_down: unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON)) } < 0,
        // SAFETY: 系统只读前台查询，不公开窗口身份。
        foreground: unsafe { GetForegroundWindow() } == main,
    })
}

pub(super) fn diagnose(main: usize, distance: i32) -> Result<(), SystemError> {
    let window = main as HWND;
    check_input(window)?;
    // SAFETY: 自有窗口状态只读，拒绝系统模态和非普通窗口。
    if unsafe { IsZoomed(window) } != 0
        || unsafe { IsIconic(window) } != 0
        || dialog(main)?.is_some()
    {
        return Err(("resizeRequiresNormalWindow", None).into());
    }
    let mut rect = RECT::default();
    // SAFETY: 自有窗口与初始化的 RECT。
    if unsafe { GetWindowRect(window, &mut rect) } == 0 {
        return Err(error("resizeWindowRect"));
    }
    let start = resize_point(&rect, ResizeEdge::Top, 0)?;
    let end = Point {
        x: start.x,
        y: start
            .y
            .checked_sub(distance)
            .ok_or(SystemError::from(("resizeCoordinateOverflow", None)))?,
    };
    let mut diagnosis = TopResizeDiagnosis {
        start,
        end,
        samples: Vec::with_capacity(6),
        complete: false,
        error: None,
    };
    let mut press_attempted = false;
    let result = (|| {
        let mut before = sample(window, TopResizeStage::Prepared)?;
        before.hit = Some(probe_border(window, start, 0)?);
        diagnosis.samples.push(before);
        let (start_absolute, end_absolute, flags) = prepare_drag(window, start, end)?;
        let mut ready = sample(window, TopResizeStage::PointerReady)?;
        ready.hit = Some(probe_border(window, start, 0)?);
        diagnosis.samples.push(ready);
        verify_drag_target(window, start)?;
        press_attempted = true;
        send(&[mouse(Point { x: 0, y: 0 }, MOUSEEVENTF_LEFTDOWN)])?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let pressed = sample(window, TopResizeStage::Pressed)?;
            if pressed.left_button_down {
                diagnosis.samples.push(pressed);
                break;
            }
            if Instant::now() >= deadline {
                return Err(("topResizeButtonNotPressed", None).into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // 诊断允许客户区路径继续真实移动；系统模式未出现是证据，不能冒充边框通过。
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let mode = sample(window, TopResizeStage::SystemMode)?;
            if !mode.foreground {
                return Err(("foregroundChangedDuringDrag", None).into());
            }
            if mode.in_move_size || Instant::now() >= deadline {
                diagnosis.samples.push(mode);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        for step in 1..=8 {
            // SAFETY: 每步仅向仍为前台的自有窗口派发本次手势。
            if unsafe { GetForegroundWindow() } != window {
                return Err(("foregroundChangedDuringDrag", None).into());
            }
            send(&[mouse(
                Point {
                    x: start_absolute.x + (end_absolute.x - start_absolute.x) * step / 8,
                    y: start_absolute.y + (end_absolute.y - start_absolute.y) * step / 8,
                },
                flags,
            )])?;
            std::thread::sleep(Duration::from_millis(25));
        }
        diagnosis
            .samples
            .push(sample(window, TopResizeStage::Moved)?);
        Ok(())
    })();
    // 无论按下／移动在哪一步失败，均释放本次尝试的左键；不向外部窗口发送新的按下。
    let cleanup = if press_attempted {
        (|| {
            release(&[mouse(Point { x: 0, y: 0 }, MOUSEEVENTF_LEFTUP)])?;
            *LAST_DRAG_RELEASE
                .lock()
                .map_err(|_| SystemError::from(("dragTimingUnavailable", None)))? =
                Some(Instant::now());
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let released = sample(window, TopResizeStage::Released)?;
                if !released.left_button_down
                    && !released.in_move_size
                    && matches!(released.mouse_capture, WindowTarget::None)
                {
                    diagnosis.samples.push(released);
                    break;
                }
                if Instant::now() >= deadline {
                    diagnosis.samples.push(released);
                    return Err(("topResizeInputNotReleased", None).into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        })()
    } else {
        Ok(())
    };
    let result: Result<(), SystemError> = cleanup.and(result);
    diagnosis.complete = result.is_ok();
    diagnosis.error = result.as_ref().err().cloned();
    *LAST_DIAGNOSIS
        .lock()
        .map_err(|_| SystemError::from(("topResizeObservationUnavailable", None)))? =
        Some((main, diagnosis));
    result
}
