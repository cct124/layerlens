//! 开发观测的唯一类型来源；仅描述真实前端事实，不提供写入业务状态的接口。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[path = "render_model.rs"]
mod render_model;
pub(crate) use render_model::{UiRenderLab, UiRenderSample};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiDocument {
    pub id: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiCanvas {
    pub viewport: UiRect,
    pub image: Option<UiRect>,
    pub document_width: u32,
    pub document_height: u32,
    pub image_decoded: bool,
    pub paint_prepared: bool,
    pub image_error: Option<String>,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub panning: bool,
    pub pointer_captured: bool,
    pub draft: Option<UiRect>,
    pub alt: bool,
    pub snap_guides: u32,
    pub snap_x: Option<UiSnapTarget>,
    pub snap_y: Option<UiSnapTarget>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiSnapTarget {
    pub position: f64,
    pub layer_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiTask {
    pub id: String,
    pub status: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiCleanup {
    pub opened: bool,
    pub phase: Option<String>,
    pub plan_id: Option<String>,
    pub task_ids: Vec<String>,
    pub syncing: bool,
    pub sync_error: bool,
    pub pending: bool,
    pub rendered_tasks: u32,
    pub scrollable: bool,
    pub scrolled_to_end: bool,
    pub focus_inside: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiContent {
    pub task_id: Option<String>,
    pub snapshot_id: String,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiInputEvidence {
    pub pointer_events: String,
    pub key_events: String,
    pub last_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiWindowControl {
    pub target: String,
    pub label: String,
    pub rect: UiRect,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiWindow {
    pub focused: bool,
    pub focus_events: String,
    pub blur_events: String,
    pub width: u32,
    pub height: u32,
    pub device_pixel_ratio: f64,
    pub titlebar: Option<UiRect>,
    pub controls: Vec<UiWindowControl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiObservation {
    #[serde(default)]
    pub render_lab: Option<UiRenderLab>,
    pub instance_id: String,
    pub frontend_id: String,
    /// 前端挂载时 timeOrigin + now 的微秒整数，用于丢弃旧挂载或旧页面迟到报告。
    pub document_epoch: String,
    pub view_revision: String,
    pub core_sequence: Option<String>,
    pub session_id: Option<String>,
    pub active_document: Option<UiDocument>,
    pub selection_revision: Option<String>,
    pub snapshot_id: Option<String>,
    pub region: Option<UiRect>,
    pub tool: String,
    pub panel: String,
    pub menu: Option<String>,
    pub focus: Option<String>,
    pub cleanup: UiCleanup,
    pub canvas: Option<UiCanvas>,
    pub tasks: Vec<UiTask>,
    pub selection_running: bool,
    pub content: Option<UiContent>,
    pub input: UiInputEvidence,
    pub window: UiWindow,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttachRequest {
    pub driver_session_id: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriverInfo {
    pub driver_session_id: String,
    pub browser_version: String,
    pub driver_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum UiAction {
    /// target 为应用公开的稳定语义标识，不接受 CSS、XPath 或脚本。
    Click {
        target: String,
    },
    FillTaskName {
        text: String,
    },
    /// 同一次 W3C Actions 完成左键拖动；坐标为 viewport CSS 像素且限定画布内。
    /// hold 仅用于观察草稿后 Esc 取消，再调用 releaseInputs；提交使用 hold=false。
    Drag {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        hold: bool,
    },
    /// 一次 W3C Actions 完成至多 32 步画布手势，支持左键／中键组合与 Esc。
    /// hold 必须与末态是否保留按键一致；保留后只允许取消和释放。
    CanvasGesture {
        steps: Vec<GestureStep>,
        hold: bool,
    },
    /// 真实 W3C 滚轮输入；位置限定已显示画布，单次 deltaY 为非零且绝对值不超过 200。
    Scroll {
        x: i32,
        y: i32,
        delta_y: i32,
    },
    Key {
        key: UiKey,
        phase: KeyPhase,
    },
    ReleaseInputs {},
    /// 只允许 nativeWindows 实例的真实标题栏双击。
    TitlebarDoubleClick {},
    /// 无按键的真实画布移动，用于证明失焦后的平移／草稿不会继续。
    MovePointer {
        x: i32,
        y: i32,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PointerButton {
    Left,
    Middle,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum GestureStep {
    Move {
        x: i32,
        y: i32,
    },
    /// 连续绘制测量使用；单步 1–100 ms，仍受总步骤与坐标边界限制。
    MoveTimed {
        x: i32,
        y: i32,
        duration_ms: u32,
    },
    Down {
        button: PointerButton,
    },
    Up {
        button: PointerButton,
    },
    Escape,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum KeyPhase {
    Press,
    Down,
    Up,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum UiKey {
    Escape,
    Alt,
    Control,
    Shift,
    Tab,
    Enter,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    V,
    M,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputRequest {
    pub frontend_id: String,
    pub expected_view_revision: Option<String>,
    pub action: UiAction,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InputResult {
    pub before: UiObservation,
    pub input_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum UiCondition {
    Renderer {
        engine: String,
        phase: String,
        generation_at_least: String,
    },
    RenderInteraction {
        selected_layer_id: Option<u32>,
        auto_select: bool,
        ctrl: bool,
    },
    Ready {},
    Applied {
        at_least: String,
    },
    ImageReady {
        document_id: String,
        revision: String,
    },
    Panel {
        panel: String,
    },
    /// 工具状态与指定可信键盘事件均已投影，避免把驱动返回当作前端已处理。
    Tool {
        tool: String,
        key_events_at_least: String,
    },
    Menu {
        menu: Option<String>,
    },
    Draft {
        active: bool,
    },
    /// 等待真实指针事件到达且草稿、平移、捕获均结束。
    CanvasIdle {
        pointer_events_at_least: String,
    },
    Cleanup {
        phase: Option<String>,
    },
    CleanupSync {
        failed: bool,
    },
    SelectionIdle {},
    CleanupListEnd {},
    Task {
        task_id: String,
        status: String,
    },
    TaskCount {
        count: u32,
    },
    Selection {
        after: String,
        has_scope: bool,
    },
    Content {
        snapshot_id: String,
    },
    ViewRevision {
        at_least: String,
    },
    WindowFocus {
        focused: bool,
        events_after: Option<String>,
    },
    WindowLayout {
        width: u32,
        height: u32,
        maximized: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiWaitRequest {
    pub frontend_id: Option<String>,
    pub condition: UiCondition,
    pub timeout_ms: u32,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UiWaitResult {
    pub satisfied: bool,
    pub timed_out: bool,
    pub condition: UiCondition,
    pub last_state: Option<UiObservation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScreenshotRequest {
    pub frontend_id: String,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScreenshotResult {
    pub before: UiObservation,
    pub after: UiObservation,
    pub unchanged: bool,
    pub width: u32,
    pub height: u32,
    pub source: String,
}
