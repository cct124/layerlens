//! 实验绘制观测，不包含文字、原始像素或状态写入入口。
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiRenderSample {
    pub generation: String,
    pub operation: String,
    pub submit_ms: f64,
    pub ready_ms: f64,
    pub readback_ms: f64,
    pub synchronized: bool,
    pub frame_interval_ms: Option<f64>,
    pub input_to_ready_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiRenderLab {
    pub engine: String,
    pub phase: String,
    pub generation: String,
    pub source_sha256: Option<String>,
    pub font_sha256: Option<String>,
    pub font_ready: bool,
    pub node_count: u32,
    pub resource_count: u32,
    pub selected_layer_id: Option<u32>,
    pub candidate_ids: Vec<u32>,
    pub hover_layer_id: Option<u32>,
    pub auto_select: bool,
    pub ctrl: bool,
    pub init_ms: f64,
    pub render_ms: f64,
    pub scene_read_ms: f64,
    pub assets_ms: f64,
    pub first_ready_ms: f64,
    pub text_mode: String,
    pub native_text_layers: u32,
    pub shared_text_layers: u32,
    pub unsupported_layers: u32,
    pub profile: String,
    pub source_layers: u32,
    pub drawn_layer_ids: Vec<u32>,
    pub raster_layer_ids: Vec<u32>,
    pub text_raster_layer_ids: Vec<u32>,
    pub degraded_layer_ids: Vec<u32>,
    pub unsupported_layer_ids: Vec<u32>,
    pub encoded_bytes: u32,
    pub alpha_bytes: u32,
    pub selection_ms: Option<f64>,
    pub hit_ms: Option<f64>,
    pub samples: Vec<super::UiRenderSample>,
    pub backend: String,
    pub error: Option<String>,
    pub limitations: Vec<String>,
}
